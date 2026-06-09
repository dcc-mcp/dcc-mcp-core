//! Shared gateway ensure primitives.
//!
//! This crate provides the building blocks for gateway auto-launch in both
//! `dcc-mcp-cli` and `dcc-mcp-sidecar`:
//!
//! - Health check probes (`/health` endpoint)
//! - File-based launch lock with stale reclaim
//! - Detached process spawn (cross-platform)
//! - Gateway-ready polling
//! - PID file read/write/remove
//! - Cross-platform process liveness / termination
//!
//! The orchestrator-level `ensure_gateway_running` stays in each consumer
//! because CLI and sidecar have different lock-conflict behaviour and the
//! sidecar needs version-takeover logic (which depends on `dcc-mcp-gateway`).

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;

// ── Constants ──────────────────────────────────────────────────────────────

/// How long to wait for a single `/health` probe before timing out.
pub const HEALTH_TIMEOUT: Duration = Duration::from_millis(600);

/// Default lock staleness: reclaim a lock file older than this.
pub const DEFAULT_LOCK_STALE_SECS: u64 = 30;

/// Env var that overrides the launch-lock staleness threshold (seconds).
pub const ENV_GATEWAY_LAUNCH_LOCK_STALE_SECS: &str = "DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS";

/// Env var that overrides the gateway-ensure wait timeout (seconds).
pub const ENV_GATEWAY_ENSURE_TIMEOUT_SECS: &str = "DCC_MCP_GATEWAY_ENSURE_TIMEOUT_SECS";

/// Default ensure-wait timeout (15 s) when neither the caller nor the env var
/// provide a value.
pub const DEFAULT_ENSURE_TIMEOUT_SECS: u64 = 15;

// ── Health check ───────────────────────────────────────────────────────────

/// Check whether the gateway `/health` endpoint responds successfully.
pub async fn gateway_health_ok(host: &str, port: u16) -> bool {
    gateway_health_ok_with_timeout(host, port, HEALTH_TIMEOUT).await
}

/// Check whether the gateway `/health` endpoint responds successfully,
/// with a caller-specified timeout.
pub async fn gateway_health_ok_with_timeout(host: &str, port: u16, timeout: Duration) -> bool {
    let url = format!("http://{host}:{port}/health");
    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .get(url)
        .send()
        .await
        .is_ok_and(|resp| resp.status().is_success())
}

// ── Launch lock ────────────────────────────────────────────────────────────

/// A file-based launch lock that is automatically removed on drop.
#[derive(Debug)]
pub struct LaunchLock {
    _file: File,
    path: PathBuf,
}

impl Drop for LaunchLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Acquire the launch lock at `path`, using the default staleness threshold.
pub fn acquire_launch_lock(path: &Path) -> std::io::Result<LaunchLock> {
    acquire_launch_lock_with_stale(path, gateway_launch_lock_stale_after())
}

/// Acquire the launch lock at `path`, reclaiming locks older than `stale_after`.
pub fn acquire_launch_lock_with_stale(
    path: &Path,
    stale_after: Duration,
) -> std::io::Result<LaunchLock> {
    match create_launch_lock(path) {
        Ok(lock) => Ok(lock),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            if remove_stale_launch_lock(path, stale_after)? {
                create_launch_lock(path)
            } else {
                Err(err)
            }
        }
        Err(err) => Err(err),
    }
}

/// Create a new launch lock file (fails with `AlreadyExists` if one exists).
pub fn create_launch_lock(path: &Path) -> std::io::Result<LaunchLock> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|file| LaunchLock {
            _file: file,
            path: path.to_path_buf(),
        })
}

/// Read the launch lock staleness threshold from the env, falling back to
/// [`DEFAULT_LOCK_STALE_SECS`].
pub fn gateway_launch_lock_stale_after() -> Duration {
    std::env::var(ENV_GATEWAY_LAUNCH_LOCK_STALE_SECS)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(DEFAULT_LOCK_STALE_SECS))
}

/// Attempt to remove a stale launch lock. Returns `Ok(true)` if the file was
/// removed, `Ok(false)` if it was not stale.
pub fn remove_stale_launch_lock(path: &Path, stale_after: Duration) -> std::io::Result<bool> {
    if !launch_lock_is_stale(path, stale_after)? {
        return Ok(false);
    }
    // Re-check immediately before unlinking (race protection).
    if !launch_lock_is_stale(path, stale_after)? {
        return Ok(false);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err) => Err(err),
    }
}

/// Check whether a launch lock file is older than `stale_after`.
pub fn launch_lock_is_stale(path: &Path, stale_after: Duration) -> std::io::Result<bool> {
    let modified = match std::fs::metadata(path) {
        Ok(meta) => meta.modified()?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => return Err(err),
    };
    let age = modified.elapsed().unwrap_or_default();
    Ok(age >= stale_after)
}

// ── Timeout resolution ─────────────────────────────────────────────────────

/// Resolve the effective ensure timeout: env var
/// `DCC_MCP_GATEWAY_ENSURE_TIMEOUT_SECS` takes priority, then the
/// caller-supplied `explicit_secs`, then the crate default (15 s).
pub fn resolve_ensure_timeout_secs(explicit_secs: u64) -> u64 {
    std::env::var(ENV_GATEWAY_ENSURE_TIMEOUT_SECS)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or({
            if explicit_secs > 0 {
                explicit_secs
            } else {
                DEFAULT_ENSURE_TIMEOUT_SECS
            }
        })
}

// ── Wait for ready ─────────────────────────────────────────────────────────

/// Poll the gateway `/health` endpoint until it responds successfully or the
/// `timeout` expires.
pub async fn wait_gateway_ready(host: &str, port: u16, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if gateway_health_ok(host, port).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    anyhow::bail!(
        "gateway did not become healthy at http://{host}:{port}/health within {timeout:?}"
    )
}

// ── Spawn ──────────────────────────────────────────────────────────────────

/// Spawn a detached gateway process from the given `exe` binary.
///
/// Returns the child process ID. The caller is responsible for resolving the
/// binary path (e.g. via gateway discovery or `current_exe`).
pub fn spawn_detached_gateway(
    exe: &Path,
    args: &[OsString],
    registry_dir: &Path,
) -> anyhow::Result<u32> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .env("DCC_MCP_REGISTRY_DIR", registry_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }

    let child = cmd
        .spawn()
        .with_context(|| format!("spawning gateway from {}", exe.display()))?;
    Ok(child.id())
}

/// Build the command-line arguments for a standalone gateway process.
pub fn gateway_command_args(
    host: &str,
    port: u16,
    name: Option<&str>,
    remote_host: &str,
    remote_port: u16,
    gateway_idle_timeout_secs: u64,
) -> Vec<OsString> {
    let mut cargs = vec![
        OsString::from("gateway"),
        OsString::from("--host"),
        OsString::from(host),
        OsString::from("--port"),
        OsString::from(port.to_string()),
        OsString::from("--remote-host"),
        OsString::from(remote_host),
        OsString::from("--remote-port"),
        OsString::from(remote_port.to_string()),
        OsString::from("--gateway-idle-timeout-secs"),
        OsString::from(gateway_idle_timeout_secs.to_string()),
    ];
    if let Some(name) = name
        && !name.trim().is_empty()
    {
        cargs.push(OsString::from("--name"));
        cargs.push(OsString::from(name));
    }
    cargs
}

// ── PID file ───────────────────────────────────────────────────────────────

/// Read a PID from a pidfile, returning `None` if the file doesn't exist or
/// can't be parsed.
pub fn read_pid_from_pidfile(pidfile: Option<&Path>) -> Option<u32> {
    let path = pidfile?;
    let raw = std::fs::read_to_string(path).ok()?;
    raw.trim().parse::<u32>().ok()
}

/// Write a PID to a pidfile, creating parent directories as needed.
pub fn write_pidfile(path: &Path, pid: u32) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating pidfile dir {}", parent.display()))?;
    }
    std::fs::write(path, format!("{pid}\n"))
        .with_context(|| format!("writing pidfile {}", path.display()))
}

/// Remove the pidfile if it exists.
pub fn remove_pidfile(pidfile: Option<&Path>) {
    if let Some(path) = pidfile {
        let _ = std::fs::remove_file(path);
    }
}

// ── Process utilities (subprocess-based, no foreign deps) ──────────────────

/// Check whether a process with the given PID is still running.
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        // `tasklist /FI "PID eq <pid>" /NH` returns output containing the
        // PID if the process exists, or "INFO: No tasks..." on stderr if not.
        // Exit code is always 0, so we check stdout for the PID string.
        let output = Command::new("tasklist")
            .args([
                "/FI",
                &format!("PID eq {pid}"),
                "/NH", // No headers
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout.contains(&pid.to_string())
            }
            Err(_) => false,
        }
    }
    #[cfg(unix)]
    {
        // `kill -0 <pid>` succeeds if the process exists and we can signal it.
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Send a termination signal to the process with the given PID.
pub fn stop_process(pid: u32) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("taskkill /PID {pid}"))?;
        if !status.success() {
            // Exit code 128 means the process was not found — that's OK.
            if status.code() != Some(128) {
                anyhow::bail!("taskkill /PID {pid} exited with {status}");
            }
        }
    }
    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("kill {pid}"))?;
        if !status.success() {
            anyhow::bail!("kill {pid} exited with {status}");
        }
    }
    Ok(())
}

// ── Default registry dir ───────────────────────────────────────────────────

/// Default registry directory used by the gateway and sidecar.
pub fn default_registry_dir() -> PathBuf {
    std::env::var("DCC_MCP_REGISTRY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("dcc-mcp-core-registry"))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── PID file tests ─────────────────────────────────────────────────

    #[test]
    fn test_read_pid_from_pidfile_none() {
        assert!(read_pid_from_pidfile(None).is_none());
    }

    #[test]
    fn test_read_pid_from_pidfile_invalid() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("bad.pid");
        std::fs::write(&path, "not-a-number").unwrap();
        assert!(read_pid_from_pidfile(Some(&path)).is_none());
    }

    #[test]
    fn test_read_pid_from_pidfile_valid() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.pid");
        write_pidfile(&path, 12345).unwrap();
        assert_eq!(read_pid_from_pidfile(Some(&path)), Some(12345));
    }

    #[test]
    fn test_read_pid_from_pidfile_with_newline() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.pid");
        std::fs::write(&path, "12345\n").unwrap();
        assert_eq!(read_pid_from_pidfile(Some(&path)), Some(12345));
    }

    #[test]
    fn test_write_then_read_pidfile() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("gateway.pid");
        write_pidfile(&path, 99999).unwrap();
        assert_eq!(read_pid_from_pidfile(Some(&path)), Some(99999));
        remove_pidfile(Some(&path));
        assert!(read_pid_from_pidfile(Some(&path)).is_none());
    }

    // ── Process tests ──────────────────────────────────────────────────

    #[test]
    fn test_is_process_alive_current() {
        assert!(is_process_alive(std::process::id()));
    }

    #[test]
    fn test_is_process_alive_invalid() {
        // PIDs are well under 10 million on every OS.
        assert!(!is_process_alive(9_999_999));
    }

    // ── Launch lock tests ──────────────────────────────────────────────

    #[test]
    fn test_launch_lock_create_and_drop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.lock");
        {
            let lock = acquire_launch_lock(&path).unwrap();
            assert!(path.exists());
            // Second acquire must fail with AlreadyExists.
            let err = acquire_launch_lock(&path).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
            drop(lock);
        }
        // After drop, lock file should be removed.
        assert!(!path.exists());
    }

    #[test]
    fn test_launch_lock_stale_removed_correctly() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stale.lock");
        {
            let _f = File::create(&path).unwrap();
        }
        // File is brand new, so it should NOT be stale with a 1 s threshold.
        let result = acquire_launch_lock_with_stale(&path, Duration::from_secs(1));
        assert!(result.is_err());
    }

    #[test]
    fn test_launch_lock_stale_reclaim_with_zero_secs() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stale0.lock");
        let _f = File::create(&path).unwrap();
        drop(_f);
        let lock = acquire_launch_lock_with_stale(&path, Duration::from_secs(0)).unwrap();
        drop(lock);
        assert!(!path.exists());
    }

    // ── Health check tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_gateway_health_ok_unreachable() {
        assert!(!gateway_health_ok("127.0.0.1", 19999).await);
    }

    // ── Command args tests ─────────────────────────────────────────────

    #[test]
    fn test_gateway_command_args_minimal() {
        let argv: Vec<String> = gateway_command_args("127.0.0.1", 9765, None, "0.0.0.0", 59765, 30)
            .into_iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(argv[0] == "gateway");
        assert!(argv.contains(&"--port".to_string()));
        assert!(argv.contains(&"9765".to_string()));
    }

    #[test]
    fn test_gateway_command_args_with_name() {
        let argv: Vec<String> = gateway_command_args(
            "127.0.0.1",
            9765,
            Some("test-gateway"),
            "0.0.0.0",
            59765,
            30,
        )
        .into_iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
        assert!(argv.contains(&"--name".to_string()));
        assert!(argv.contains(&"test-gateway".to_string()));
    }

    #[test]
    fn test_gateway_command_args_does_not_include_registry_dir_flag() {
        let argv: Vec<String> = gateway_command_args(
            "127.0.0.1",
            9765,
            Some("gateway-for-test"),
            "0.0.0.0",
            59765,
            30,
        )
        .into_iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
        assert!(
            !argv.iter().any(|arg| arg == "--registry-dir"),
            "auto-launched gateway should inherit DCC_MCP_REGISTRY_DIR instead of exposing --registry-dir"
        );
    }

    // ── Default registry dir tests ─────────────────────────────────────

    #[test]
    fn test_default_registry_dir_is_not_empty() {
        let dir = default_registry_dir();
        assert!(!dir.as_os_str().is_empty());
        assert!(dir.is_absolute());
    }

    // ── Stale lock reclaim integration tests ───────────────────────────

    #[test]
    fn stale_gateway_launch_lock_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway-launch.lock");
        std::fs::write(&path, "stale").unwrap();

        let lock = acquire_launch_lock_with_stale(&path, Duration::ZERO)
            .expect("stale launch lock should be reclaimed");

        assert!(path.exists());
        drop(lock);
        assert!(!path.exists());
    }

    #[test]
    fn fresh_gateway_launch_lock_stays_single_flight() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway-launch.lock");
        std::fs::write(&path, "busy").unwrap();

        let err = match acquire_launch_lock_with_stale(&path, Duration::from_secs(3600)) {
            Ok(_) => panic!("fresh launch lock should remain busy"),
            Err(err) => err,
        };

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(path.exists());
    }
}
