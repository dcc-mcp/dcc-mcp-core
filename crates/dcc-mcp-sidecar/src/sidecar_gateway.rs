//! Gateway election + health-probe failover for per-DCC sidecar processes.
//!
//! In-process DCC adapters use :class:`dcc_mcp_core.gateway_election.DccGatewayElection`
//! (Python) to promote when ``GET /health`` on the well-known gateway port fails.
//! Sidecar mode moves MCP dispatch out-of-process; the sidecar must run the
//! same probe loop in Rust so a surviving peer can take over when the elected
//! gateway crashes without restarting Maya.

use dcc_mcp_gateway::{ElectionOutcome, GatewayConfig, GatewayHandle, GatewayRunner};
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceKey};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::AbortHandle;

use crate::sidecar::SidecarArgs;

const DEFAULT_PROBE_INTERVAL_SECS: u64 = 1;
const DEFAULT_PROBE_FAILURES: u32 = 2;
const DEFAULT_PROBE_TIMEOUT_SECS: u64 = 2;

/// Owns the gateway runner, heartbeat, and optional failover probe task.
pub struct SidecarGatewayControl {
    gateway_handle: GatewayHandle,
    probe_abort: Option<AbortHandle>,
    failover: Arc<RwLock<FailoverHandles>>,
}

struct FailoverHandles {
    is_gateway: bool,
    gateway_abort: Option<AbortHandle>,
    challenger_abort: Option<AbortHandle>,
    gateway_supervisor: Option<tokio::task::JoinHandle<()>>,
    gateway_thread: Option<std::thread::JoinHandle<()>>,
    sentinel_key: Option<ServiceKey>,
}

impl SidecarGatewayControl {
    /// Stop gateway HTTP, challenger loop, and probe task (best-effort).
    pub async fn shutdown(self) {
        if let Some(abort) = self.probe_abort {
            abort.abort();
        }
        let mut failover = self.failover.write().await;
        if let Some(sentinel_key) = failover.sentinel_key.take() {
            let reg = self.gateway_handle.registry();
            if let Ok(registry) = reg.try_read() {
                let _ = registry.deregister(&sentinel_key);
            }
        }
        abort_failover_handles(&mut failover);
        drop(failover);
        drop(self.gateway_handle);
    }
}

/// Register heartbeat, run initial election, and spawn the probe loop when needed.
pub async fn start_sidecar_gateway(
    args: &SidecarArgs,
    registry: Arc<FileRegistry>,
    entry: ServiceEntry,
) -> anyhow::Result<Option<SidecarGatewayControl>> {
    if args.gateway_port == 0 {
        return Ok(None);
    }

    let gateway_cfg = build_gateway_config(args);
    let runner =
        GatewayRunner::new(gateway_cfg).map_err(|e| anyhow::anyhow!("GatewayRunner::new: {e}"))?;
    let gateway_handle = runner
        .start(entry, None)
        .await
        .map_err(|e| anyhow::anyhow!("GatewayRunner::start: {e}"))?;

    let failover = Arc::new(RwLock::new(FailoverHandles {
        is_gateway: gateway_handle.is_gateway,
        gateway_abort: None,
        challenger_abort: None,
        gateway_supervisor: None,
        gateway_thread: None,
        sentinel_key: None,
    }));

    let probe_abort = if gateway_handle.is_gateway {
        tracing::info!(
            port = args.gateway_port,
            "sidecar won gateway election at startup"
        );
        None
    } else {
        tracing::info!(
            port = args.gateway_port,
            "sidecar registered as plain instance — starting gateway health probe"
        );
        Some(spawn_failover_probe(
            Arc::new(runner),
            registry,
            failover.clone(),
            args.host.clone(),
            args.gateway_port,
        ))
    };

    Ok(Some(SidecarGatewayControl {
        gateway_handle,
        probe_abort,
        failover,
    }))
}

fn build_gateway_config(args: &SidecarArgs) -> GatewayConfig {
    let gateway_host = args
        .gateway_host
        .clone()
        .unwrap_or_else(|| args.host.clone());

    GatewayConfig {
        host: gateway_host,
        gateway_port: args.gateway_port,
        remote_host: Some(args.gateway_remote_host.clone()),
        remote_gateway_port: args.gateway_remote_port,
        registry_dir: args.registry_dir.clone(),
        server_name: format!("dcc-mcp-gateway-{}", args.dcc),
        gateway_name: Some(resolve_sidecar_gateway_name(args)),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        adapter_version: args.adapter_version.clone(),
        adapter_dcc: Some(args.dcc.clone()),
        ..GatewayConfig::default()
    }
}

fn resolve_sidecar_gateway_name(args: &SidecarArgs) -> String {
    args.gateway_name
        .as_ref()
        .filter(|name| !name.trim().is_empty())
        .cloned()
        .or_else(|| args.display_name.clone())
        .unwrap_or_else(|| format!("{}-pid{}", args.dcc, args.watch_pid))
}

fn spawn_failover_probe(
    runner: Arc<GatewayRunner>,
    registry: Arc<FileRegistry>,
    failover: Arc<RwLock<FailoverHandles>>,
    host: String,
    port: u16,
) -> AbortHandle {
    let probe_interval = std::env::var("DCC_MCP_GATEWAY_PROBE_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PROBE_INTERVAL_SECS)
        .max(1);
    let probe_failures = std::env::var("DCC_MCP_GATEWAY_PROBE_FAILURES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PROBE_FAILURES)
        .max(1);
    let probe_timeout = std::env::var("DCC_MCP_GATEWAY_PROBE_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PROBE_TIMEOUT_SECS)
        .max(1);

    let handle = tokio::spawn(async move {
        let mut consecutive_failures = 0u32;
        loop {
            tokio::time::sleep(Duration::from_secs(probe_interval)).await;

            let is_gateway = failover.read().await.is_gateway;
            if is_gateway {
                consecutive_failures = 0;
                continue;
            }

            if probe_gateway_health(&host, port, probe_timeout).await {
                consecutive_failures = 0;
                continue;
            }

            consecutive_failures += 1;
            if consecutive_failures < probe_failures {
                tracing::debug!(
                    failures = consecutive_failures,
                    threshold = probe_failures,
                    "sidecar gateway probe failed"
                );
                continue;
            }

            tracing::warn!(
                port,
                failures = consecutive_failures,
                "gateway unreachable from sidecar — re-running election"
            );
            consecutive_failures = 0;

            match runner.run_election().await {
                Ok(outcome) => {
                    let mut state = failover.write().await;
                    apply_election_outcome(&mut state, outcome);
                    if state.is_gateway {
                        tracing::info!(port, "sidecar promoted to gateway after probe failover");
                    }
                }
                Err(err) => {
                    tracing::error!(error = %err, "sidecar gateway re-election failed");
                }
            }

            // Avoid hammering the registry while a challenger loop is already polling.
            let _ = registry.prune_dead_entries();
        }
    });
    handle.abort_handle()
}

async fn probe_gateway_health(host: &str, port: u16, timeout_secs: u64) -> bool {
    let url = format!("http://{host}:{port}/health");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

fn apply_election_outcome(state: &mut FailoverHandles, outcome: ElectionOutcome) {
    abort_failover_handles(state);
    state.is_gateway = outcome.is_gateway;
    state.gateway_abort = outcome.gateway_abort;
    state.challenger_abort = outcome.challenger_abort;
    state.gateway_supervisor = outcome.gateway_supervisor;
    state.gateway_thread = outcome.gateway_thread;
    state.sentinel_key = outcome.sentinel_key;
}

fn abort_failover_handles(state: &mut FailoverHandles) {
    if let Some(abort) = state.gateway_abort.take() {
        abort.abort();
    }
    if let Some(abort) = state.challenger_abort.take() {
        abort.abort();
    }
    if let Some(handle) = state.gateway_supervisor.take() {
        handle.abort();
    }
    if let Some(handle) = state.gateway_thread.take() {
        drop(handle);
    }
    state.sentinel_key = None;
}
