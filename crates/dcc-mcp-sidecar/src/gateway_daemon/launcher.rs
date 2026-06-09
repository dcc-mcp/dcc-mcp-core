use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use dcc_mcp_gateway::{ElectionInfo, has_newer_sentinel, is_newer_election};
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};

const ENV_GATEWAY_LAUNCH_LOCK_STALE_SECS: &str = "DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS";
const DEFAULT_GATEWAY_LAUNCH_LOCK_STALE_SECS: u64 = 30;
const GATEWAY_SENTINEL_STALE_SECS: u64 = 30;
/// Env var that overrides the gateway-ensure wait timeout.
const ENV_GATEWAY_ENSURE_TIMEOUT_SECS: &str = "DCC_MCP_GATEWAY_ENSURE_TIMEOUT_SECS";
/// Default ensure-wait timeout (15 s) when the env var is not set.
const DEFAULT_ENSURE_TIMEOUT_SECS: u64 = 15;

/// Resolve the effective ensure-wait timeout: env var
/// `DCC_MCP_GATEWAY_ENSURE_TIMEOUT_SECS` > explicit arg > crate default (15 s).
fn resolve_ensure_timeout_secs(explicit_secs: u64) -> u64 {
    std::env::var(ENV_GATEWAY_ENSURE_TIMEOUT_SECS)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or_else(|| {
            if explicit_secs > 0 {
                explicit_secs
            } else {
                DEFAULT_ENSURE_TIMEOUT_SECS
            }
        })
}

/// Helpers for auto-launching the standalone gateway from inside another
/// process (the per-DCC sidecar / embedded server).
#[derive(Debug, Clone)]
pub struct EnsureGatewayOptions {
    pub host: String,
    pub port: u16,
    pub name: Option<String>,
    pub registry_dir: PathBuf,
    pub remote_host: String,
    pub remote_port: u16,
    /// Crate (dcc-mcp-server) version to advertise for version-aware takeover.
    pub crate_version: Option<String>,
    /// Adapter package version (e.g. dcc_mcp_maya = "0.3.0").
    pub adapter_version: Option<String>,
    /// Adapter DCC type (e.g. "maya", "blender").
    pub adapter_dcc: Option<String>,
    /// Gateway idle timeout after last backend exits (0 = persist mode).
    pub gateway_idle_timeout_secs: u64,
}

/// Ensure the machine-wide gateway is reachable, launching it once if needed.
///
/// When a gateway is already running, checks whether this sidecar carries a
/// newer crate/adapter version.  If it does, the sidecar writes a sentinel
/// entry into the FileRegistry to trigger the gateway's voluntary yield
/// (the gateway checks `has_newer_sentinel` every 15 s), then waits for
/// the old gateway to exit before spawning a replacement.
pub async fn ensure_gateway_running(opts: &EnsureGatewayOptions) -> anyhow::Result<()> {
    if opts.port == 0 {
        return Ok(());
    }

    if gateway_health_ok(&opts.host, opts.port).await {
        try_version_takeover(opts).await?;
        return Ok(());
    }

    std::fs::create_dir_all(&opts.registry_dir)
        .with_context(|| format!("creating registry dir {}", opts.registry_dir.display()))?;
    let lock_path = opts.registry_dir.join("gateway-launch.lock");
    match acquire_launch_lock(&lock_path) {
        Ok(_lock) => {
            if gateway_health_ok(&opts.host, opts.port).await {
                try_version_takeover(opts).await?;
                return Ok(());
            }
            spawn_detached_gateway(opts)?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            tracing::info!(
                path = %lock_path.display(),
                "another process is launching the gateway"
            );
        }
        Err(err) => return Err(err).with_context(|| format!("creating {}", lock_path.display())),
    }

    wait_gateway_ready(
        &opts.host,
        opts.port,
        Duration::from_secs(resolve_ensure_timeout_secs(0)),
    )
    .await
}

/// When the gateway port is already occupied, check whether we carry a newer
/// version than the running gateway.  If we do, write a sentinel entry with
/// our version info so the gateway's cleanup loop (`has_newer_sentinel`)
/// triggers a voluntary yield, then wait for the port to free up and spawn
/// the replacement.
async fn try_version_takeover(opts: &EnsureGatewayOptions) -> anyhow::Result<()> {
    let Some(crate_version) = opts.crate_version.as_deref() else {
        return Ok(());
    };
    if crate_version.is_empty() {
        return Ok(());
    }

    let _ = std::fs::create_dir_all(&opts.registry_dir);
    let reg = FileRegistry::new(&opts.registry_dir)
        .with_context(|| format!("opening FileRegistry at {}", opts.registry_dir.display()))?;

    let our_info = ElectionInfo::new(
        crate_version,
        opts.adapter_version.as_deref(),
        opts.adapter_dcc.as_deref(),
    );

    // If the running gateway is already newer than us, nothing to do.
    let stale_timeout = Duration::from_secs(GATEWAY_SENTINEL_STALE_SECS);
    if has_newer_sentinel(&reg, our_info, stale_timeout) {
        tracing::info!(
            our_version = crate_version,
            adapter = ?opts.adapter_version,
            "running gateway sentinel is newer; no takeover needed"
        );
        return Ok(());
    }

    // Determine if we are newer than the existing sentinel.
    let sentinels = reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE);
    let should_takeover = sentinels.iter().any(|entry| {
        if entry.is_stale(stale_timeout) {
            return false;
        }
        let Some(their_version) = entry.version.as_deref() else {
            return false;
        };
        let their_info = ElectionInfo::new(
            their_version,
            entry.adapter_version.as_deref(),
            entry.adapter_dcc.as_deref(),
        );
        is_newer_election(our_info, their_info)
    });

    if !should_takeover {
        return Ok(());
    }

    tracing::info!(
        crate_version = crate_version,
        adapter_version = ?opts.adapter_version,
        adapter_dcc = ?opts.adapter_dcc,
        "sidecar is newer than current gateway — triggering version takeover"
    );

    // Write a sentinel with our version.  The gateway's 15 s cleanup loop
    // calls `has_newer_sentinel` and will voluntarily yield.
    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, &opts.host, opts.port);
    sentinel.version = Some(crate_version.to_string());
    sentinel.adapter_version = opts.adapter_version.clone();
    sentinel.adapter_dcc = opts.adapter_dcc.clone();
    reg.register(sentinel)
        .with_context(|| "registering takeover sentinel")?;

    // Wait for the old gateway to yield (up to ~20 s for the 15 s cleanup
    // interval + grace).
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if !gateway_health_ok(&opts.host, opts.port).await {
            tracing::info!("old gateway yielded — spawning new gateway");
            return spawn_gateway_with_lock(opts).await;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    tracing::warn!("old gateway did not yield within 20 s; continuing with existing gateway");
    Ok(())
}

/// Acquire the launch lock and spawn the gateway, then wait for it to be
/// ready.  Used after a version takeover has freed the port.
async fn spawn_gateway_with_lock(opts: &EnsureGatewayOptions) -> anyhow::Result<()> {
    std::fs::create_dir_all(&opts.registry_dir)
        .with_context(|| format!("creating registry dir {}", opts.registry_dir.display()))?;
    let lock_path = opts.registry_dir.join("gateway-launch.lock");
    match acquire_launch_lock(&lock_path) {
        Ok(_lock) => {
            if gateway_health_ok(&opts.host, opts.port).await {
                return Ok(());
            }
            spawn_detached_gateway(opts)?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            tracing::info!(
                path = %lock_path.display(),
                "another process is launching the gateway"
            );
        }
        Err(err) => return Err(err).with_context(|| format!("creating {}", lock_path.display())),
    }
    wait_gateway_ready(
        &opts.host,
        opts.port,
        Duration::from_secs(resolve_ensure_timeout_secs(0)),
    )
    .await
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

pub(super) async fn gateway_health_ok(host: &str, port: u16) -> bool {
    gateway_health_ok_with_timeout(host, port, Duration::from_millis(600)).await
}

pub(super) async fn gateway_health_ok_with_timeout(
    host: &str,
    port: u16,
    timeout: Duration,
) -> bool {
    let url = format!("http://{host}:{port}/health");
    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(client) => client,
        Err(_) => return false,
    };
    client
        .get(url)
        .send()
        .await
        .is_ok_and(|resp| resp.status().is_success())
}

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
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_GATEWAY_LAUNCH_LOCK_STALE_SECS))
}

fn remove_stale_launch_lock(path: &Path, stale_after: Duration) -> std::io::Result<bool> {
    if !launch_lock_is_stale(path, stale_after)? {
        return Ok(false);
    }

    // Re-check immediately before unlinking so a fresh lock created by another
    // sidecar is not removed after the stale check wins a race.
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

fn spawn_detached_gateway(opts: &EnsureGatewayOptions) -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("resolving current executable")?;
    let mut cmd = Command::new(exe);
    cmd.args(gateway_command_args(opts))
        .env("DCC_MCP_REGISTRY_DIR", &opts.registry_dir)
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

    cmd.spawn().context("spawning standalone gateway")?;
    tracing::info!(port = opts.port, "spawned standalone gateway process");
    Ok(())
}

fn gateway_command_args(opts: &EnsureGatewayOptions) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("gateway"),
        OsString::from("--host"),
        OsString::from(&opts.host),
        OsString::from("--port"),
        OsString::from(opts.port.to_string()),
        OsString::from("--remote-host"),
        OsString::from(&opts.remote_host),
        OsString::from("--remote-port"),
        OsString::from(opts.remote_port.to_string()),
    ];
    if let Some(name) = opts.name.as_ref().filter(|name| !name.trim().is_empty()) {
        args.push(OsString::from("--name"));
        args.push(OsString::from(name));
    }
    args.push(OsString::from("--gateway-idle-timeout-secs"));
    args.push(OsString::from(opts.gateway_idle_timeout_secs.to_string()));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_launch_gateway_args_do_not_include_registry_dir_flag() {
        let opts = EnsureGatewayOptions {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: Some("gateway-for-test".to_string()),
            registry_dir: PathBuf::from(r"C:\tmp\dcc-mcp-registry"),
            remote_host: "0.0.0.0".to_string(),
            remote_port: 59765,
            crate_version: None,
            adapter_version: None,
            adapter_dcc: None,
            gateway_idle_timeout_secs: 30,
        };

        let args: Vec<String> = gateway_command_args(&opts)
            .into_iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(
            !args.iter().any(|arg| arg == "--registry-dir"),
            "auto-launched gateway should inherit DCC_MCP_REGISTRY_DIR instead of exposing --registry-dir in the command line"
        );
        assert!(args.iter().any(|arg| arg == "gateway"));
        assert!(args.iter().any(|arg| arg == "--name"));
    }

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
