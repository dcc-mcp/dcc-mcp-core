use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceStatus};

use crate::gateway::instance_diagnostics::InstanceDiagnosticsStore;

/// Configuration for the health-check task.
pub(crate) struct HealthCheckConfig {
    pub own_host: String,
    pub own_port: u16,
    pub health_check_interval_secs: u64,
    pub health_check_failures: u32,
    #[cfg(feature = "prometheus")]
    pub metrics: Arc<crate::gateway::event_log::GatewayMetrics>,
}

/// Spawn the periodic backend health-check task (issues #556 / #854).
pub(crate) fn spawn_health_check_task(
    registry: Arc<RwLock<FileRegistry>>,
    http_client: reqwest::Client,
    event_log: Arc<crate::gateway::event_log::EventLog>,
    instance_diagnostics: Arc<InstanceDiagnosticsStore>,
    cfg: HealthCheckConfig,
) -> tokio::task::JoinHandle<()> {
    let effective_interval_secs = std::env::var("DCC_MCP_GATEWAY_HEALTH_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(cfg.health_check_interval_secs);
    let effective_failures = std::env::var("DCC_MCP_GATEWAY_HEALTH_FAILURES")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(cfg.health_check_failures);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(effective_interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut failure_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        loop {
            interval.tick().await;
            let entries = {
                let r = registry.read().await;
                r.list_all()
                    .into_iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                            && !crate::gateway::is_own_instance(e, &cfg.own_host, cfg.own_port)
                    })
                    .collect::<Vec<_>>()
            };

            let probe_results: Vec<_> = {
                let client = &http_client;
                futures::future::join_all(entries.iter().map(|e| {
                    let url = format!("http://{}:{}/mcp", e.host, e.port);
                    async move {
                        crate::gateway::backend_client::probe_mcp_readiness_once(
                            client,
                            &url,
                            Duration::from_secs(5),
                        )
                        .await
                    }
                }))
                .await
            };

            for (entry, (readiness_report, outcome)) in entries.iter().zip(probe_results) {
                if let Some(report) = readiness_report {
                    instance_diagnostics.record_readiness(entry.instance_id, report);
                }
                let key = format!("{}:{}", entry.dcc_type, entry.instance_id);
                let id8 = entry.instance_id.to_string()[..8].to_string();

                if outcome.is_ready() {
                    let recovered_from_failure = failure_counts.remove(&key).is_some();
                    let was_not_available = !matches!(entry.status, ServiceStatus::Available);
                    if recovered_from_failure || was_not_available {
                        let r = registry.read().await;
                        let _ = r.update_status(&entry.key(), ServiceStatus::Available);
                        tracing::info!(
                            dcc_type = %entry.dcc_type,
                            instance_id = %entry.instance_id,
                            previous_status = %entry.status,
                            "Readiness probe green — marking Available"
                        );
                    }
                    #[cfg(feature = "prometheus")]
                    cfg.metrics.inc_probe("ready");
                    continue;
                }

                if outcome.is_alive() {
                    if !matches!(entry.status, ServiceStatus::Booting) {
                        let r = registry.read().await;
                        let _ = r.update_status(&entry.key(), ServiceStatus::Booting);
                        tracing::info!(
                            dcc_type = %entry.dcc_type,
                            instance_id = %entry.instance_id,
                            previous_status = %entry.status,
                            "Backend booting (GET /v1/readyz red) — marking Booting without deregister"
                        );
                        crate::gateway::event_log::record_event(
                            &event_log,
                            #[cfg(feature = "prometheus")]
                            &cfg.metrics,
                            crate::gateway::event_log::EventKind::ProbeBooting,
                            &entry.dcc_type,
                            &id8,
                            None,
                        );
                    }
                    failure_counts.remove(&key);
                    continue;
                }

                let count = {
                    let c = failure_counts.entry(key.clone()).or_insert(0);
                    *c += 1;
                    *c
                };
                tracing::warn!(
                    dcc_type = %entry.dcc_type,
                    instance_id = %entry.instance_id,
                    consecutive_failures = count,
                    "Health check failed"
                );

                if count >= effective_failures {
                    let r = registry.read().await;
                    let _ = r.update_status(&entry.key(), ServiceStatus::Unreachable);
                    crate::gateway::event_log::record_event(
                        &event_log,
                        #[cfg(feature = "prometheus")]
                        &cfg.metrics,
                        crate::gateway::event_log::EventKind::ProbeUnreachable,
                        &entry.dcc_type,
                        &id8,
                        Some(format!("{} consecutive failures", count)),
                    );
                }

                if count > effective_failures {
                    let r = registry.read().await;
                    let _ = r.deregister(&entry.key());
                    failure_counts.remove(&key);
                    tracing::info!(
                        dcc_type = %entry.dcc_type,
                        instance_id = %entry.instance_id,
                        consecutive_failures = count,
                        "Auto-deregistered after consecutive health-check failures"
                    );
                    crate::gateway::event_log::record_event(
                        &event_log,
                        #[cfg(feature = "prometheus")]
                        &cfg.metrics,
                        crate::gateway::event_log::EventKind::AutoDeregister,
                        &entry.dcc_type,
                        &id8,
                        Some(format!("{} consecutive health-check failures", count)),
                    );
                }
            }
        }
    })
}
