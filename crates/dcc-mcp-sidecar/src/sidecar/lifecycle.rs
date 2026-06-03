use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceKey;
use tokio::sync::watch;

use crate::is_process_alive;

use super::registry::mark_sidecar_dispatch_ready;
#[cfg(feature = "gateway-daemon")]
use super::registry::publish_guardian_status;
use super::{ExitReason, SidecarArgs};

/// Connect the freshly-instantiated [`dcc_mcp_host_rpc::HostRpcClient`] to the DCC.
///
/// Wrapped as a separate helper so the caller can keep the `match`
/// arms in `run()` shallow and so the timeout / log surface is in
/// one place.
pub(crate) async fn client_connect(
    mut client: Box<dyn dcc_mcp_host_rpc::HostRpcClient>,
    endpoint: &str,
    timeout: Duration,
) -> Result<Box<dyn dcc_mcp_host_rpc::HostRpcClient>, dcc_mcp_host_rpc::HostRpcError> {
    client.connect(endpoint, timeout).await?;
    Ok(client)
}

pub(crate) fn spawn_ppid_watcher(
    parent_pid: u32,
    poll_interval: Duration,
    exit_tx: watch::Sender<Option<ExitReason>>,
) {
    tokio::spawn(async move {
        loop {
            if !is_process_alive(parent_pid) {
                tracing::info!(
                    parent_pid,
                    "parent DCC process no longer alive - signalling sidecar exit"
                );
                let _ = exit_tx.send(Some(ExitReason::ParentDied));
                return;
            }
            tokio::time::sleep(poll_interval).await;
        }
    });
}

pub(crate) fn spawn_sidecar_heartbeat(
    registry: Arc<FileRegistry>,
    key: ServiceKey,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            match registry.heartbeat(&key) {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(
                        dcc = %key.dcc_type,
                        instance_id = %key.instance_id,
                        "sidecar heartbeat skipped because registry row is missing"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        dcc = %key.dcc_type,
                        instance_id = %key.instance_id,
                        error = %err,
                        "sidecar heartbeat failed"
                    );
                }
            }
        }
    })
}

/// Periodically sync the guardian watchdog's live status into the sidecar's
/// FileRegistry metadata so admin UI and diagnostics surfaces can show the
/// fallback reason and current watchdog state.
#[cfg(feature = "gateway-daemon")]
pub(crate) fn spawn_guardian_status_publisher(
    handle: crate::gateway_daemon::GatewayGuardianHandle,
    registry: Arc<FileRegistry>,
    key: ServiceKey,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let status = handle.status();
            if let Err(err) = publish_guardian_status(&registry, &key, &status) {
                tracing::warn!(
                    dcc = %key.dcc_type,
                    instance_id = %key.instance_id,
                    error = %err,
                    "sidecar guardian status publisher failed to update registry"
                );
            }
        }
    })
}

pub(crate) fn should_retry_host_rpc_connect(args: &SidecarArgs) -> bool {
    match dcc_mcp_host_rpc::parse_scheme(&args.host_rpc) {
        Ok(scheme) => scheme != "stub" || args.allow_stub_dispatch_ready,
        Err(_) => false,
    }
}

pub(crate) fn spawn_host_rpc_reconnector(
    state: crate::sidecar_mcp::SidecarMcpState,
    registry: Arc<FileRegistry>,
    key: ServiceKey,
    host_rpc: String,
    connect_timeout: Duration,
    allow_stub_dispatch_ready: bool,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let client = match dcc_mcp_host_rpc::client_for_uri(&host_rpc) {
                Ok(client) => client,
                Err(err) => {
                    tracing::warn!(
                        host_rpc = %host_rpc,
                        error = %err,
                        "sidecar host-rpc reconnect stopped because the URI scheme is unsupported"
                    );
                    return;
                }
            };
            match client_connect(client, &host_rpc, connect_timeout).await {
                Ok(connected) => {
                    if connected.uri_scheme() == "stub" && !allow_stub_dispatch_ready {
                        tracing::debug!(
                            host_rpc = %host_rpc,
                            "sidecar host-rpc reconnect ignored test-only stub endpoint"
                        );
                        return;
                    }
                    state.replace_host_rpc(connected).await;
                    if let Err(err) = mark_sidecar_dispatch_ready(&registry, &key) {
                        tracing::warn!(
                            dcc = %key.dcc_type,
                            instance_id = %key.instance_id,
                            error = %err,
                            "FileRegistry failed to publish sidecar dispatch-ready after host-rpc reconnect"
                        );
                    }
                    tracing::info!(
                        dcc = %key.dcc_type,
                        instance_id = %key.instance_id,
                        host_rpc = %host_rpc,
                        "sidecar host-rpc reconnect succeeded"
                    );
                    return;
                }
                Err(err) => {
                    tracing::debug!(
                        host_rpc = %host_rpc,
                        error = %err,
                        "sidecar host-rpc reconnect attempt failed"
                    );
                }
            }
        }
    })
}
