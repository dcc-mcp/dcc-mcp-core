use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceStatus};

use crate::gateway::instance_diagnostics::InstanceDiagnosticsStore;

/// Configuration for the health-check task.
pub(crate) struct HealthCheckConfig {
    pub own_host: String,
    pub own_port: u16,
    pub health_check_interval_secs: u64,
    pub health_check_failures: u32,
    #[cfg(feature = "admin")]
    #[allow(dead_code)]
    pub admin_sqlite_lane: Option<crate::gateway::admin::sqlite_lane::AdminSqliteLane>,
    #[cfg(feature = "prometheus")]
    pub metrics: Arc<crate::gateway::event_log::GatewayMetrics>,
}

fn port_zero_boot_reason(entry: &ServiceEntry) -> String {
    entry
        .metadata
        .get("failure_reason")
        .cloned()
        .unwrap_or_else(|| "sidecar listener has not published an MCP port yet".to_string())
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
        let mut port_zero_seen: std::collections::HashSet<String> =
            std::collections::HashSet::new();
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

            let probe_results: std::collections::HashMap<String, _> = {
                let client = &http_client;
                futures::future::join_all(entries.iter().filter(|e| e.port != 0).map(|e| {
                    let key = format!("{}:{}", e.dcc_type, e.instance_id);
                    let url = format!("http://{}:{}/mcp", e.host, e.port);
                    async move {
                        (
                            key,
                            crate::gateway::backend_client::probe_mcp_readiness_once(
                                client,
                                &url,
                                Duration::from_secs(5),
                            )
                            .await,
                        )
                    }
                }))
                .await
                .into_iter()
                .collect()
            };

            for entry in &entries {
                let key = format!("{}:{}", entry.dcc_type, entry.instance_id);
                let id8 = entry.instance_id.to_string()[..8].to_string();
                if entry.port == 0 {
                    let first_seen = port_zero_seen.insert(key.clone());
                    failure_counts.remove(&key);
                    if !matches!(entry.status, ServiceStatus::Booting) {
                        let r = registry.read().await;
                        let _ = r.update_status(&entry.key(), ServiceStatus::Booting);
                    }
                    if first_seen || !matches!(entry.status, ServiceStatus::Booting) {
                        let reason = port_zero_boot_reason(entry);
                        tracing::warn!(
                            dcc_type = %entry.dcc_type,
                            instance_id = %entry.instance_id,
                            reason = %reason,
                            "Health check skipped port=0 backend; keeping row Booting"
                        );
                        crate::gateway::event_log::record_event(
                            &event_log,
                            #[cfg(feature = "prometheus")]
                            &cfg.metrics,
                            crate::gateway::event_log::EventKind::ProbeBooting,
                            &entry.dcc_type,
                            &id8,
                            Some(reason),
                        );
                    }
                    continue;
                }
                port_zero_seen.remove(&key);
                let Some((readiness_report, outcome)) = probe_results.get(&key) else {
                    continue;
                };
                if let Some(report) = readiness_report {
                    instance_diagnostics.record_readiness(entry.instance_id, report.clone());
                }

                if outcome.is_ready() {
                    let recovered_from_failure = failure_counts.remove(&key).is_some();
                    let was_not_available = !matches!(entry.status, ServiceStatus::Available);
                    if recovered_from_failure || was_not_available {
                        let r = registry.read().await;
                        let _ = r.update_status_if_unchanged(entry, ServiceStatus::Available);
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
                        let _ = r.update_status_if_unchanged(entry, ServiceStatus::Booting);
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
                    *c = c.saturating_add(1);
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
                    let _ = r.update_status_if_unchanged(entry, ServiceStatus::Unreachable);
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

                // A transport/readiness timeout is not proof that the DCC
                // process died: a monolithic host-main call can temporarily
                // starve HTTP while the independent registry heartbeat and
                // owner sentinel remain valid. Keep the row Unreachable so
                // agents can rediscover/retry it. Owner-lock/PID pruning and
                // remote-registration TTL are the authoritative death tests.
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_transport::discovery::types::ServiceEntry;
    use tempfile::tempdir;

    #[tokio::test]
    async fn port_zero_rows_stay_booting_and_are_not_deregistered() {
        let dir = tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
        let mut entry = ServiceEntry::new("3dsmax", "127.0.0.1", 0);
        entry
            .metadata
            .insert("failure_reason".into(), "host-rpc connect failed".into());
        let key = entry.key();
        {
            let reg = registry.write().await;
            reg.register(entry).unwrap();
        }

        let event_log = Arc::new(crate::gateway::event_log::EventLog::new());
        let handle = spawn_health_check_task(
            registry.clone(),
            reqwest::Client::new(),
            event_log.clone(),
            Arc::new(InstanceDiagnosticsStore::new()),
            HealthCheckConfig {
                own_host: "127.0.0.1".into(),
                own_port: 9765,
                health_check_interval_secs: 60,
                health_check_failures: 1,
                #[cfg(feature = "admin")]
                admin_sqlite_lane: None,
                #[cfg(feature = "prometheus")]
                metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
            },
        );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let row = {
                let reg = registry.read().await;
                reg.get(&key)
            };
            if let Some(row) = row
                && row.status == ServiceStatus::Booting
                && event_log
                    .recent_events(10)
                    .iter()
                    .any(|event| event.event == crate::gateway::event_log::EventKind::ProbeBooting)
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "port=0 row was not marked Booting in time"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        handle.abort();

        let reg = registry.read().await;
        let row = reg.get(&key).expect("port=0 row must remain registered");
        assert_eq!(row.status, ServiceStatus::Booting);
        assert_eq!(row.port, 0);
        assert!(
            !event_log
                .recent_events(10)
                .iter()
                .any(|event| event.event == crate::gateway::event_log::EventKind::AutoDeregister)
        );
    }

    #[tokio::test]
    async fn transport_failures_mark_unreachable_without_deregistering_live_owner() {
        let dir = tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
        let entry = ServiceEntry::new("houdini", "127.0.0.1", 9);
        let key = entry.key();
        {
            let reg = registry.write().await;
            reg.register(entry).unwrap();
        }

        let handle = spawn_health_check_task(
            registry.clone(),
            reqwest::Client::new(),
            Arc::new(crate::gateway::event_log::EventLog::new()),
            Arc::new(InstanceDiagnosticsStore::new()),
            HealthCheckConfig {
                own_host: "127.0.0.1".into(),
                own_port: 9765,
                health_check_interval_secs: 1,
                health_check_failures: 1,
                #[cfg(feature = "admin")]
                admin_sqlite_lane: None,
                #[cfg(feature = "prometheus")]
                metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
            },
        );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
        loop {
            let row = registry.read().await.get(&key);
            if row
                .as_ref()
                .is_some_and(|row| row.status == ServiceStatus::Unreachable)
            {
                break;
            }
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(1100)).await;
        handle.abort();

        let row = registry.read().await.get(&key);
        assert!(
            row.is_some(),
            "transport failures must not erase a live-owned row"
        );
        assert_eq!(row.unwrap().status, ServiceStatus::Unreachable);
    }
}
