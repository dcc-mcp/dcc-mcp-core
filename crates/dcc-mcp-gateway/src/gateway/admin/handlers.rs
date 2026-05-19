//! Admin UI HTTP handlers.

use std::collections::HashMap;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use dcc_mcp_gateway_core::naming::instance_short;
use serde::Deserialize;
use serde_json::{Value, json};

use super::html::ADMIN_HTML;
use super::state::{AdminAuditRecord, AdminState};
use super::trace::DispatchTrace;
use crate::gateway::capability::RefreshReason;
use crate::gateway::capability_service::refresh_all_live_backends;
use crate::gateway::event_log::{ContendEvent, EventKind};
use crate::gateway::resilience::{self as gw_resilience, gateway_limits};
use crate::gateway::state::entry_to_json;
use dcc_mcp_db::env::ENV_DCC_MCP_LOG_DIR;
use dcc_mcp_db::read_gateway_log_dir_rows_recent;

/// `GET /admin` — serve the inline HTML dashboard.
pub async fn handle_admin_ui() -> impl IntoResponse {
    let mut resp = axum::response::Html(ADMIN_HTML).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store"),
    );
    resp
}

/// `GET /admin/api/activity` — unified operator / agent activity timeline.
pub async fn handle_admin_activity(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(200)
        .clamp(1, 1_000);
    Json(crate::gateway::admin::activity::build_activity_payload(&s, limit).await)
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

fn admin_audit_row_json(r: &AdminAuditRecord) -> Value {
    let ts = r
        .timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|_d| chrono::DateTime::<chrono::Utc>::from(r.timestamp).to_rfc3339())
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
}

/// `GET /admin/api/calls` — recent calls from the AuditLog ring buffer.
///
/// If no `AuditLog` is attached to the state, returns an empty array.
pub async fn handle_admin_calls(State(s): State<AdminState>) -> impl IntoResponse {
    let mut by_rid: HashMap<String, Value> = HashMap::new();
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        for rec in r.list_audits_recent(500) {
            by_rid.insert(rec.request_id.clone(), admin_audit_row_json(&rec));
        }
    }
    if let Some(log) = &s.audit_log {
        for r in log.lock().iter().rev().take(200) {
            by_rid.insert(r.request_id.clone(), admin_audit_row_json(r));
        }
    }
    let mut calls: Vec<Value> = by_rid.into_values().collect();
    calls.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    calls.truncate(200);
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

    // Merge on-disk log files (issue #963).
    let log_dir = std::env::var(ENV_DCC_MCP_LOG_DIR).unwrap_or_else(|_| {
        #[cfg(test)]
        {
            String::new()
        }
        #[cfg(not(test))]
        {
            dcc_mcp_db::default_gateway_log_dir()
        }
    });
    if std::fs::metadata(&log_dir)
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        let mut file_logs = read_gateway_log_dir_rows_recent(&log_dir, 500);
        logs.append(&mut file_logs);
    }

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
    let mut by_id: HashMap<String, DispatchTrace> = HashMap::new();
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        for t in r.list_traces_since(None, limit.saturating_mul(4).max(500)) {
            by_id.insert(t.request_id.clone(), t);
        }
    }
    if let Some(log) = &s.trace_log {
        for t in log.recent(limit) {
            by_id.insert(t.request_id.clone(), t);
        }
    }
    let mut traces: Vec<DispatchTrace> = by_id.into_values().collect();
    traces.sort_by(|a, b| {
        let ta = a.started_at.duration_since(UNIX_EPOCH).ok();
        let tb = b.started_at.duration_since(UNIX_EPOCH).ok();
        tb.cmp(&ta)
    });
    traces.truncate(limit);
    let mapped: Vec<Value> = traces.iter().map(dispatch_trace_to_admin_row).collect();
    Json(json!({ "total": mapped.len(), "traces": mapped }))
}

/// `GET /admin/api/traces/{request_id}` — full waterfall for one call.
///
/// Returns 404 when the trace is not in the ring buffer or SQLite store.
pub async fn handle_admin_trace_detail(
    State(s): State<AdminState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(trace) = s.trace_log.as_ref().and_then(|log| log.get(&request_id)) {
        return (
            StatusCode::OK,
            Json(serde_json::to_value(&trace).unwrap_or(json!({}))),
        )
            .into_response();
    }
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        if let Some(trace) = r.get_trace(&request_id) {
            return (
                StatusCode::OK,
                Json(serde_json::to_value(&trace).unwrap_or(json!({}))),
            )
                .into_response();
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "trace not found", "request_id": request_id })),
    )
        .into_response()
}

/// `GET /admin/api/tasks` — task-like projection over retained gateway work.
pub async fn handle_admin_tasks(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(200)
        .clamp(1, 1_000);
    Json(crate::gateway::admin::activity::build_tasks_payload(&s, limit).await)
}

/// `GET /admin/api/debug-bundle/{request_id}` — correlated material for one request.
pub async fn handle_admin_debug_bundle(
    State(s): State<AdminState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match crate::gateway::admin::activity::build_debug_bundle(&s, &request_id).await {
        Some(bundle) => (StatusCode::OK, Json(bundle)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "debug bundle not found", "request_id": request_id })),
        )
            .into_response(),
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

#[derive(Debug, Deserialize)]
pub struct SkillPathAddBody {
    pub path: String,
}

async fn wait_for_custom_skill_path_visible(
    lane: &crate::gateway::admin::sqlite_lane::AdminSqliteLane,
    needle: &str,
) {
    for _ in 0..80 {
        if lane
            .reader()
            .list_custom_skill_paths()
            .iter()
            .any(|(_, p)| p == needle)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tracing::warn!(path = %needle, "skill path not visible after 2 s poll — writer may be lagging");
}

async fn wait_until_custom_skill_path_id_removed(
    lane: &crate::gateway::admin::sqlite_lane::AdminSqliteLane,
    id: i64,
) {
    for _ in 0..80 {
        if !lane
            .reader()
            .list_custom_skill_paths()
            .iter()
            .any(|(i, _)| *i == id)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tracing::warn!(
        skill_path_id = id,
        "skill path id not removed after 2 s poll — writer may be lagging"
    );
}

fn push_admin_operator_note(state: &AdminState, msg: String) {
    state.gateway.event_log.push(ContendEvent::new(
        EventKind::OperatorNote,
        "admin",
        "gateway",
        Some(msg),
    ));
}

/// `GET /admin/api/skill-paths` — skill search paths (snapshot + SQLite custom).
pub async fn handle_admin_skill_paths(State(s): State<AdminState>) -> impl IntoResponse {
    let mut flat: Vec<Value> = s
        .skill_paths_snapshot
        .iter()
        .map(|e| json!({ "path": e.path, "source": e.source }))
        .collect();
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        for (id, path) in r.list_custom_skill_paths() {
            if !flat
                .iter()
                .any(|v| v.get("path").and_then(|x| x.as_str()) == Some(path.as_str()))
            {
                flat.push(json!({ "path": path, "source": "admin_custom", "id": id }));
            }
        }
    }
    Json(json!({ "paths": flat }))
}

/// `POST /admin/api/skill-paths` — enqueue a custom path; embedder hook may reload disk catalog.
pub async fn handle_admin_skill_path_add(
    State(s): State<AdminState>,
    Json(body): Json<SkillPathAddBody>,
) -> impl IntoResponse {
    let path = body.path.trim().to_string();
    if path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path is empty" })),
        )
            .into_response();
    }
    let Some(ref lane) = s.admin_sqlite_lane else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "admin sqlite lane disabled" })),
        )
            .into_response();
    };
    if lane.try_add_skill_path(path.clone()) {
        wait_for_custom_skill_path_visible(lane, &path).await;
        if let Some(cb) = s.skill_paths_reload.clone() {
            cb();
        }
        let gw = s.gateway.clone();
        tokio::spawn(async move {
            refresh_all_live_backends(&gw, RefreshReason::ToolsListChanged).await;
        });
        push_admin_operator_note(
            &s,
            format!("Custom skill path persisted; catalog reload hook ran: {path}"),
        );
        (StatusCode::OK, Json(json!({ "ok": true, "path": path }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "persist queue full or sqlite disabled" })),
        )
            .into_response()
    }
}

/// `DELETE /admin/api/skill-paths/{id}` — remove a custom path row.
pub async fn handle_admin_skill_path_delete(
    State(s): State<AdminState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let Some(ref lane) = s.admin_sqlite_lane else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "admin sqlite lane disabled" })),
        )
            .into_response();
    };
    if lane.try_delete_skill_path(id) {
        wait_until_custom_skill_path_id_removed(lane, id).await;
        if let Some(cb) = s.skill_paths_reload.clone() {
            cb();
        }
        let gw = s.gateway.clone();
        tokio::spawn(async move {
            refresh_all_live_backends(&gw, RefreshReason::ToolsListChanged).await;
        });
        push_admin_operator_note(
            &s,
            format!("Custom skill path removed (id={id}); catalog reload hook ran."),
        );
        (StatusCode::OK, Json(json!({ "ok": true, "id": id }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "persist queue full or sqlite disabled" })),
        )
            .into_response()
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
    if matches!(e.event, EventKind::OperatorNote) {
        let message = e
            .reason
            .clone()
            .unwrap_or_else(|| "operator note".to_string());
        return json!({
            "timestamp": e.timestamp,
            "level": "info",
            "message": message,
            "source": "admin",
            "event": e.event,
            "dcc_type": e.dcc_type,
            "instance_id": e.instance_id,
            "reason": e.reason,
        });
    }
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
