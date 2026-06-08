//! Gateway lifecycle commands: start, stop, status.
//!
//! Wraps `gateway_ensure` for `start` and adds PID-based stop/status.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use serde::Serialize;

use crate::application::gateway_ensure;

/// Parameters shared across `start`, `stop`, and `status`.
#[derive(Debug, Clone)]
pub struct GatewayCtrlArgs {
    pub host: String,
    pub port: u16,
    pub registry_dir: PathBuf,
    pub pidfile: PathBuf,
    /// Used only by `start`.
    pub start_opts: Option<GatewayStartOpts>,
}

#[derive(Debug, Clone)]
pub struct GatewayStartOpts {
    pub name: Option<String>,
    pub remote_host: String,
    pub remote_port: u16,
    pub gateway_idle_timeout_secs: u64,
    pub gateway_bin: Option<PathBuf>,
    pub wait_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GatewayStatus {
    pub host: String,
    pub port: u16,
    pub healthy: bool,
    pub pid: Option<u32>,
    pub alive: bool,
    pub running: bool,
}

/// Start the gateway (ensure it's running; alias for `ensure`).
pub async fn gateway_start(args: &GatewayCtrlArgs) -> anyhow::Result<gateway_ensure::EnsureResult> {
    let start = args.start_opts.as_ref().context("start options required")?;
    let ensure_args = gateway_ensure::EnsureGatewayArgs {
        host: args.host.clone(),
        port: args.port,
        name: start.name.clone(),
        registry_dir: args.registry_dir.clone(),
        remote_host: start.remote_host.clone(),
        remote_port: start.remote_port,
        gateway_idle_timeout_secs: start.gateway_idle_timeout_secs,
        gateway_bin: start.gateway_bin.clone(),
        wait_timeout_secs: start.wait_timeout_secs,
        pidfile: Some(args.pidfile.clone()),
    };
    gateway_ensure::ensure_gateway_running(&ensure_args).await
}

/// Stop the gateway by sending a termination signal to the PID recorded
/// in the pidfile, then wait for the health endpoint to become unreachable.
pub async fn gateway_stop(
    args: &GatewayCtrlArgs,
    wait_timeout_secs: u64,
) -> anyhow::Result<GatewayStatus> {
    let pid = gateway_ensure::read_pid_from_pidfile(Some(&args.pidfile));

    let status_before = gateway_status(args).await;
    if !status_before.running {
        return Ok(status_before);
    }

    if let Some(pid) = pid
        && gateway_ensure::is_process_alive(pid)
    {
        gateway_ensure::stop_process(pid)?;
        // Wait for the health endpoint to go dark.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(wait_timeout_secs);
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if !gateway_ensure::gateway_health_ok(&args.host, args.port).await {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "gateway at {host}:{port} did not stop within {wait_timeout_secs}s",
                    host = args.host,
                    port = args.port
                );
            }
        }
    }

    gateway_ensure::remove_pidfile(Some(&args.pidfile));
    Ok(gateway_status(args).await)
}

/// Query the gateway status: health check + PID liveness.
pub async fn gateway_status(args: &GatewayCtrlArgs) -> GatewayStatus {
    let healthy = gateway_ensure::gateway_health_ok(&args.host, args.port).await;
    let pid = gateway_ensure::read_pid_from_pidfile(Some(&args.pidfile));
    let alive = pid.is_some_and(gateway_ensure::is_process_alive);
    GatewayStatus {
        host: args.host.clone(),
        port: args.port,
        healthy,
        pid,
        alive,
        running: healthy,
    }
}

/// Build the default PID file path under the registry directory.
pub fn default_pidfile(registry_dir: &std::path::Path) -> PathBuf {
    registry_dir.join("gateway.pid")
}
