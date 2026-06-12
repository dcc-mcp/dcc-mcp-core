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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::Serialize;

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

/// Context recorded in gateway autolaunch manifests and wait-timeout errors.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GatewayLaunchContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_idle_timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_dcc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_version: Option<String>,
}

impl GatewayLaunchContext {
    pub fn gateway(
        host: &str,
        port: u16,
        remote_host: &str,
        remote_port: u16,
        gateway_idle_timeout_secs: u64,
    ) -> Self {
        Self {
            host: Some(host.to_string()),
            port: Some(port),
            remote_host: Some(remote_host.to_string()),
            remote_port: Some(remote_port),
            gateway_idle_timeout_secs: Some(gateway_idle_timeout_secs),
            ..Self::default()
        }
    }

    pub fn health_url(&self) -> Option<String> {
        match (self.host.as_deref(), self.port) {
            (Some(host), Some(port)) => Some(gateway_health_url(host, port)),
            _ => None,
        }
    }
}

/// Files and process details produced by a detached gateway launch.
#[derive(Debug, Clone)]
pub struct GatewayLaunchArtifacts {
    pub pid: u32,
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    pub manifest_path: PathBuf,
}

/// Extra context for a gateway-ready wait failure.
#[derive(Debug, Clone, Copy, Default)]
pub struct GatewayReadyDiagnostics<'a> {
    pub registry_dir: Option<&'a Path>,
    pub launch_lock: Option<&'a Path>,
    pub launch: Option<&'a GatewayLaunchArtifacts>,
    pub started: Option<Instant>,
    pub gateway_idle_timeout_secs: Option<u64>,
    pub remote_host: Option<&'a str>,
    pub remote_port: Option<u16>,
}

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

/// Build the gateway `/health` URL used by diagnostics and probes.
pub fn gateway_health_url(host: &str, port: u16) -> String {
    format!("http://{host}:{port}/health")
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
    wait_gateway_ready_with_diagnostics(host, port, timeout, GatewayReadyDiagnostics::default())
        .await
}

/// Poll the gateway `/health` endpoint with extra failure diagnostics.
pub async fn wait_gateway_ready_with_diagnostics(
    host: &str,
    port: u16,
    timeout: Duration,
    diagnostics: GatewayReadyDiagnostics<'_>,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if gateway_health_ok(host, port).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    anyhow::bail!(
        "{}",
        gateway_ready_timeout_message(host, port, timeout, diagnostics)
    )
}

/// Build the detailed timeout message used by wait helpers.
pub fn gateway_ready_timeout_message(
    host: &str,
    port: u16,
    timeout: Duration,
    diagnostics: GatewayReadyDiagnostics<'_>,
) -> String {
    let mut parts = vec![format!(
        "gateway did not become healthy at {} within {:?}",
        gateway_health_url(host, port),
        timeout
    )];
    if let Some(started) = diagnostics.started {
        parts.push(format!("elapsed_ms={}", started.elapsed().as_millis()));
    }
    if let Some(registry_dir) = diagnostics.registry_dir {
        parts.push(format!("registry_dir={}", registry_dir.display()));
    }
    if let Some(lock_path) = diagnostics.launch_lock {
        parts.push(format!("launch_lock={}", lock_path.display()));
        parts.push(format!("launch_lock_exists={}", lock_path.exists()));
    }
    if let Some(idle_timeout) = diagnostics.gateway_idle_timeout_secs {
        parts.push(format!("gateway_idle_timeout_secs={idle_timeout}"));
    }
    match (diagnostics.remote_host, diagnostics.remote_port) {
        (Some(remote_host), Some(remote_port)) => {
            parts.push(format!("remote={remote_host}:{remote_port}"));
        }
        (Some(remote_host), None) => {
            parts.push(format!("remote_host={remote_host}"));
        }
        (None, Some(remote_port)) => {
            parts.push(format!("remote_port={remote_port}"));
        }
        (None, None) => {}
    }
    if let Some(launch) = diagnostics.launch {
        let args = launch
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        parts.extend([
            format!("spawned_pid={}", launch.pid),
            format!("executable={}", launch.executable.display()),
            format!("args={args}"),
            format!("stdout_log={}", launch.stdout_log.display()),
            format!("stderr_log={}", launch.stderr_log.display()),
            format!("manifest={}", launch.manifest_path.display()),
        ]);
    } else {
        parts.push("spawned_pid=<none>".to_string());
    }
    parts.join("; ")
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
    let artifacts = spawn_detached_gateway_with_context(
        exe,
        args,
        registry_dir,
        GatewayLaunchContext::default(),
    )?;
    Ok(artifacts.pid)
}

/// Spawn a detached gateway and keep stdout/stderr plus a JSON manifest in
/// the registry directory for post-failure debugging.
pub fn spawn_detached_gateway_with_context(
    exe: &Path,
    args: &[OsString],
    registry_dir: &Path,
    context: GatewayLaunchContext,
) -> anyhow::Result<GatewayLaunchArtifacts> {
    let prefix = gateway_autolaunch_prefix(&context);
    let stdout_log = registry_dir.join(format!("{prefix}-stdout.log"));
    let stderr_log = registry_dir.join(format!("{prefix}-stderr.log"));
    let manifest_path = registry_dir.join(format!("{prefix}.json"));
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .env("DCC_MCP_REGISTRY_DIR", registry_dir)
        .stdin(Stdio::null())
        .stdout(log_stdio_or_null(&stdout_log, "stdout"))
        .stderr(log_stdio_or_null(&stderr_log, "stderr"));

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    let child = cmd
        .spawn()
        .with_context(|| format!("spawning gateway from {}", exe.display()))?;
    let artifacts = GatewayLaunchArtifacts {
        pid: child.id(),
        executable: exe.to_path_buf(),
        args: args.to_vec(),
        stdout_log,
        stderr_log,
        manifest_path,
    };
    write_launch_manifest(registry_dir, &context, &artifacts);
    Ok(artifacts)
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

fn gateway_autolaunch_prefix(context: &GatewayLaunchContext) -> String {
    context
        .port
        .map(|port| format!("gateway-autolaunch-{port}"))
        .unwrap_or_else(|| "gateway-autolaunch".to_string())
}

fn log_stdio_or_null(path: &Path, stream_name: &str) -> Stdio {
    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => Stdio::from(file),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                stream = stream_name,
                error = %err,
                "gateway autolaunch stdio log could not be opened; discarding stream"
            );
            Stdio::null()
        }
    }
}

#[derive(Serialize)]
struct GatewayLaunchManifest {
    pid: u32,
    spawned_at_unix: u64,
    executable: String,
    args: Vec<String>,
    registry_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    health_url: Option<String>,
    #[serde(flatten)]
    context: GatewayLaunchContext,
    stdout_log: String,
    stderr_log: String,
}

fn write_launch_manifest(
    registry_dir: &Path,
    context: &GatewayLaunchContext,
    artifacts: &GatewayLaunchArtifacts,
) {
    let args = artifacts
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();
    let manifest = GatewayLaunchManifest {
        pid: artifacts.pid,
        spawned_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        executable: artifacts.executable.display().to_string(),
        args,
        registry_dir: registry_dir.display().to_string(),
        health_url: context.health_url(),
        context: context.clone(),
        stdout_log: artifacts.stdout_log.display().to_string(),
        stderr_log: artifacts.stderr_log.display().to_string(),
    };
    match serde_json::to_vec_pretty(&manifest)
        .map_err(std::io::Error::other)
        .and_then(|bytes| std::fs::write(&artifacts.manifest_path, bytes))
    {
        Ok(()) => {}
        Err(err) => tracing::warn!(
            path = %artifacts.manifest_path.display(),
            error = %err,
            "gateway autolaunch manifest could not be written"
        ),
    }
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

    #[test]
    fn gateway_ready_timeout_message_includes_debug_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("gateway-launch.lock");
        std::fs::write(&lock_path, "busy").unwrap();
        let launch = GatewayLaunchArtifacts {
            pid: 4242,
            executable: PathBuf::from("dcc-mcp-server"),
            args: gateway_command_args(
                "127.0.0.1",
                9765,
                Some("gateway-for-test"),
                "0.0.0.0",
                59765,
                30,
            ),
            stdout_log: dir.path().join("gateway-autolaunch-9765-stdout.log"),
            stderr_log: dir.path().join("gateway-autolaunch-9765-stderr.log"),
            manifest_path: dir.path().join("gateway-autolaunch-9765.json"),
        };

        let msg = gateway_ready_timeout_message(
            "127.0.0.1",
            9765,
            Duration::from_secs(15),
            GatewayReadyDiagnostics {
                registry_dir: Some(dir.path()),
                launch_lock: Some(&lock_path),
                launch: Some(&launch),
                started: Some(Instant::now()),
                gateway_idle_timeout_secs: Some(30),
                remote_host: Some("0.0.0.0"),
                remote_port: Some(59765),
            },
        );

        assert!(msg.contains("http://127.0.0.1:9765/health"));
        assert!(msg.contains("registry_dir="));
        assert!(msg.contains("launch_lock="));
        assert!(msg.contains("launch_lock_exists=true"));
        assert!(msg.contains("gateway_idle_timeout_secs=30"));
        assert!(msg.contains("remote=0.0.0.0:59765"));
        assert!(msg.contains("spawned_pid=4242"));
        assert!(msg.contains("stdout_log="));
        assert!(msg.contains("stderr_log="));
        assert!(msg.contains("manifest="));
    }

    #[test]
    fn spawn_detached_gateway_with_context_writes_debug_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let args = vec![OsString::from("--help")];
        let mut context = GatewayLaunchContext::gateway("127.0.0.1", 19765, "0.0.0.0", 59765, 30);
        context.adapter_dcc = Some("maya".to_string());
        context.adapter_version = Some("0.1.0-test".to_string());
        context.crate_version = Some("0.18.19-test".to_string());

        let artifacts =
            spawn_detached_gateway_with_context(&exe, &args, dir.path(), context).unwrap();

        assert_eq!(artifacts.executable, exe);
        assert_eq!(artifacts.args, args);
        assert!(artifacts.pid > 0);
        assert!(artifacts.stdout_log.exists());
        assert!(artifacts.stderr_log.exists());
        assert!(artifacts.manifest_path.exists());
        assert_eq!(
            artifacts
                .stdout_log
                .file_name()
                .and_then(|name| name.to_str()),
            Some("gateway-autolaunch-19765-stdout.log")
        );
        assert_eq!(
            artifacts
                .stderr_log
                .file_name()
                .and_then(|name| name.to_str()),
            Some("gateway-autolaunch-19765-stderr.log")
        );
        assert_eq!(
            artifacts
                .manifest_path
                .file_name()
                .and_then(|name| name.to_str()),
            Some("gateway-autolaunch-19765.json")
        );

        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&artifacts.manifest_path).unwrap()).unwrap();
        assert_eq!(manifest["pid"].as_u64(), Some(artifacts.pid as u64));
        assert_eq!(
            manifest["executable"].as_str(),
            Some(artifacts.executable.display().to_string().as_str())
        );
        assert_eq!(manifest["args"].as_array().map(Vec::len), Some(1));
        assert_eq!(manifest["args"][0].as_str(), Some("--help"));
        assert_eq!(
            manifest["registry_dir"].as_str(),
            Some(dir.path().display().to_string().as_str())
        );
        assert_eq!(
            manifest["health_url"].as_str(),
            Some("http://127.0.0.1:19765/health")
        );
        assert_eq!(manifest["host"].as_str(), Some("127.0.0.1"));
        assert_eq!(manifest["port"].as_u64(), Some(19765));
        assert_eq!(manifest["remote_host"].as_str(), Some("0.0.0.0"));
        assert_eq!(manifest["remote_port"].as_u64(), Some(59765));
        assert_eq!(manifest["gateway_idle_timeout_secs"].as_u64(), Some(30));
        assert_eq!(manifest["adapter_dcc"].as_str(), Some("maya"));
        assert_eq!(manifest["adapter_version"].as_str(), Some("0.1.0-test"));
        assert_eq!(manifest["crate_version"].as_str(), Some("0.18.19-test"));
        assert_eq!(
            manifest["stdout_log"].as_str(),
            Some(artifacts.stdout_log.display().to_string().as_str())
        );
        assert_eq!(
            manifest["stderr_log"].as_str(),
            Some(artifacts.stderr_log.display().to_string().as_str())
        );
        assert!(
            manifest["spawned_at_unix"]
                .as_u64()
                .is_some_and(|value| value > 0)
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
