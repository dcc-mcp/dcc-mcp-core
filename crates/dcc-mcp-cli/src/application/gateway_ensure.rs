//! Gateway health check and auto-launch helpers.
//!
//! Ported from `dcc-mcp-sidecar`'s `gateway_daemon::launcher` and simplified:
//! - No version takeover (CLI is not a DCC adapter).
//! - No FileRegistry dependency.
//! - No adapter_version / adapter_dcc fields.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::Serialize;

/// How long to wait for a single `/health` probe before timing out.
const HEALTH_TIMEOUT: Duration = Duration::from_millis(600);

/// Default lock staleness: reclaim a lock file older than this.
const DEFAULT_LOCK_STALE_SECS: u64 = 30;

const ENV_GATEWAY_LAUNCH_LOCK_STALE_SECS: &str = "DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS";

/// Outcome of an `ensure_gateway_running` call.
#[derive(Debug, Clone, Serialize)]
pub struct EnsureResult {
    pub host: String,
    pub port: u16,
    pub already_running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

/// Parameters for `ensure_gateway_running`.
#[derive(Debug, Clone)]
pub struct EnsureGatewayArgs {
    pub host: String,
    pub port: u16,
    pub name: Option<String>,
    pub registry_dir: PathBuf,
    pub remote_host: String,
    pub remote_port: u16,
    pub gateway_idle_timeout_secs: u64,
    pub gateway_bin: Option<PathBuf>,
    pub wait_timeout_secs: u64,
    /// Optional path for the PID file written after a successful start.
    pub pidfile: Option<PathBuf>,
}

/// Ensure the gateway is reachable at `host:port`, launching it once if needed.
pub async fn ensure_gateway_running(args: &EnsureGatewayArgs) -> anyhow::Result<EnsureResult> {
    if args.port == 0 {
        anyhow::bail!("gateway port must be non-zero");
    }

    if gateway_health_ok(&args.host, args.port).await {
        return Ok(EnsureResult {
            host: args.host.clone(),
            port: args.port,
            already_running: true,
            pid: read_pid_from_pidfile(args.pidfile.as_deref()),
        });
    }

    std::fs::create_dir_all(&args.registry_dir)
        .with_context(|| format!("creating registry dir {}", args.registry_dir.display()))?;
    let lock_path = args.registry_dir.join("gateway-launch.lock");
    match acquire_launch_lock(&lock_path) {
        Ok(_lock) => {
            // Double-check after acquiring the lock (race protection).
            if gateway_health_ok(&args.host, args.port).await {
                return Ok(EnsureResult {
                    host: args.host.clone(),
                    port: args.port,
                    already_running: true,
                    pid: read_pid_from_pidfile(args.pidfile.as_deref()),
                });
            }
            let pid = spawn_detached_gateway(args)?;

            wait_gateway_ready(
                &args.host,
                args.port,
                Duration::from_secs(args.wait_timeout_secs),
            )
            .await?;

            // Release lock after gateway is confirmed ready.
            drop(_lock);

            // Write PID file so stop/status commands can find the process.
            if let Some(ref pidfile) = args.pidfile {
                write_pidfile(pidfile, pid)?;
            }

            Ok(EnsureResult {
                host: args.host.clone(),
                port: args.port,
                already_running: false,
                pid: Some(pid),
            })
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::bail!(
                "another process is launching the gateway (lock: {})",
                lock_path.display()
            );
        }
        Err(err) => {
            Err(err).with_context(|| format!("creating launch lock {}", lock_path.display()))?
        }
    }
}

/// Check whether the gateway `/health` endpoint responds successfully.
pub async fn gateway_health_ok(host: &str, port: u16) -> bool {
    gateway_health_ok_with_timeout(host, port, HEALTH_TIMEOUT).await
}

async fn gateway_health_ok_with_timeout(host: &str, port: u16, timeout: Duration) -> bool {
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

// ── Launch lock ──────────────────────────────────────────────────────────

#[derive(Debug)]
struct LaunchLock {
    _file: File,
    path: PathBuf,
}

impl Drop for LaunchLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn acquire_launch_lock(path: &Path) -> std::io::Result<LaunchLock> {
    acquire_launch_lock_with_stale(path, gateway_launch_lock_stale_after())
}

fn acquire_launch_lock_with_stale(
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

fn create_launch_lock(path: &Path) -> std::io::Result<LaunchLock> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|file| LaunchLock {
            _file: file,
            path: path.to_path_buf(),
        })
}

fn gateway_launch_lock_stale_after() -> Duration {
    std::env::var(ENV_GATEWAY_LAUNCH_LOCK_STALE_SECS)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_LOCK_STALE_SECS))
}

fn remove_stale_launch_lock(path: &Path, stale_after: Duration) -> std::io::Result<bool> {
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

fn launch_lock_is_stale(path: &Path, stale_after: Duration) -> std::io::Result<bool> {
    let modified = match std::fs::metadata(path) {
        Ok(meta) => meta.modified()?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => return Err(err),
    };
    let age = modified.elapsed().unwrap_or_default();
    Ok(age >= stale_after)
}

// ── Spawn ────────────────────────────────────────────────────────────────

fn resolve_gateway_bin(args: &EnsureGatewayArgs) -> PathBuf {
    args.gateway_bin
        .clone()
        .unwrap_or_else(|| std::env::current_exe().expect("resolving current executable"))
}

fn spawn_detached_gateway(args: &EnsureGatewayArgs) -> anyhow::Result<u32> {
    let exe = resolve_gateway_bin(args);
    let mut cmd = Command::new(&exe);
    cmd.args(gateway_command_args(args))
        .env("DCC_MCP_REGISTRY_DIR", &args.registry_dir)
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

fn gateway_command_args(args: &EnsureGatewayArgs) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;
    let mut cargs = vec![
        OsString::from("gateway"),
        OsString::from("--host"),
        OsString::from(&args.host),
        OsString::from("--port"),
        OsString::from(args.port.to_string()),
        OsString::from("--remote-host"),
        OsString::from(&args.remote_host),
        OsString::from("--remote-port"),
        OsString::from(args.remote_port.to_string()),
        OsString::from("--gateway-idle-timeout-secs"),
        OsString::from(args.gateway_idle_timeout_secs.to_string()),
    ];
    if let Some(ref name) = args.name
        && !name.trim().is_empty()
    {
        cargs.push(OsString::from("--name"));
        cargs.push(OsString::from(name));
    }
    cargs
}

async fn wait_gateway_ready(host: &str, port: u16, timeout: Duration) -> anyhow::Result<()> {
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

// ── PID file ─────────────────────────────────────────────────────────────

/// Read a PID from a pidfile, returning `None` if the file doesn't exist or
/// can't be parsed.
pub fn read_pid_from_pidfile(pidfile: Option<&Path>) -> Option<u32> {
    let path = pidfile?;
    let raw = std::fs::read_to_string(path).ok()?;
    raw.trim().parse::<u32>().ok()
}

fn write_pidfile(path: &Path, pid: u32) -> anyhow::Result<()> {
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

// ── Process utilities (subprocess-based, no foreign deps) ────────────────

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

// ── Default registry dir ────────────────────────────────────────────────

/// Default registry directory used by the gateway and sidecar.
pub fn default_registry_dir() -> PathBuf {
    std::env::var("DCC_MCP_REGISTRY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("dcc-mcp-core-registry"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
        // Integration test: write → read → remove.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("gateway.pid");
        write_pidfile(&path, 99999).unwrap();
        assert_eq!(read_pid_from_pidfile(Some(&path)), Some(99999));
        remove_pidfile(Some(&path));
        assert!(read_pid_from_pidfile(Some(&path)).is_none());
    }

    #[test]
    fn test_is_process_alive_current() {
        assert!(is_process_alive(std::process::id()));
    }

    #[test]
    fn test_is_process_alive_invalid() {
        // PIDs are well under 10 million on every OS.
        assert!(!is_process_alive(9_999_999));
    }

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
        // Simulate a stale lock: the file exists but with a very old mtime.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stale.lock");
        {
            let _f = File::create(&path).unwrap();
        }
        // File is brand new, so it should NOT be stale with a 1s threshold.
        let result = acquire_launch_lock_with_stale(&path, Duration::from_secs(1));
        // The file is fresh (<1s old), so AlreadyExists should be returned
        // (it shouldn't be reclaimed).
        assert!(result.is_err());
    }

    #[test]
    fn test_launch_lock_stale_reclaim_with_zero_secs() {
        // With a 0-second stale threshold, any existing lock is immediately stale.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stale0.lock");
        let _f = File::create(&path).unwrap();
        drop(_f);
        // Zero-second stale → any file is stale.
        let lock = acquire_launch_lock_with_stale(&path, Duration::from_secs(0)).unwrap();
        drop(lock);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn test_gateway_health_ok_unreachable() {
        assert!(!gateway_health_ok("127.0.0.1", 19999).await);
    }

    #[test]
    fn test_gateway_command_args_minimal() {
        let args = EnsureGatewayArgs {
            host: "127.0.0.1".into(),
            port: 9765,
            name: None,
            registry_dir: PathBuf::from("/tmp"),
            remote_host: "0.0.0.0".into(),
            remote_port: 59765,
            gateway_idle_timeout_secs: 30,
            gateway_bin: None,
            wait_timeout_secs: 10,
            pidfile: None,
        };
        let argv: Vec<String> = gateway_command_args(&args)
            .into_iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(argv[0] == "gateway");
        assert!(argv.contains(&"--port".to_string()));
        assert!(argv.contains(&"9765".to_string()));
    }

    #[test]
    fn test_default_registry_dir_is_not_empty() {
        let dir = default_registry_dir();
        assert!(!dir.as_os_str().is_empty());
        assert!(dir.is_absolute());
    }
}
