use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use dcc_mcp_gateway::{ElectionInfo, has_newer_sentinel, is_newer_election, is_newer_version};
use dcc_mcp_gateway_ensure as ensure;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};
use serde_json::Value;

const GATEWAY_SENTINEL_STALE_SECS: u64 = 30;
const GATEWAY_TAKEOVER_WAIT_SECS: u64 = 20;
const RESIDENT_GATEWAY_PROBE_TIMEOUT_MS: u64 = 800;
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

    let started = Instant::now();
    std::fs::create_dir_all(&opts.registry_dir)
        .with_context(|| format!("creating registry dir {}", opts.registry_dir.display()))?;
    let lock_path = opts.registry_dir.join("gateway-launch.lock");
    let mut launch = None;
    match ensure::acquire_launch_lock(&lock_path) {
        Ok(_lock) => {
            if ensure::gateway_health_ok(&opts.host, opts.port).await {
                try_version_takeover(opts).await?;
                return Ok(());
            }
            launch = Some(spawn_detached_gateway_now(opts)?);
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            tracing::info!(
                path = %lock_path.display(),
                "another process is launching the gateway"
            );
        }
        Err(err) => return Err(err).with_context(|| format!("creating {}", lock_path.display())),
    }

    wait_gateway_ready(opts, &lock_path, launch.as_ref(), started).await
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
        try_direct_gateway_takeover(opts, crate_version).await?;
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
    if wait_for_gateway_down(
        &opts.host,
        opts.port,
        Duration::from_secs(GATEWAY_TAKEOVER_WAIT_SECS),
    )
    .await
    {
        tracing::info!("old gateway yielded — spawning new gateway");
        return spawn_gateway_with_lock(opts).await;
    }

    tracing::warn!(
        timeout_secs = GATEWAY_TAKEOVER_WAIT_SECS,
        "old gateway did not yield within timeout; continuing with existing gateway"
    );
    Ok(())
}

async fn try_direct_gateway_takeover(
    opts: &EnsureGatewayOptions,
    crate_version: &str,
) -> anyhow::Result<()> {
    let Some(resident_version) = probe_resident_gateway_version(&opts.host, opts.port).await else {
        tracing::debug!(
            host = %opts.host,
            port = opts.port,
            "gateway is healthy but resident version could not be probed; no takeover requested"
        );
        return Ok(());
    };

    if !is_newer_version(crate_version, &resident_version) {
        tracing::debug!(
            our_version = crate_version,
            resident_version = %resident_version,
            "resident gateway is not older; no direct takeover needed"
        );
        return Ok(());
    }

    tracing::info!(
        crate_version = crate_version,
        resident_version = %resident_version,
        adapter_version = ?opts.adapter_version,
        adapter_dcc = ?opts.adapter_dcc,
        "sidecar is newer than resident gateway without a local sentinel; requesting direct yield"
    );

    if !request_gateway_yield(&opts.host, opts.port, crate_version).await {
        tracing::warn!(
            host = %opts.host,
            port = opts.port,
            "resident gateway did not accept direct yield request; continuing with existing gateway"
        );
        return Ok(());
    }

    if wait_for_gateway_down(
        &opts.host,
        opts.port,
        Duration::from_secs(GATEWAY_TAKEOVER_WAIT_SECS),
    )
    .await
    {
        tracing::info!("resident gateway yielded — spawning new gateway");
        return spawn_gateway_with_lock(opts).await;
    }

    tracing::warn!(
        timeout_secs = GATEWAY_TAKEOVER_WAIT_SECS,
        "resident gateway accepted yield but did not exit within timeout; continuing with existing gateway"
    );
    Ok(())
}

async fn probe_resident_gateway_version(host: &str, port: u16) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(RESIDENT_GATEWAY_PROBE_TIMEOUT_MS))
        .build()
        .ok()?;

    let url = format!("http://{host}:{port}/admin/api/health");
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.json::<Value>().await.ok()?;
    resident_gateway_version_from_admin_health(&body).map(str::to_string)
}

fn resident_gateway_version_from_admin_health(body: &Value) -> Option<&str> {
    body.get("version")
        .and_then(Value::as_str)
        .or_else(|| {
            body.pointer("/gateway/current/version")
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn request_gateway_yield(host: &str, port: u16, crate_version: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!(error = %err, "failed to build HTTP client for gateway yield");
            return false;
        }
    };

    let url = format!("http://{host}:{port}/gateway/yield");
    let response = match client
        .post(url)
        .json(&serde_json::json!({
            "challenger_version": crate_version,
            "reason": "version_preempt_missing_registry_sentinel"
        }))
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => {
            tracing::debug!(error = %err, "gateway yield request failed");
            return false;
        }
    };

    response.status().is_success()
}

async fn wait_for_gateway_down(host: &str, port: u16, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if !ensure::gateway_health_ok(host, port).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

/// Acquire the launch lock and spawn the gateway, then wait for it to be
/// ready.  Used after a version takeover has freed the port.
async fn spawn_gateway_with_lock(opts: &EnsureGatewayOptions) -> anyhow::Result<()> {
    let started = Instant::now();
    std::fs::create_dir_all(&opts.registry_dir)
        .with_context(|| format!("creating registry dir {}", opts.registry_dir.display()))?;
    let lock_path = opts.registry_dir.join("gateway-launch.lock");
    let mut launch = None;
    match ensure::acquire_launch_lock(&lock_path) {
        Ok(_lock) => {
            if ensure::gateway_health_ok(&opts.host, opts.port).await {
                return Ok(());
            }
            launch = Some(spawn_detached_gateway_now(opts)?);
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            tracing::info!(
                path = %lock_path.display(),
                "another process is launching the gateway"
            );
        }
        Err(err) => return Err(err).with_context(|| format!("creating {}", lock_path.display())),
    }
    wait_gateway_ready(opts, &lock_path, launch.as_ref(), started).await
}

async fn wait_gateway_ready(
    opts: &EnsureGatewayOptions,
    lock_path: &Path,
    launch: Option<&ensure::GatewayLaunchArtifacts>,
    started: Instant,
) -> anyhow::Result<()> {
    ensure::wait_gateway_ready_with_diagnostics(
        &opts.host,
        opts.port,
        Duration::from_secs(ensure::resolve_ensure_timeout_secs(0)),
        ensure::GatewayReadyDiagnostics {
            registry_dir: Some(&opts.registry_dir),
            launch_lock: Some(lock_path),
            launch,
            started: Some(started),
            gateway_idle_timeout_secs: Some(opts.gateway_idle_timeout_secs),
            remote_host: Some(&opts.remote_host),
            remote_port: Some(opts.remote_port),
        },
    )
    .await
}

// ── Spawn helper ───────────────────────────────────────────────────────────

fn spawn_detached_gateway_now(
    opts: &EnsureGatewayOptions,
) -> anyhow::Result<ensure::GatewayLaunchArtifacts> {
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
    let mut context = ensure::GatewayLaunchContext::gateway(
        &opts.host,
        opts.port,
        &opts.remote_host,
        opts.remote_port,
        opts.gateway_idle_timeout_secs,
    );
    context.adapter_dcc = opts.adapter_dcc.clone();
    context.adapter_version = opts.adapter_version.clone();
    context.crate_version = opts.crate_version.clone();
    let artifacts =
        ensure::spawn_detached_gateway_with_context(&exe, &cmd_args, &opts.registry_dir, context)?;
    tracing::info!(
        port = opts.port,
        pid = artifacts.pid,
        executable = %artifacts.executable.display(),
        stdout_log = %artifacts.stdout_log.display(),
        stderr_log = %artifacts.stderr_log.display(),
        manifest = %artifacts.manifest_path.display(),
        "spawned standalone gateway process"
    );
    Ok(artifacts)
}

// ── Re-exports ─────────────────────────────────────────────────────────────

pub use ensure::gateway_health_ok_with_timeout;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_health_probe_reads_running_gateway_version_without_local_sentinel() {
        let body = serde_json::json!({
            "ok": true,
            "version": "0.18.20",
            "gateway": {
                "current": {
                    "version": "0.18.20",
                    "adapter_version": null,
                    "adapter_dcc": null,
                    "metadata": {
                        "gateway_process_exe": "F:\\dcc-mcp-core-maya-verify\\target\\release\\dcc-mcp-server.exe"
                    }
                }
            }
        });
        let version = resident_gateway_version_from_admin_health(&body)
            .expect("admin health should expose the resident gateway version");

        assert_eq!(version, "0.18.20");
    }

    #[test]
    fn admin_health_probe_uses_current_gateway_version_when_top_level_missing() {
        let body = serde_json::json!({
            "gateway": {
                "current": {
                    "version": "0.18.20",
                    "adapter_version": "0.1.31",
                    "adapter_dcc": "maya"
                }
            }
        });
        let version = resident_gateway_version_from_admin_health(&body)
            .expect("gateway.current.version should be enough for resident probing");

        assert_eq!(version, "0.18.20");
    }

    #[test]
    fn resident_gateway_probe_detects_older_cross_registry_gateway() {
        assert!(is_newer_version("0.19.17", "0.18.20"));
    }

    #[test]
    fn resident_gateway_probe_does_not_preempt_same_gateway_version() {
        assert!(!is_newer_version("0.19.17", "0.19.17"));
    }

    #[tokio::test]
    async fn resident_gateway_probe_reads_admin_health_endpoint() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = axum::Router::new().route(
            "/admin/api/health",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({
                    "ok": true,
                    "version": "0.18.20",
                    "gateway": {
                        "current": {
                            "version": "0.18.20",
                            "adapter_version": null,
                            "adapter_dcc": null
                        }
                    }
                }))
            }),
        );
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        let version = probe_resident_gateway_version("127.0.0.1", port)
            .await
            .expect("probe should read admin health");

        assert_eq!(version, "0.18.20");
        let _ = shutdown_tx.send(());
        let _ = server.await;
    }

    #[tokio::test]
    async fn direct_yield_request_sends_challenger_version() {
        use std::sync::{Arc, Mutex};

        let seen = Arc::new(Mutex::new(None::<Value>));
        let seen_for_handler = Arc::clone(&seen);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = axum::Router::new().route(
            "/gateway/yield",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let seen_for_handler = Arc::clone(&seen_for_handler);
                async move {
                    *seen_for_handler.lock().unwrap() = Some(body);
                    axum::Json(serde_json::json!({"ok": true, "handoff": true}))
                }
            }),
        );
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        assert!(request_gateway_yield("127.0.0.1", port, "0.19.17").await);

        let body = seen
            .lock()
            .unwrap()
            .clone()
            .expect("yield request body should be captured");
        assert_eq!(body["challenger_version"], "0.19.17");
        assert_eq!(body["reason"], "version_preempt_missing_registry_sentinel");
        let _ = shutdown_tx.send(());
        let _ = server.await;
    }

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
