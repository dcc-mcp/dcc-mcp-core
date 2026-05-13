//! Admin UI HTTP handlers.

use std::time::UNIX_EPOCH;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use dcc_mcp_gateway_core::naming::instance_short;
use serde_json::{Value, json};

use super::html::ADMIN_HTML;
use super::state::AdminState;
use super::trace::DispatchTrace;
use crate::gateway::capability::RefreshReason;
use crate::gateway::capability_service::refresh_all_live_backends;
use crate::gateway::event_log::ContendEvent;
use crate::gateway::resilience::{self as gw_resilience, gateway_limits};
use crate::gateway::state::entry_to_json;

/// `GET /admin` — serve the inline HTML dashboard.
pub async fn handle_admin_ui() -> impl IntoResponse {
    let mut resp = axum::response::Html(ADMIN_HTML).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store"),
    );
    resp
}

/// `GET /admin/api/instances` — list all instances known to the registry.
pub async fn handle_admin_instances(State(s): State<AdminState>) -> impl IntoResponse {
    let registry = s.gateway.registry.read().await;
    let instances: Vec<Value> = s
        .gateway
        .all_instances(&registry)
        .into_iter()
        .map(|e| {
            let mut v = entry_to_json(&e, s.gateway.stale_timeout);
            // Alias `instance_id` → `id` for the UI convenience.
            let id = v["instance_id"].clone();
            v.as_object_mut().map(|m| m.insert("id".into(), id));
            v
        })
        .collect();
    Json(json!({ "total": instances.len(), "instances": instances }))
}

/// `GET /admin/api/tools` — list all registered capability records.
pub async fn handle_admin_tools(State(s): State<AdminState>) -> impl IntoResponse {
    refresh_all_live_backends(&s.gateway, RefreshReason::Periodic).await;
    let records = s.gateway.capability_index.snapshot().records;
    let tools: Vec<Value> = records
        .iter()
        .map(|r| {
            let instance_prefix = instance_short(&r.instance_id);
            json!({
                "slug": r.tool_slug,
                "name": r.backend_tool,
                "dcc_type": r.dcc_type,
                "summary": r.summary,
                "skill_name": r.skill_name,
                "instance_id": r.instance_id.to_string(),
                "instance_prefix": instance_prefix,
            })
        })
        .collect();
    Json(json!({ "total": tools.len(), "tools": tools }))
}

/// `GET /admin/api/calls` — recent calls from the AuditLog ring buffer.
///
/// If no `AuditLog` is attached to the state, returns an empty array.
pub async fn handle_admin_calls(State(s): State<AdminState>) -> impl IntoResponse {
    let calls = match &s.audit_log {
        Some(log) => {
            let records = log.lock().clone();
            records
                .iter()
                .rev()
                .take(200)
                .map(|r| {
                    let ts = r
                        .timestamp
                        .duration_since(UNIX_EPOCH)
                        .ok()
                        .map(|_d| {
                            // ISO-8601 via chrono (already a dep).
                            chrono::DateTime::<chrono::Utc>::from(r.timestamp).to_rfc3339()
                        })
                        .unwrap_or_default();
                    json!({
                        "timestamp": ts,
                        "request_id": r.request_id,
                        "method": r.method,
                        "instance_id": r.instance_id,
                        "session_id": r.session_id,
                        "tool": r.action,
                        "dcc_type": r.dcc_type,
                        "status": if r.success { "ok" } else { "err" },
                        "success": r.success,
                        "error": r.error,
                        "duration_ms": r.duration_ms,
                    })
                })
                .collect::<Vec<_>>()
        }
        None => vec![],
    };
    Json(json!({ "total": calls.len(), "calls": calls }))
}

/// `GET /admin/api/logs` — gateway contention events (same ring as
/// `resources://gateway/events`).
///
/// Rows are normalised to `{timestamp, level, message}` for the embedded admin
/// UI. Data comes from [`GatewayState::event_log`] (same ring as
/// `resources://gateway/events`).
pub async fn handle_admin_logs(State(s): State<AdminState>) -> impl IntoResponse {
    let mut logs: Vec<Value> = s
        .gateway
        .event_log
        .recent_events(500)
        .into_iter()
        .map(contend_event_to_admin_row)
        .collect();

    if let Some(audit) = &s.audit_log {
        let records = audit.lock().clone();
        for r in records.iter().rev().take(200) {
            let ts = r
                .timestamp
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|_| {
                    chrono::DateTime::<chrono::Utc>::from(r.timestamp)
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                })
                .unwrap_or_default();
            let inst = r.instance_id.as_deref().unwrap_or("-");
            let tool = r.action.as_str();
            let msg = format!(
                "{} {} {}ms — {}",
                r.method.as_deref().unwrap_or("call"),
                if r.success { "ok" } else { "err" },
                r.duration_ms
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".into()),
                tool
            );
            logs.push(json!({
                "timestamp": ts,
                "level": if r.success { "info" } else { "warn" },
                "message": msg,
                "source": "audit",
                "dcc_type": r.dcc_type,
                "instance_id": r.instance_id,
                "request_id": r.request_id,
                "tool": tool,
                "success": r.success,
                "detail": format!("instance={inst}"),
            }));
        }
    }

    logs.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    logs.truncate(500);

    Json(json!({ "total": logs.len(), "logs": logs }))
}

/// `GET /admin/api/health` — service health summary.
pub async fn handle_admin_health(State(s): State<AdminState>) -> impl IntoResponse {
    let registry = s.gateway.registry.read().await;
    let all = s.gateway.all_instances(&registry);
    let ready = s.gateway.live_instances(&registry).len();
    let total = all.len();
    drop(registry);

    let uptime_secs = s.started_at.elapsed().unwrap_or_default().as_secs();

    let status = if ready > 0 || total == 0 {
        "ok"
    } else {
        "degraded"
    };

    let limits = gateway_limits();
    let circuits = gw_resilience::circuits().snapshot_json();
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
            "limits": {
                "body_max_bytes": limits.body_max_bytes,
                "rate_limit_per_minute_per_ip": limits.rate_limit_per_minute_per_ip,
                "xff_trusted_depth": limits.xff_trusted_depth,
                "read_retry_max": limits.read_retry_max,
                "circuit_failure_threshold": limits.circuit_failure_threshold,
                "circuit_open_secs": limits.circuit_open_secs,
            },
            "circuits": circuits,
        })),
    )
}

fn gateway_self_rss_bytes() -> Option<u64> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(std::process::id());
    sys.refresh_processes(ProcessesToUpdate::Some(std::slice::from_ref(&pid)), true);
    sys.process(pid).map(|p| p.memory())
}
/// `GET /admin/api/traces?limit=200` — recent per-call dispatch traces (Phase 2).
///
/// Each trace includes a waterfall of [`TraceSpan`]s plus optionally the
/// request / response payloads captured in `handle_tools_call`.
/// Returns `{"total": N, "traces": [...]}`.  When no `TraceLog` is attached
/// to the state, returns an empty array.
pub async fn handle_admin_traces(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(200)
        .min(500);
    let mapped: Vec<Value> = match &s.trace_log {
        Some(log) => log
            .recent(limit)
            .iter()
            .map(dispatch_trace_to_admin_row)
            .collect(),
        None => vec![],
    };
    Json(json!({ "total": mapped.len(), "traces": mapped }))
}

/// `GET /admin/api/traces/{request_id}` — full waterfall for one call.
///
/// Returns 404 when the trace is not in the ring buffer.
pub async fn handle_admin_trace_detail(
    State(s): State<AdminState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match s.trace_log.as_ref().and_then(|log| log.get(&request_id)) {
        Some(trace) => (
            StatusCode::OK,
            Json(serde_json::to_value(&trace).unwrap_or(json!({}))),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "trace not found", "request_id": request_id })),
        ),
    }
}

/// `GET /admin/api/stats?range=1h|24h|7d` — aggregated call statistics (Phase 3).
///
/// Computes on-demand from the [`TraceLog`] ring buffer: call count, success
/// rate, latency percentiles, top-N tools, top-N instances, and hour-of-day
/// distribution.  Returns `{"range":"...", "total_calls":N, ...}`.
pub async fn handle_admin_stats(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    use crate::gateway::admin::stats::StatsRange;

    let range_str = params.get("range").map(String::as_str).unwrap_or("all");
    let range = StatsRange::from_str(range_str);

    match &s.stats {
        Some(agg) => {
            let stats = agg.compute(range);
            let mut root = serde_json::to_value(&stats).unwrap_or(json!({}));
            if let Some(obj) = root.as_object_mut() {
                obj.insert("p50_ms".to_string(), json!(stats.latency_ms.p50_ms));
                obj.insert("p95_ms".to_string(), json!(stats.latency_ms.p95_ms));
                // Embedded admin UI expects a 0–100 percentage in `success_rate`.
                obj.insert(
                    "success_rate".to_string(),
                    json!(stats.success_rate * 100.0),
                );
            }
            Json(root)
        }
        None => Json(json!({
            "error": "stats aggregator not available — admin feature may be disabled",
            "range": range_str,
            "total_calls": 0,
        })),
    }
}

/// `GET /admin/api/workers` — per-instance worker cards (Phase 4).
///
/// Returns the live registry view of each known instance plus best-effort
/// uptime / heartbeat fields.  CPU and memory are reported as `null` until
/// the per-backend diagnostic resource is wired (separate follow-up — see
/// the `admin::workers` module docs).
pub async fn handle_admin_workers(State(s): State<AdminState>) -> impl IntoResponse {
    let payload = crate::gateway::admin::workers::build_workers_payload(&s.gateway).await;
    Json(payload)
}

fn dispatch_trace_to_admin_row(t: &DispatchTrace) -> Value {
    let ts = chrono::DateTime::<chrono::Utc>::from(t.started_at)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let tool = t.tool_slug.clone().unwrap_or_else(|| t.method.clone());
    let status = if t.ok { "ok" } else { "err" };
    json!({
        "timestamp": ts,
        "request_id": t.request_id,
        "tool": tool,
        "status": status,
        "success": t.ok,
        "total_ms": t.total_ms,
        "instance_id": t.instance_id,
        "dcc_type": t.dcc_type,
    })
}

fn contend_event_to_admin_row(e: ContendEvent) -> Value {
    let label = e.event.as_label();
    let mut message = format!("{label} dcc_type={} instance={}", e.dcc_type, e.instance_id);
    if let Some(r) = &e.reason {
        message.push_str(" — ");
        message.push_str(r);
    }
    json!({
        "timestamp": e.timestamp,
        "level": "info",
        "message": message,
        "source": "contention",
        "event": e.event,
        "dcc_type": e.dcc_type,
        "instance_id": e.instance_id,
        "reason": e.reason,
    })
}
