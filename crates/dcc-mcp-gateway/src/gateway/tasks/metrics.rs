#![cfg(feature = "prometheus")]

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceStatus};

/// Spawn the Prometheus instance-count updater (issue #559).
pub(crate) fn spawn_metrics_updater(
    registry: Arc<RwLock<FileRegistry>>,
    stale_timeout: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let exporter = dcc_mcp_telemetry::PrometheusExporter::new();
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let r = registry.read().await;
            let all = r.list_all();
            let active = all
                .iter()
                .filter(|e| {
                    e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                        && !e.is_stale(stale_timeout)
                        && !matches!(
                            e.status,
                            ServiceStatus::ShuttingDown | ServiceStatus::Unreachable
                        )
                })
                .count() as i64;
            let stale = all
                .iter()
                .filter(|e| e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE && e.is_stale(stale_timeout))
                .count() as i64;
            exporter.set_instances_total("active", active);
            exporter.set_instances_total("stale", stale);
        }
    })
}
