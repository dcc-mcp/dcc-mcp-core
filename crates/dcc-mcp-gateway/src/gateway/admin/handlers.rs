//! Admin UI HTTP handlers.

use std::collections::HashMap;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use axum::Json;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
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
use dcc_mcp_db::env::ENV_DCC_MCP_LOG_DIR;
use dcc_mcp_db::read_gateway_log_dir_rows_recent;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};

const ADMIN_FILE_LOG_READ_TIMEOUT: Duration = Duration::from_millis(750);

#[derive(Clone)]
struct AdminLinkBuilder {
    origin: String,
    admin_base: String,
}

impl AdminLinkBuilder {
    fn from_request(headers: &HeaderMap, uri: &Uri) -> Self {
        let proto = header_value(headers, "x-forwarded-proto").unwrap_or_else(|| "http".into());
        let host = header_value(headers, "x-forwarded-host")
            .or_else(|| header_value(headers, "host"))
            .unwrap_or_else(|| "127.0.0.1:9765".into());
        let admin_base = admin_base_path(uri.path());
        Self {
            origin: format!("{proto}://{host}"),
            admin_base,
        }
    }

    fn request_links(&self, request_id: &str) -> Value {
        let encoded = encode_url_component(request_id);
        json!({
            "admin_trace_url": format!(
                "{}{}?panel=traces&trace={}",
                self.origin, self.admin_base, encoded
            ),
            "trace_api_url": format!(
                "{}{}/api/traces/{}",
                self.origin, self.admin_base, encoded
            ),
            "debug_bundle_url": format!(
                "{}{}/api/debug-bundle/{}",
                self.origin, self.admin_base, encoded
            ),
            "issue_report_url": format!(
                "{}{}/api/issue-report/{}",
                self.origin, self.admin_base, encoded
            ),
            "openapi_inspector_url": self.panel_url("openapi"),
            "openapi_spec_url": format!("{}/v1/openapi.json", self.origin),
            "openapi_docs_url": format!("{}/docs", self.origin),
            "stats_url": self.panel_url("stats"),
        })
    }

    fn panel_url(&self, panel: &str) -> String {
        format!("{}{}?panel={panel}", self.origin, self.admin_base)
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn admin_base_path(path: &str) -> String {
    if path.starts_with("/v1/debug/") {
        return "/admin".to_string();
    }
    let base = path
        .find("/api")
        .map(|idx| &path[..idx])
        .unwrap_or(path)
        .trim_end_matches('/');
    if base.is_empty() {
        "/admin".to_string()
    } else {
        base.to_string()
    }
}

fn encode_url_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn issue_report_filename(request_id: &str) -> String {
    let mut safe = String::with_capacity(request_id.len());
    for ch in request_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            safe.push(ch);
        } else {
            safe.push('-');
        }
    }
    if safe.is_empty() {
        safe.push_str("request");
    }
    format!("dcc-mcp-issue-report-{safe}.json")
}

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

#[derive(Debug, Default, Deserialize)]
pub struct AdminInstancesQuery {
    /// Default: current routable instances. `all` exposes the registry view.
    view: Option<String>,
    /// Compatibility flag for callers that want stale diagnostic rows.
    include_stale: Option<bool>,
    /// Include rows whose owner process is gone. Diagnostic use only.
    include_dead: Option<bool>,
}

/// `GET /admin/api/instances` — list current routable instances by default.
pub async fn handle_admin_instances(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<AdminInstancesQuery>,
) -> impl IntoResponse {
    let include_dead = params.include_dead.unwrap_or(false);
    let include_stale = params.include_stale.unwrap_or(false);
    let registry_view = params
        .view
        .as_deref()
        .is_some_and(|v| v.eq_ignore_ascii_case("all") || v.eq_ignore_ascii_case("registry"))
        || include_stale
        || include_dead;

    let registry = s.gateway.registry.read().await;
    let (entries, evicted_dead) = if registry_view {
        if include_dead {
            (s.gateway.all_instances(&registry), 0usize)
        } else {
            match s.gateway.read_alive_instances(&registry) {
                Ok((entries, evicted)) => (entries, evicted),
                Err(err) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "registry-read-failed",
                            "message": err.to_string(),
                        })),
                    )
                        .into_response();
                }
            }
        }
    } else {
        (s.gateway.live_instances(&registry), 0usize)
    };

    let known_total = entries.len();
    let mut live_count = 0usize;
    let mut stale_count = 0usize;
    let mut unhealthy_count = 0usize;
    let instances: Vec<Value> = entries
        .into_iter()
        .filter(|e| {
            let stale = e.is_stale(s.gateway.stale_timeout);
            if stale {
                stale_count += 1;
            }
            registry_view || !stale
        })
        .map(|e| {
            let mut v = s.gateway.instance_json(&e);
            match v["status"].as_str() {
                Some("available" | "busy") => live_count += 1,
                Some("stale") => {}
                _ => unhealthy_count += 1,
            }
            // Alias `instance_id` → `id` for the UI convenience.
            let id = v["instance_id"].clone();
            v.as_object_mut().map(|m| m.insert("id".into(), id));
            v
        })
        .collect();

    Json(json!({
        "total": instances.len(),
        "known_total": known_total,
        "evicted_dead": evicted_dead,
        "view": if registry_view { "all" } else { "live" },
        "summary": {
            "live": live_count,
            "stale": stale_count,
            "unhealthy": unhealthy_count,
        },
        "instances": instances,
    }))
    .into_response()
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

fn admin_audit_row_json(r: &AdminAuditRecord, links: Option<AdminLinkBuilder>) -> Value {
    let ts = r
        .timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|_d| chrono::DateTime::<chrono::Utc>::from(r.timestamp).to_rfc3339())
        .unwrap_or_default();
    let mut row = json!({
        "timestamp": ts,
        "request_id": r.request_id,
        "trace_id": r.trace_id,
        "span_id": r.span_id,
        "parent_span_id": r.parent_span_id,
        "method": r.method,
        "instance_id": r.instance_id,
        "session_id": r.session_id,
        "transport": r.transport,
        "agent_id": r.agent_id,
        "agent_name": r.agent_name,
        "agent_model": r.agent_model,
        "parent_request_id": r.parent_request_id,
        "tool": r.action,
        "dcc_type": r.dcc_type,
        "status": if r.success { "ok" } else { "err" },
        "success": r.success,
        "error": r.error,
        "duration_ms": r.duration_ms,
    });
    if let Some(links) = links {
        row["links"] = links.request_links(&r.request_id);
    }
    row
}

/// `GET /admin/api/calls` — recent calls from the AuditLog ring buffer.
///
/// If no `AuditLog` is attached to the state, returns an empty array.
pub async fn handle_admin_calls(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    let links = Some(AdminLinkBuilder::from_request(&headers, &uri));
    let mut by_rid: HashMap<String, Value> = HashMap::new();
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        for rec in r.list_audits_recent(500) {
            by_rid.insert(
                rec.request_id.clone(),
                admin_audit_row_json(&rec, links.clone()),
            );
        }
    }
    if let Some(log) = &s.audit_log {
        for r in log.lock().iter().rev().take(200) {
            by_rid.insert(r.request_id.clone(), admin_audit_row_json(r, links.clone()));
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
    let file_log_task = tokio::task::spawn_blocking(move || {
        if !std::fs::metadata(&log_dir)
            .map(|m| m.is_dir())
            .unwrap_or(false)
        {
            return Vec::new();
        }
        read_gateway_log_dir_rows_recent(&log_dir, 500)
    });
    match tokio::time::timeout(ADMIN_FILE_LOG_READ_TIMEOUT, file_log_task).await {
        Ok(Ok(mut file_logs)) => logs.append(&mut file_logs),
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "admin file log read task failed");
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = ADMIN_FILE_LOG_READ_TIMEOUT.as_millis() as u64,
                "admin file log read timed out"
            );
        }
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
    let gateway_sentinels = registry.list_instances(GATEWAY_SENTINEL_DCC_TYPE);
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
            "gateway": gateway_health_snapshot(&gateway_sentinels),
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

fn gateway_health_snapshot(sentinels: &[ServiceEntry]) -> Value {
    let mut rows: Vec<Value> = sentinels.iter().map(gateway_sentinel_json).collect();
    rows.sort_by(|a, b| {
        let role_a = a.get("role").and_then(Value::as_str).unwrap_or("");
        let role_b = b.get("role").and_then(Value::as_str).unwrap_or("");
        let rank_a = if role_a == "active" { 0 } else { 1 };
        let rank_b = if role_b == "active" { 0 } else { 1 };
        rank_a.cmp(&rank_b).then_with(|| {
            let ta = a
                .get("last_heartbeat_unix")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let tb = b
                .get("last_heartbeat_unix")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            tb.cmp(&ta)
        })
    });
    let current = rows
        .iter()
        .find(|row| row.get("role").and_then(Value::as_str) == Some("active"))
        .cloned()
        .or_else(|| rows.first().cloned());
    let candidates: Vec<Value> = rows
        .into_iter()
        .filter(|row| row.get("role").and_then(Value::as_str) != Some("active"))
        .collect();
    json!({
        "current": current,
        "candidates": candidates,
    })
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
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
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
    let mapped: Vec<Value> = traces
        .iter()
        .map(|trace| dispatch_trace_to_admin_row(trace, Some(links.clone())))
        .collect();
    Json(json!({
        "total": mapped.len(),
        "traces": mapped,
        "links": {
            "admin_traces_url": links.panel_url("traces"),
            "stats_url": links.panel_url("stats"),
        }
    }))
}

/// `GET /admin/api/traces/{request_id}` — full waterfall for one call.
///
/// Returns 404 when the trace is not in the ring buffer or SQLite store.
pub async fn handle_admin_trace_detail(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    if let Some(trace) = s.trace_log.as_ref().and_then(|log| log.get(&request_id)) {
        return (
            StatusCode::OK,
            Json(trace_detail_json(
                &trace,
                Some(links.request_links(&request_id)),
            )),
        )
            .into_response();
    }
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        if let Some(trace) = r.get_trace(&request_id) {
            return (
                StatusCode::OK,
                Json(trace_detail_json(
                    &trace,
                    Some(links.request_links(&request_id)),
                )),
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
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    match crate::gateway::admin::activity::build_debug_bundle(&s, &request_id).await {
        Some(mut bundle) => {
            let resolved_request_id = bundle
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or(&request_id)
                .to_string();
            bundle["links"] = links.request_links(&resolved_request_id);
            (StatusCode::OK, Json(bundle)).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "debug bundle not found", "request_id": request_id })),
        )
            .into_response(),
    }
}

/// `GET /v1/debug/traces/{lookup_id}` — trace lookup by trace id or request id.
pub async fn handle_v1_debug_trace_lookup(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Path(lookup_id): Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    match crate::gateway::admin::activity::build_debug_bundle(&s, &lookup_id).await {
        Some(bundle) => {
            let request_id = bundle
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or(&lookup_id);
            let payload = json!({
                "lookup_id": lookup_id,
                "trace_id": bundle.get("trace_id").cloned().unwrap_or(Value::Null),
                "request_id": request_id,
                "request_ids": bundle.get("request_ids").cloned().unwrap_or_else(|| json!([])),
                "trace": bundle.get("trace").cloned().unwrap_or(Value::Null),
                "traces": bundle.get("traces").cloned().unwrap_or_else(|| json!([])),
                "links": links.request_links(request_id),
            });
            (StatusCode::OK, Json(payload)).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "trace not found", "lookup_id": lookup_id })),
        )
            .into_response(),
    }
}

/// `GET /admin/api/issue-report/{request_id}` — export a GitHub-attachable JSON report.
pub async fn handle_admin_issue_report(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    match crate::gateway::admin::activity::build_debug_bundle(&s, &request_id).await {
        Some(mut bundle) => {
            let request_links = links.request_links(&request_id);
            bundle["links"] = request_links.clone();
            let report = issue_report_json(&request_id, bundle, request_links);
            let mut response = (StatusCode::OK, Json(report)).into_response();
            if let Ok(value) = HeaderValue::from_str(&format!(
                "attachment; filename=\"{}\"",
                issue_report_filename(&request_id)
            )) {
                response
                    .headers_mut()
                    .insert(header::CONTENT_DISPOSITION, value);
            }
            response
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "issue report not found", "request_id": request_id })),
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

fn trace_detail_json(trace: &DispatchTrace, links: Option<Value>) -> Value {
    let mut value = serde_json::to_value(trace).unwrap_or(json!({}));
    if let Some(links) = links {
        value["links"] = links;
    }
    value
}

fn issue_report_json(request_id: &str, bundle: Value, links: Value) -> Value {
    let trace = bundle.get("trace").cloned().unwrap_or(Value::Null);
    let audit = bundle.get("audit").cloned().unwrap_or(Value::Null);
    let tool = trace
        .get("tool_slug")
        .or_else(|| trace.get("method"))
        .or_else(|| audit.get("tool"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let dcc_type = trace
        .get("dcc_type")
        .or_else(|| audit.get("dcc_type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let status = trace
        .get("ok")
        .and_then(Value::as_bool)
        .or_else(|| audit.get("success").and_then(Value::as_bool))
        .map(|ok| if ok { "ok" } else { "failed" })
        .unwrap_or("unknown");
    let total_ms = trace
        .get("total_ms")
        .or_else(|| audit.get("duration_ms"))
        .cloned()
        .unwrap_or(Value::Null);
    let trace_id = bundle
        .get("trace_id")
        .cloned()
        .unwrap_or_else(|| trace.get("trace_id").cloned().unwrap_or(Value::Null));
    let postmortem = bundle.get("postmortem").cloned().unwrap_or(Value::Null);
    let previous_call_count = postmortem
        .get("previous_calls")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let gateway_event_count = postmortem
        .get("gateway_events")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let generated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let title = format!("DCC-MCP request {request_id} {status}: {tool}");
    let body_template = format!(
        "## Summary\n\nRequest `{request_id}` returned `{status}` for `{tool}` on `{dcc_type}`.\n\n## Attached data\n\nUpload this JSON export to the issue so maintainers can inspect trace spans, audit metadata, payload previews, postmortem context, and links.\n\n## Notes\n\nReview the JSON for secrets or proprietary scene paths before uploading."
    );

    json!({
        "schema_version": "dcc-mcp.admin.issue-report.v1",
        "report_type": "github_issue_debug_json",
        "generated_at": generated_at,
        "request_id": request_id,
        "trace_id": trace_id,
        "summary": {
            "title": title,
            "status": status,
            "tool": tool,
            "dcc_type": dcc_type,
            "total_ms": total_ms,
            "postmortem": {
                "previous_call_count": previous_call_count,
                "gateway_event_count": gateway_event_count,
            },
        },
        "github_issue": {
            "title": title,
            "body_template": body_template,
            "suggested_labels": ["bug", "admin-telemetry"],
        },
        "links": links,
        "privacy_note": "Review request and response payloads before uploading; this export may include scene paths, prompts, tokens, or proprietary data.",
        "debug_bundle": bundle,
    })
}

fn dispatch_trace_to_admin_row(t: &DispatchTrace, links: Option<AdminLinkBuilder>) -> Value {
    let ts = chrono::DateTime::<chrono::Utc>::from(t.started_at)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let tool = t.tool_slug.clone().unwrap_or_else(|| t.method.clone());
    let status = if t.ok { "ok" } else { "err" };
    let (slowest_span_name, slowest_span_ms) = t
        .slowest_span()
        .map(|(span, ms)| (Some(span.name.clone()), Some(ms)))
        .unwrap_or((None, None));
    let agent_id = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.agent_id.clone());
    let agent_name = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.agent_name.clone());
    let agent_model = t.agent_context.as_ref().and_then(|ctx| ctx.model.clone());
    let mut row = json!({
        "timestamp": ts,
        "request_id": t.request_id,
        "trace_id": t.trace_id,
        "span_id": t.span_id,
        "parent_span_id": t.parent_span_id,
        "parent_request_id": t.parent_request_id,
        "tool": tool,
        "status": status,
        "success": t.ok,
        "total_ms": t.total_ms,
        "instance_id": t.instance_id,
        "dcc_type": t.dcc_type,
        "transport": t.transport,
        "agent_id": agent_id,
        "agent_name": agent_name,
        "agent_model": agent_model,
        "span_count": t.span_count(),
        "input_bytes": t.input_bytes(),
        "output_bytes": t.output_bytes(),
        "slowest_span_name": slowest_span_name,
        "slowest_span_ms": slowest_span_ms,
    });
    if let Some(links) = links {
        row["links"] = links.request_links(&t.request_id);
    }
    row
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
