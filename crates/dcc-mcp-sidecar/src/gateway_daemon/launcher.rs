use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context as _;
use dcc_mcp_gateway::{ElectionInfo, has_newer_sentinel, is_newer_election};
use dcc_mcp_gateway_ensure as ensure;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};

const GATEWAY_SENTINEL_STALE_SECS: u64 = 30;
pub const AUTO_ENSURE_GATEWAY_IDLE_TIMEOUT_SECS: u64 = 300;

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

    if ensure::gateway_health_ok(&opts.host, opts.port).await {
        try_version_takeover(opts).await?;
        return Ok(());
    }

    std::fs::create_dir_all(&opts.registry_dir)
        .with_context(|| format!("creating registry dir {}", opts.registry_dir.display()))?;
    let lock_path = opts.registry_dir.join("gateway-launch.lock");
    match ensure::acquire_launch_lock(&lock_path) {
        Ok(_lock) => {
            if ensure::gateway_health_ok(&opts.host, opts.port).await {
                try_version_takeover(opts).await?;
                return Ok(());
            }
            spawn_detached_gateway_now(opts)?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            tracing::info!(
                path = %lock_path.display(),
                "another process is launching the gateway"
            );
        }
        Err(err) => return Err(err).with_context(|| format!("creating {}", lock_path.display())),
    }

    ensure::wait_gateway_ready(
        &opts.host,
        opts.port,
        Duration::from_secs(ensure::resolve_ensure_timeout_secs(0)),
    )
    .await
}

// ── Version takeover ───────────────────────────────────────────────────────

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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        if !ensure::gateway_health_ok(&opts.host, opts.port).await {
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
    match ensure::acquire_launch_lock(&lock_path) {
        Ok(_lock) => {
            if ensure::gateway_health_ok(&opts.host, opts.port).await {
                return Ok(());
            }
            spawn_detached_gateway_now(opts)?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            tracing::info!(
                path = %lock_path.display(),
                "another process is launching the gateway"
            );
        }
        Err(err) => return Err(err).with_context(|| format!("creating {}", lock_path.display())),
    }
    ensure::wait_gateway_ready(
        &opts.host,
        opts.port,
        Duration::from_secs(ensure::resolve_ensure_timeout_secs(0)),
    )
    .await
}

// ── Spawn helper ───────────────────────────────────────────────────────────

fn spawn_detached_gateway_now(opts: &EnsureGatewayOptions) -> anyhow::Result<()> {
    let exe =
        std::env::current_exe().context("resolving current executable for detached gateway")?;
    let cmd_args = ensure::gateway_command_args(
        &opts.host,
        opts.port,
        opts.name.as_deref(),
        &opts.remote_host,
        opts.remote_port,
        opts.gateway_idle_timeout_secs,
    );
    ensure::spawn_detached_gateway(&exe, &cmd_args, &opts.registry_dir)?;
    tracing::info!(port = opts.port, "spawned standalone gateway process");
    Ok(())
}

// ── Re-exports ─────────────────────────────────────────────────────────────

pub use ensure::gateway_health_ok_with_timeout;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_launch_gateway_args_do_not_include_registry_dir_flag() {
        let argv: Vec<String> = ensure::gateway_command_args(
            "127.0.0.1",
            9765,
            Some("gateway-for-test"),
            "0.0.0.0",
            59765,
            30,
        )
        .into_iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

        assert!(
            !argv.iter().any(|arg| arg == "--registry-dir"),
            "auto-launched gateway should inherit DCC_MCP_REGISTRY_DIR instead of exposing --registry-dir in the command line"
        );
        assert!(argv.iter().any(|arg| arg == "gateway"));
        assert!(argv.iter().any(|arg| arg == "--name"));
    }

    #[test]
    fn stale_gateway_launch_lock_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway-launch.lock");
        std::fs::write(&path, "stale").unwrap();

        let lock = ensure::acquire_launch_lock_with_stale(&path, Duration::ZERO)
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

        let err = match ensure::acquire_launch_lock_with_stale(&path, Duration::from_secs(3600)) {
            Ok(_) => panic!("fresh launch lock should remain busy"),
            Err(err) => err,
        };

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(path.exists());
    }
}
