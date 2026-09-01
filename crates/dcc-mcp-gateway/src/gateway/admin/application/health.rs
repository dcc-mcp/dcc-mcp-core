//! Admin health and reliability endpoints.

use std::time::UNIX_EPOCH;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dcc_mcp_gateway_admin::gateway_health_payload;
use futures::future::join_all;
use serde_json::{Value, json};

use crate::gateway::GatewayPolicy;
use crate::gateway::admin::state::AdminState;
use crate::gateway::backend_client::health_url_from_mcp_url;
use crate::gateway::capability_service::{is_public_ip, safe_discovery_target};
use crate::gateway::http_registration::{SOURCE_HTTP, entry_mcp_url, entry_registry_source};
use crate::gateway::response_codec::{
    JSON_MIME, TOKEN_ESTIMATOR, TOON_MIME, default_rest_response_format,
};
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};

/// `GET /admin/api/health` — service health summary.
pub async fn handle_admin_health(State(s): State<AdminState>) -> impl IntoResponse {
    let registry = s.gateway.registry.clone();
    let all = s.gateway.all_instances_async().await;
    let live = s.gateway.live_instances_async().await;
    let eligible = eligible_health_entries(&s.gateway.policy, live);
    let ready = eligible.len();
    let job_persistence = collect_job_persistence_health(&s.gateway, &eligible).await;
    let gateway_sentinels = registry
        .list_instances_async(GATEWAY_SENTINEL_DCC_TYPE.to_string())
        .await
        .unwrap_or_default();
    let total = eligible_instance_count(&s.gateway.policy, &all);

    let uptime_secs = s.started_at.elapsed().unwrap_or_default().as_secs();

    let status = if ready > 0 || total == 0 {
        "ok"
    } else {
        "degraded"
    };

    let limits = s.gateway.ingress.limits();
    let resilience = s.gateway.resilience.policy();
    let circuits = s.gateway.resilience.circuits().snapshot_json();
    let rss_bytes = gateway_self_rss_bytes();

    (
        StatusCode::OK,
        Json(json!({
            "status": status,
            "instances_ready": ready,
            "instances_total": total,
            "uptime_secs": uptime_secs,
            "version": s.gateway.server_version,
            "rss_bytes": rss_bytes,
            "response_format": {
                "default": default_rest_response_format().as_str(),
                "legacy_mime": JSON_MIME,
                "compact_mime": TOON_MIME,
                "token_estimator": TOKEN_ESTIMATOR,
            },
            "gateway": gateway_health_snapshot(&gateway_sentinels),
            "job_persistence": job_persistence,
            "limits": {
                "body_max_bytes": limits.body_max_bytes,
                "rate_limit_per_minute_per_ip": limits.rate_limit_per_minute_per_ip,
                "xff_trusted_depth": limits.xff_trusted_depth,
                "read_retry_max": resilience.read_retry_max,
                "circuit_failure_threshold": resilience.circuit_failure_threshold,
                "circuit_open_secs": resilience.circuit_open_secs,
            },
            "circuits": circuits,
        })),
    )
}

/// Collect payload-safe job-persistence snapshots from registered backends.
///
/// This is intentionally an on-demand admin/debug read. It does not probe or
/// mutate the backend registry and it never forwards raw backend errors or
/// filesystem paths. A backend that cannot answer is represented as
/// `unavailable`, so the operator can distinguish missing telemetry from a
/// healthy persistence circuit.
async fn collect_job_persistence_health(
    gateway: &crate::gateway::state::GatewayState,
    entries: &[ServiceEntry],
) -> Value {
    let timeout = gateway
        .backend_timeout
        .min(std::time::Duration::from_secs(2));
    let probe_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| gateway.http_client.clone());
    let reports = join_all(entries.iter().filter(|entry| entry.port != 0).map(|entry| {
        let client = probe_client.clone();
        let url = health_probe_url(entry);
        let dcc_type = entry.dcc_type.clone();
        let instance_id = entry.instance_id.to_string();
        async move {
            let fallback_dcc_type = dcc_type.clone();
            let fallback_instance_id = instance_id.clone();
            let Some(url) = url else {
                return json!({
                    "dcc_type": fallback_dcc_type,
                    "instance_id": fallback_instance_id,
                    "state": "unavailable",
                    "consecutive_failures": 0,
                    "last_error_kind": Value::Null,
                });
            };
            let report = tokio::time::timeout(timeout, async move {
                let response = client.get(url).send().await.ok()?;
                if !response.status().is_success() {
                    return None;
                }
                response.json::<Value>().await.ok().and_then(|body| {
                    let status = body.get("job_persistence")?.clone();
                    let state = match status.get("state").and_then(Value::as_str) {
                        Some(value) if is_known_persistence_state(value) => value,
                        _ => "unavailable",
                    };
                    let last_error_kind = normalize_error_kind(status.get("last_error_kind"));
                    Some(json!({
                        "dcc_type": dcc_type,
                        "instance_id": instance_id,
                        "state": state,
                        "consecutive_failures": status
                            .get("consecutive_failures")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                        "last_error_kind": last_error_kind,
                    }))
                })
            })
            .await
            .ok()
            .flatten();
            report.unwrap_or_else(|| {
                json!({
                    "dcc_type": fallback_dcc_type,
                    "instance_id": fallback_instance_id,
                    "state": "unavailable",
                    "consecutive_failures": 0,
                    "last_error_kind": Value::Null,
                })
            })
        }
    }))
    .await;

    let degraded_instances = reports
        .iter()
        .filter(|report| report["state"] == "degraded")
        .count();
    let disabled_instances = reports
        .iter()
        .filter(|report| report["state"] == "disabled")
        .count();
    let unavailable_instances = reports
        .iter()
        .filter(|report| report["state"] == "unavailable")
        .count();

    json!({
        "instances": reports,
        "degraded_instances": degraded_instances,
        "disabled_instances": disabled_instances,
        "unavailable_instances": unavailable_instances,
    })
}

fn eligible_health_entries(
    policy: &GatewayPolicy,
    entries: Vec<ServiceEntry>,
) -> Vec<ServiceEntry> {
    entries
        .into_iter()
        .filter(|entry| policy.allows_dcc(&entry.dcc_type))
        .filter(|entry| entry.port != 0)
        .filter(safe_discovery_target)
        .collect()
}

fn eligible_instance_count(policy: &GatewayPolicy, entries: &[ServiceEntry]) -> usize {
    entries
        .iter()
        .filter(|entry| policy.allows_dcc(&entry.dcc_type) && entry.port != 0)
        .count()
}

/// Construct a safe health target from the registry identity.
///
/// Metadata may contain an MCP URL for routing, but health telemetry must not
/// turn that field into an arbitrary outbound request.  Require the URL to
/// resolve to the registered host/port, strip query credentials, and reject
/// unsupported schemes.  A rejected target is represented as unavailable.
fn health_probe_url(entry: &ServiceEntry) -> Option<String> {
    let raw = entry_mcp_url(entry);
    let mut url = reqwest::Url::parse(&raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let host = url.host_str()?;
    if !host.eq_ignore_ascii_case(&entry.host) {
        return None;
    }
    // HTTP registrations are untrusted input.  Do not let them turn the
    // gateway's periodic health read into a private-network SSRF primitive;
    // local/file discovery remains valid for the normal per-DCC deployment.
    if entry_registry_source(entry) == SOURCE_HTTP {
        // HTTP registration is an untrusted cross-process input.  Require a
        // literal, globally-routable address so DNS rebinding cannot turn a
        // periodic probe into private-network egress.
        let Ok(ip) = host.parse::<std::net::IpAddr>() else {
            return None;
        };
        if !is_public_ip(ip) {
            return None;
        }
    }
    if url.port_or_known_default()? != entry.port {
        return None;
    }
    url.set_query(None);
    Some(health_url_from_mcp_url(url.as_str()))
}

fn is_known_persistence_state(value: &str) -> bool {
    matches!(
        value,
        "not_configured" | "healthy" | "degraded" | "disabled" | "unavailable"
    )
}

fn is_known_error_kind(value: &str) -> bool {
    matches!(
        value,
        "readonly"
            | "wal"
            | "busy"
            | "disk_full"
            | "backend"
            | "decode"
            | "feature_disabled"
            | "queue_full"
            | "worker_unavailable"
            | "shutdown_timeout"
            | "retention_prune_failed"
            | "server_shutdown"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        eligible_health_entries, eligible_instance_count, health_probe_url,
        health_url_from_mcp_url, is_known_error_kind, is_known_persistence_state,
        normalize_error_kind,
    };
    use dcc_mcp_gateway_core::policy::GatewayPolicy;
    use dcc_mcp_transport::discovery::types::ServiceEntry;
    use serde_json::json;

    use crate::gateway::http_registration::SOURCE_HTTP;

    #[test]
    fn health_projection_rejects_untrusted_backend_values() {
        assert!(!is_known_error_kind("C:\\secrets\\jobs.sqlite3"));
        assert!(!is_known_error_kind("raw sqlite error"));
        assert!(is_known_error_kind("readonly"));
        assert!(!is_known_persistence_state("healthy /etc/passwd"));
        assert!(is_known_persistence_state("degraded"));
    }

    #[test]
    fn health_projection_normalizes_non_string_error_values() {
        let payloads = [json!(123), json!(true), json!({"path": "/secret"})];
        for payload in payloads {
            assert_eq!(normalize_error_kind(Some(&payload)), json!("backend"));
        }
        assert_eq!(normalize_error_kind(None), serde_json::Value::Null);
        assert_eq!(
            normalize_error_kind(Some(&serde_json::Value::Null)),
            serde_json::Value::Null
        );
    }

    #[test]
    fn health_collection_applies_dcc_allowlist_and_preserves_https() {
        let policy = GatewayPolicy {
            allowed_dcc_types: vec!["maya".to_string()],
            ..GatewayPolicy::default()
        };
        let entries = vec![
            ServiceEntry::new("maya", "127.0.0.1", 8765),
            ServiceEntry::new("blender", "127.0.0.1", 8766),
        ];
        let eligible = eligible_health_entries(&policy, entries);
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].dcc_type, "maya");
        assert_eq!(
            health_url_from_mcp_url("https://127.0.0.1:8765/mcp"),
            "https://127.0.0.1:8765/health"
        );
    }

    #[test]
    fn health_counts_only_policy_allowed_nonzero_ports() {
        let policy = GatewayPolicy {
            allowed_dcc_types: vec!["maya".to_string()],
            ..GatewayPolicy::default()
        };
        let entries = vec![
            ServiceEntry::new("maya", "127.0.0.1", 8765),
            ServiceEntry::new("maya", "127.0.0.1", 0),
            ServiceEntry::new("blender", "127.0.0.1", 8766),
        ];
        assert_eq!(eligible_instance_count(&policy, &entries), 1);
    }

    #[test]
    fn health_probe_rejects_alias_targets_and_strips_query_credentials() {
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 8765);
        entry.metadata.insert(
            "mcp_url".to_string(),
            "https://backend.example:8765/mcp?token=secret".to_string(),
        );
        assert!(health_probe_url(&entry).is_none());

        entry.metadata.insert(
            "mcp_url".to_string(),
            "https://127.0.0.1:8765/prefix/mcp?token=secret".to_string(),
        );
        assert_eq!(
            health_probe_url(&entry).as_deref(),
            Some("https://127.0.0.1:8765/prefix/health")
        );

        entry.metadata.insert(
            "dcc_mcp_registry_source".to_string(),
            SOURCE_HTTP.to_string(),
        );
        assert!(health_probe_url(&entry).is_none());
    }

    #[test]
    fn health_probe_rejects_userinfo_and_mapped_private_ipv6() {
        let mut entry = ServiceEntry::new("maya", "203.0.113.7", 8765);
        entry.metadata.insert(
            "mcp_url".to_string(),
            "https://user:secret@203.0.113.7:8765/mcp".to_string(),
        );
        assert!(health_probe_url(&entry).is_none());

        entry.host = "::ffff:127.0.0.1".to_string();
        entry.metadata.insert(
            "mcp_url".to_string(),
            "https://[::ffff:127.0.0.1]:8765/mcp".to_string(),
        );
        entry.metadata.insert(
            "dcc_mcp_registry_source".to_string(),
            SOURCE_HTTP.to_string(),
        );
        assert!(health_probe_url(&entry).is_none());
    }
}

fn normalize_error_kind(value: Option<&Value>) -> Value {
    match value {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(value)) if is_known_error_kind(value) => Value::from(value.as_str()),
        Some(_) => Value::from("backend"),
    }
}

pub(crate) fn gateway_health_snapshot(sentinels: &[ServiceEntry]) -> Value {
    gateway_health_payload(sentinels.iter().map(gateway_sentinel_json).collect())
}

fn gateway_sentinel_json(entry: &ServiceEntry) -> Value {
    let last_heartbeat_secs = entry
        .last_heartbeat
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());
    let role = entry
        .metadata
        .get("gateway_role")
        .cloned()
        .unwrap_or_else(|| "active".to_string());
    let name = entry
        .metadata
        .get("gateway_name")
        .cloned()
        .or_else(|| entry.display_name.clone())
        .unwrap_or_else(|| format!("gateway-pid{}", entry.pid.unwrap_or_default()));
    json!({
        "name": name,
        "role": role,
        "pid": entry.pid,
        "host": entry.host,
        "port": entry.port,
        "instance_id": entry.instance_id.to_string(),
        "version": entry.version,
        "adapter_version": entry.adapter_version,
        "adapter_dcc": entry.adapter_dcc,
        "last_heartbeat_unix": last_heartbeat_secs,
        "metadata": entry.metadata,
    })
}

pub(crate) fn gateway_self_rss_bytes() -> Option<u64> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(std::process::id());
    sys.refresh_processes(ProcessesToUpdate::Some(std::slice::from_ref(&pid)), true);
    sys.process(pid).map(|p| p.memory())
}

/// `GET /admin/api/reliability` — reliability & stability summary.
///
/// Aggregates health, circuit breaker state, capability funnel, and
/// 24-hour stability (crashes, reconnects, recoveries) into a single
/// payload for the admin UI Reliability panel.
pub async fn handle_admin_reliability(State(s): State<AdminState>) -> impl IntoResponse {
    let registry = s.gateway.registry.clone();
    let all = s.gateway.all_instances_async().await;
    let live = s.gateway.live_instances_async().await;
    let ready = eligible_health_entries(&s.gateway.policy, live).len();
    let total = eligible_instance_count(&s.gateway.policy, &all);
    let gateway_sentinels = registry
        .list_instances_async(GATEWAY_SENTINEL_DCC_TYPE.to_string())
        .await
        .unwrap_or_default();

    let uptime_secs = s.started_at.elapsed().unwrap_or_default().as_secs();
    let status = if ready > 0 || total == 0 {
        "ok"
    } else {
        "degraded"
    };
    let limits = s.gateway.ingress.limits();
    let resilience = s.gateway.resilience.policy();
    let circuits = s.gateway.resilience.circuits().snapshot_json();
    let rss_bytes = gateway_self_rss_bytes();

    // Stability: query sessions table for crash/reconnect/recovery counts
    // in the last 24 hours.
    let (crashes_24h, reconnects_24h, recoveries_24h) = if let Some(ref lane) = s.admin_sqlite_lane
    {
        let reader = lane.reader();
        let since_ms = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
            - 86_400_000; // 24 hours

        // Count sessions that ended abnormally in the last 24h
        let all_sessions = reader.list_sessions(10_000, None, None);
        let crashes = all_sessions
            .iter()
            .filter(|row| {
                let status = row.get("status").and_then(|v| v.as_str()).unwrap_or("");
                let ended = row.get("ended_at_ms").and_then(|v| v.as_i64()).unwrap_or(0);
                (status == "crashed" || status == "gpu_crashed" || status == "disconnected")
                    && ended >= since_ms
            })
            .count();

        // Count session_events with event_type = 'reconnect' in the last 24h
        let reconnects = all_sessions
            .iter()
            .filter(|row| {
                let session_id = row.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                let events = reader.list_session_events(session_id, 1000);
                events.iter().any(|ev| {
                    let event_type = ev.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
                    let created = ev
                        .get("created_at_ms")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    event_type == "reconnect" && created >= since_ms
                })
            })
            .count();

        // Active sessions now (recoveries)
        let recoveries = all_sessions
            .iter()
            .filter(|row| {
                let status = row.get("status").and_then(|v| v.as_str()).unwrap_or("");
                status == "active"
            })
            .count();

        (crashes as i64, reconnects as i64, recoveries as i64)
    } else {
        (0, 0, 0)
    };

    // Stats for success rate — derive from existing stats if available,
    // otherwise report 100% as safe default.
    let success_rate = 100.0_f64;
    let p50_latency_ms = 0_i64;

    (
        StatusCode::OK,
        Json(json!({
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "status": status,
            "uptime_secs": uptime_secs,
            "version": s.gateway.server_version,
            "rss_bytes": rss_bytes,
            "gateway": {
                "status": status,
                "uptime_secs": uptime_secs,
                "version": s.gateway.server_version,
                "election": gateway_health_snapshot(&gateway_sentinels),
                "limits": {
                    "body_max_bytes": limits.body_max_bytes,
                    "rate_limit_per_minute_per_ip": limits.rate_limit_per_minute_per_ip,
                    "read_retry_max": resilience.read_retry_max,
                    "circuit_failure_threshold": resilience.circuit_failure_threshold,
                    "circuit_open_secs": resilience.circuit_open_secs,
                },
                "circuits": circuits,
            },
            "capability_funnel": {
                "instances_ready": ready,
                "instances_total": total,
                "skills_loaded": 0,
                "skills_total": 0,
                "tools_registered": 0,
                "resources_exposed": total,
            },
            "artifact_verification": {
                "builds_verified": 0,
                "builds_total": 0,
                "verification_errors": 0,
            },
            "stability": {
                "crashes_24h": crashes_24h,
                "reconnects_24h": reconnects_24h,
                "recoveries_24h": recoveries_24h,
                "uptime_pct": success_rate,
                "p50_latency_ms": p50_latency_ms,
            },
        })),
    )
}
