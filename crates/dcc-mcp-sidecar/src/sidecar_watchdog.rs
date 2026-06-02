//! Watchdog tasks for per-DCC sidecars.

use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceKey;
use tokio::sync::watch;

use crate::is_process_alive;
use crate::sidecar::ExitReason;

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
