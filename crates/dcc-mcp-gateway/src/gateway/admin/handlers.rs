//! Admin UI HTTP handlers.

use std::collections::HashMap;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use axum::Json;
use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use dcc_mcp_gateway_core::naming::instance_short;
use serde::Deserialize;
use serde_json::{Value, json};

use super::debug_response::{DebugListQuery, debug_response};
use super::html::ADMIN_HTML;
use super::issue_report::{IssueReportMode, issue_report_filename, issue_report_json};
use super::links::AdminLinkBuilder;
use super::state::{AdminAuditRecord, AdminState};
use super::trace::{AgentContext, DispatchTrace};
use crate::gateway::capability::RefreshReason;
use crate::gateway::capability_service::refresh_all_live_backends;
use crate::gateway::event_log::{ContendEvent, EventKind};
use crate::gateway::resilience::{self as gw_resilience, gateway_limits};
use crate::gateway::response_codec::{
    JSON_MIME, TOKEN_ESTIMATOR, TOON_MIME, default_rest_response_format,
};
use dcc_mcp_db::env::ENV_DCC_MCP_LOG_DIR;
use dcc_mcp_db::read_gateway_log_dir_rows_recent;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};

const ADMIN_FILE_LOG_READ_TIMEOUT: Duration = Duration::from_millis(750);

fn traffic_export_filename() -> &'static str {
    "dcc-mcp-traffic-capture.jsonl"
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
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(200, 1_000);
    Json(crate::gateway::admin::activity::build_activity_payload(&s, limit).await)
}

/// `GET /admin/api/governance` — effective traffic governance policy and decisions.
pub async fn handle_admin_governance(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(200, 1_000);
    Json(crate::gateway::admin::governance::build_governance_payload(&s, limit).await)
}

/// `GET /admin/api/traffic?limit=200` — retained live traffic frames.
pub async fn handle_admin_traffic(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(params): Query<DebugListQuery>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    let limit = params.limit(200, 1_000);
    Json(crate::gateway::admin::traffic::build_traffic_payload(
        &s.gateway.traffic_capture,
        limit,
        json!({
            "admin_traffic_url": links.panel_url("traffic"),
            "traffic_api_url": links.api_url("/traffic"),
            "traffic_export_jsonl_url": links.api_url("/traffic/export"),
        }),
    ))
}

/// `GET /admin/api/traffic/export?limit=1000` — retained live frames as JSONL.
pub async fn handle_admin_traffic_export(
    State(s): State<AdminState>,
    Query(params): Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(1_000, 10_000);
    let body = crate::gateway::admin::traffic::build_traffic_export_body(
        &s.gateway.traffic_capture,
        limit,
    );
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            traffic_export_filename()
        ))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    response
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

#[derive(Debug, Default, Deserialize)]
pub struct AdminSkillDetailQuery {
    pub name: Option<String>,
    pub skill_name: Option<String>,
    pub dcc: Option<String>,
    pub dcc_type: Option<String>,
    pub instance_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct IssueReportQuery {
    /// Default is public-safe. Use `mode=raw` for local evidence review.
    mode: Option<String>,
    /// Compatibility flag for explicit raw export requests.
    include_raw: Option<String>,
}

impl IssueReportQuery {
    fn mode(&self) -> IssueReportMode {
        if self
            .mode
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("raw"))
            || self.include_raw.as_deref().is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            })
        {
            IssueReportMode::RawDebugBundle
        } else {
            IssueReportMode::PublicSafe
        }
    }
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

#[derive(Debug, Default, Deserialize)]
pub struct DeregisteredQuery {
    limit: Option<String>,
}

/// `GET /admin/api/deregistered` — recently auto-deregistered registry rows.
pub async fn handle_admin_deregistered(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DeregisteredQuery>,
) -> impl IntoResponse {
    let limit = params
        .limit
        .as_deref()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, 100);
    let rows = s
        .admin_sqlite_lane
        .as_ref()
        .map(|lane| lane.reader().list_deregistered_instances(limit))
        .unwrap_or_default();

    Json(json!({
        "total": rows.len(),
        "deregistered": rows,
    }))
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

/// `GET /admin/api/skills` — skills currently indexed by the gateway.
pub async fn handle_admin_skills(State(s): State<AdminState>) -> impl IntoResponse {
    reload_skill_paths_and_refresh_backends(&s, RefreshReason::Periodic).await;
    let records = s.gateway.capability_index.snapshot().records;
    Json(crate::gateway::admin::skill_health::build_skill_inventory_payload(&s, records).await)
}

fn admin_skill_query_name(params: &AdminSkillDetailQuery) -> Option<&str> {
    params
        .name
        .as_deref()
        .or(params.skill_name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn admin_skill_query_dcc(params: &AdminSkillDetailQuery) -> Option<&str> {
    params
        .dcc_type
        .as_deref()
        .or(params.dcc.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn admin_instance_matches_filter(instance: &Value, filter: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let instance_id = instance
        .get("instance_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let instance_short = instance
        .get("instance_short")
        .and_then(Value::as_str)
        .unwrap_or_default();
    instance_id.eq_ignore_ascii_case(filter)
        || instance_short.eq_ignore_ascii_case(filter)
        || instance_id
            .to_ascii_lowercase()
            .starts_with(&filter.to_ascii_lowercase())
}

fn admin_parse_backend_skill_detail(instance: &Value) -> Value {
    let instance_id = instance
        .get("instance_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let instance_short = instance
        .get("instance_short")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let dcc_type = instance
        .get("dcc_type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let mut detail = if let Some(result) = instance.get("result").and_then(Value::as_str) {
        serde_json::from_str::<Value>(result).unwrap_or_else(|_| json!({ "message": result }))
    } else if let Some(error) = instance.get("error").and_then(Value::as_str) {
        json!({ "error": error })
    } else {
        json!({})
    };

    if !detail.is_object() {
        detail = json!({ "value": detail });
    }

    if let Some(obj) = detail.as_object_mut() {
        obj.insert("instance_id".to_string(), json!(instance_id));
        obj.insert("instance_short".to_string(), json!(instance_short));
        obj.insert("dcc_type".to_string(), json!(dcc_type));
    }
    detail
}

fn admin_skill_detail_instances(text: &str, instance_filter: Option<&str>) -> Vec<Value> {
    let parsed = serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({ "message": text }));
    if let Some(instances) = parsed.get("instances").and_then(Value::as_array) {
        return instances
            .iter()
            .filter(|instance| admin_instance_matches_filter(instance, instance_filter))
            .map(admin_parse_backend_skill_detail)
            .collect();
    }
    vec![parsed]
}

/// `GET /admin/api/skill-detail` — raw rendered-review details for one skill.
pub async fn handle_admin_skill_detail(
    State(s): State<AdminState>,
    Query(params): Query<AdminSkillDetailQuery>,
) -> impl IntoResponse {
    let Some(skill_name) = admin_skill_query_name(&params) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing required query parameter: name" })),
        );
    };

    reload_skill_paths_and_refresh_backends(&s, RefreshReason::Periodic).await;
    let mut args = json!({ "skill_name": skill_name });
    if let Some(dcc) = admin_skill_query_dcc(&params)
        && let Some(obj) = args.as_object_mut()
    {
        obj.insert("dcc_type".to_string(), json!(dcc));
    }
    if let Some(instance_id) = params
        .instance_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && let Some(obj) = args.as_object_mut()
    {
        obj.insert("instance_id".to_string(), json!(instance_id));
    }

    let (text, is_error) =
        crate::gateway::aggregator::skill_mgmt_dispatch(&s.gateway, "get_skill_info", &args).await;
    let instances = admin_skill_detail_instances(&text, params.instance_id.as_deref());
    let skill = instances.first().cloned().unwrap_or(Value::Null);
    let status = if is_error && instances.is_empty() {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::OK
    };
    (
        status,
        Json(json!({
            "skill": skill,
            "instances": instances,
            "error": if is_error { Some(text) } else { None },
        })),
    )
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
        "actor_id": r.actor_id,
        "actor_name": r.actor_name,
        "actor_email_hash": r.actor_email_hash,
        "actor": display_actor_parts(
            r.actor_name.as_deref(),
            r.actor_id.as_deref(),
            r.auth_subject.as_deref(),
            r.actor_email_hash.as_deref(),
        ),
        "client_platform": r.client_platform,
        "client_os": r.client_os,
        "client_host": r.client_host,
        "auth_subject": r.auth_subject,
        "source_ip": r.source_ip,
        "attribution_trust": r.attribution_trust,
        "parent_request_id": r.parent_request_id,
        "tool": r.action,
        "dcc_type": r.dcc_type,
        "status": if r.success { "ok" } else { "err" },
        "success": r.success,
        "error": r.error,
        "duration_ms": r.duration_ms,
    });
    apply_token_fields(&mut row, r.token_accounting.as_ref());
    if let Some(llm) = r.llm_usage.as_ref() {
        row["llm_usage"] = serde_json::to_value(llm).unwrap_or_default();
    }
    if let Some(links) = links {
        row["links"] = links.request_links(&r.request_id);
    }
    row
}

fn display_actor_parts(
    actor_name: Option<&str>,
    actor_id: Option<&str>,
    auth_subject: Option<&str>,
    actor_email_hash: Option<&str>,
) -> Option<String> {
    actor_name
        .or(actor_id)
        .or(auth_subject)
        .or(actor_email_hash)
        .map(ToString::to_string)
}

fn display_actor(ctx: Option<&AgentContext>) -> Option<String> {
    let ctx = ctx?;
    display_actor_parts(
        ctx.actor_name.as_deref(),
        ctx.actor_id.as_deref(),
        ctx.auth_subject.as_deref(),
        ctx.actor_email_hash.as_deref(),
    )
}

fn apply_token_fields(
    row: &mut Value,
    token_accounting: Option<&crate::gateway::admin::trace::TokenTelemetry>,
) {
    let Some(tokens) = token_accounting else {
        return;
    };
    row["token_accounting"] = serde_json::to_value(tokens).unwrap_or(Value::Null);
    row["response_format"] = json!(tokens.response_format.clone());
    row["token_estimator"] = json!(tokens.token_estimator.clone());
    row["original_bytes"] = json!(tokens.original_bytes);
    row["returned_bytes"] = json!(tokens.returned_bytes);
    row["original_tokens"] = json!(tokens.original_tokens);
    row["returned_tokens"] = json!(tokens.returned_tokens);
    row["saved_tokens"] = json!(tokens.saved_tokens);
    row["savings_pct"] = json!(tokens.savings_pct);
}

fn payload_token_accounting(input: Option<usize>, output: Option<usize>) -> Value {
    let total = match (input, output) {
        (Some(input), Some(output)) => Some(input.saturating_add(output)),
        (Some(input), None) => Some(input),
        (None, Some(output)) => Some(output),
        (None, None) => None,
    };
    json!({
        "kind": "payload",
        "token_estimator": TOKEN_ESTIMATOR,
        "input_tokens": input,
        "output_tokens": output,
        "total_tokens": total,
        "has_input_tokens": input.is_some(),
        "has_output_tokens": output.is_some(),
        "missing_payload_tokens": input.is_none() && output.is_none(),
    })
}

/// `GET /admin/api/calls` — recent calls from the AuditLog ring buffer.
///
/// If no `AuditLog` is attached to the state, returns an empty array.
pub async fn handle_admin_calls(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    let links = Some(AdminLinkBuilder::from_request(&headers, &uri));
    let limit = params.limit(200, 1_000);
    let mut by_rid: HashMap<String, Value> = HashMap::new();
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        for rec in r.list_audits_recent(limit.saturating_mul(4).max(500)) {
            by_rid.insert(
                rec.request_id.clone(),
                admin_audit_row_json(&rec, links.clone()),
            );
        }
    }
    if let Some(log) = &s.audit_log {
        for r in log.lock().iter().rev().take(limit) {
            by_rid.insert(r.request_id.clone(), admin_audit_row_json(r, links.clone()));
        }
    }
    let mut calls: Vec<Value> = by_rid.into_values().collect();
    calls.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    calls.truncate(limit);
    Json(json!({ "total": calls.len(), "calls": calls }))
}

/// `GET /admin/api/logs` — gateway contention events (same ring as
/// `resources://gateway/events`).
///
/// Rows are normalised to `{timestamp, level, message}` for the embedded admin
/// UI. Data comes from [`GatewayState::event_log`] (same ring as
/// `resources://gateway/events`).
pub async fn handle_admin_logs(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(500, 1_000);
    let mut logs: Vec<Value> = s
        .gateway
        .event_log
        .recent_events(limit)
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
        read_gateway_log_dir_rows_recent(&log_dir, limit)
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
        for r in records.iter().rev().take(limit) {
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
                "token_accounting": r.token_accounting.as_ref(),
            }));
        }
    }

    logs.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    logs.truncate(limit);

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
            "response_format": {
                "default": default_rest_response_format().as_str(),
                "legacy_mime": JSON_MIME,
                "compact_mime": TOON_MIME,
                "token_estimator": TOKEN_ESTIMATOR,
            },
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
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    let limit = params.limit(200, 500);
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
    let payload = json!({
        "total": mapped.len(),
        "traces": mapped,
        "links": {
            "admin_traces_url": links.panel_url("traces"),
            "stats_url": links.panel_url("stats"),
        }
    });
    let compact = crate::gateway::admin::compact::compact_trace_list_payload(&payload);
    debug_response(&headers, &params, StatusCode::OK, payload, Some(compact))
}

/// `GET /admin/api/traces/{request_id}` — full waterfall for one call.
///
/// Returns 404 when the trace is not in the ring buffer or SQLite store.
pub async fn handle_admin_trace_detail(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    if let Some(trace) = s.trace_log.as_ref().and_then(|log| log.get(&request_id)) {
        let payload = trace_detail_json(&trace, Some(links.request_links(&request_id)));
        let compact = crate::gateway::admin::compact::compact_trace_detail_payload(&payload);
        return debug_response(&headers, &params, StatusCode::OK, payload, Some(compact));
    }
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        if let Some(trace) = r.get_trace(&request_id) {
            let payload = trace_detail_json(&trace, Some(links.request_links(&request_id)));
            let compact = crate::gateway::admin::compact::compact_trace_detail_payload(&payload);
            return debug_response(&headers, &params, StatusCode::OK, payload, Some(compact));
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
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    let limit = params.limit(200, 1_000);
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    Json(crate::gateway::admin::activity::build_tasks_payload(&s, limit, links).await)
}

/// `GET /admin/api/workflows` — agent/session workflow projection over
/// retained search telemetry, traces, and audit rows.
pub async fn handle_admin_workflows(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    let limit = params.limit(100, 500);
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    Json(crate::gateway::admin::workflows::build_workflows_payload(&s, limit, links).await)
}

/// `GET /admin/api/debug-bundle/{request_id}` — correlated material for one request.
pub async fn handle_admin_debug_bundle(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
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
            let compact = crate::gateway::admin::compact::compact_debug_bundle_payload(&bundle);
            debug_response(&headers, &params, StatusCode::OK, bundle, Some(compact))
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
    Query(params): Query<DebugListQuery>,
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
            let compact = crate::gateway::admin::compact::compact_trace_context_payload(&payload);
            debug_response(&headers, &params, StatusCode::OK, payload, Some(compact))
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
    Query(params): Query<IssueReportQuery>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    match crate::gateway::admin::activity::build_debug_bundle(&s, &request_id).await {
        Some(mut bundle) => {
            let request_links = links.request_links(&request_id);
            bundle["links"] = request_links.clone();
            let report = issue_report_json(&request_id, bundle, request_links, params.mode());
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
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    use crate::gateway::admin::stats::StatsRange;

    let range_str = params.range();
    let range = StatsRange::from_str(range_str);

    match &s.stats {
        Some(agg) => {
            let stats = agg.compute(range);
            let mut root = serde_json::to_value(&stats).unwrap_or(json!({}));
            if let Some(obj) = root.as_object_mut() {
                obj.insert("p50_ms".to_string(), json!(stats.latency_ms.p50_ms));
                obj.insert("p95_ms".to_string(), json!(stats.latency_ms.p95_ms));
                obj.insert(
                    "governance".to_string(),
                    crate::gateway::admin::governance::build_governance_stats(&s),
                );
                obj.insert(
                    "avg_tokens_per_call".to_string(),
                    json!(stats.avg_total_tokens_per_call),
                );
                obj.insert(
                    "payload_token_estimator".to_string(),
                    json!(TOKEN_ESTIMATOR),
                );
                // Embedded admin UI expects a 0–100 percentage in `success_rate`.
                obj.insert(
                    "success_rate".to_string(),
                    json!(stats.success_rate * 100.0),
                );
            }
            debug_response(&headers, &params, StatusCode::OK, root.clone(), Some(root))
        }
        None => {
            let root = json!({
            "error": "stats aggregator not available — admin feature may be disabled",
            "range": range_str,
            "total_calls": 0,
            "successful_calls": 0,
            "failed_calls": 0,
            "success_rate": 0.0,
            "total_input_tokens": 0,
            "total_output_tokens": 0,
            "total_tokens": 0,
            "avg_input_tokens_per_call": 0.0,
            "avg_output_tokens_per_call": 0.0,
            "avg_total_tokens_per_call": 0.0,
            "avg_tokens_per_call": 0.0,
            "payload_token_estimator": TOKEN_ESTIMATOR,
            "payload_token_usage": crate::gateway::admin::stats::PayloadTokenUsageStats::empty(0),
            "token_usage": crate::gateway::admin::stats::TokenUsageStats::default(),
            "governance": crate::gateway::admin::governance::build_governance_stats(&s),
            });
            debug_response(&headers, &params, StatusCode::OK, root.clone(), Some(root))
        }
    }
}

/// `GET /admin/api/search-telemetry?limit=200` — recent search-quality
/// records plus aggregate hit-rate metrics.
pub async fn handle_admin_search_telemetry(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(200)
        .clamp(1, 1_000);
    Json(
        serde_json::to_value(s.gateway.search_telemetry.snapshot(limit)).unwrap_or(json!({
            "stats": {},
            "total": 0,
            "recent": [],
        })),
    )
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

async fn reload_skill_paths_and_refresh_backends(state: &AdminState, reason: RefreshReason) {
    if let Some(cb) = state.skill_paths_reload.clone() {
        cb();
    }
    refresh_all_live_backends(&state.gateway, reason).await;
}

/// `GET /admin/api/skill-paths` — skill search paths (snapshot + SQLite custom).
pub async fn handle_admin_skill_paths(State(s): State<AdminState>) -> impl IntoResponse {
    Json(crate::gateway::admin::skill_health::build_skill_paths_payload(&s))
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
        reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
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
        reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
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
    let input_tokens = trace.input_tokens();
    let output_tokens = trace.output_tokens();
    let total_tokens = trace.total_tokens();
    if let Some(links) = links {
        value["links"] = links;
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert("input_tokens".to_string(), json!(input_tokens));
        obj.insert("output_tokens".to_string(), json!(output_tokens));
        obj.insert("total_tokens".to_string(), json!(total_tokens));
        obj.insert("estimated_tokens".to_string(), json!(total_tokens));
        obj.insert("estimated_total_tokens".to_string(), json!(total_tokens));
        obj.insert(
            "payload_token_accounting".to_string(),
            payload_token_accounting(input_tokens, output_tokens),
        );
        obj.insert(
            "payload_token_estimator".to_string(),
            json!(TOKEN_ESTIMATOR),
        );
    }
    value
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
    let input_tokens = t.input_tokens();
    let output_tokens = t.output_tokens();
    let total_tokens = t.total_tokens();
    let agent_id = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.agent_id.clone());
    let agent_name = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.agent_name.clone());
    let agent_model = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.model.clone().or_else(|| ctx.model_version.clone()));
    let actor = display_actor(t.agent_context.as_ref());
    let actor_id = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.actor_id.clone());
    let actor_name = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.actor_name.clone());
    let actor_email_hash = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.actor_email_hash.clone());
    let client_platform = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.client_platform.clone());
    let client_os = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.client_os.clone());
    let client_host = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.client_host.clone());
    let auth_subject = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.auth_subject.clone());
    let source_ip = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.source_ip.clone());
    let attribution_trust = t
        .agent_context
        .as_ref()
        .map(|ctx| ctx.trust.clone())
        .filter(|trust| !trust.is_empty());
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
        "actor_id": actor_id,
        "actor_name": actor_name,
        "actor_email_hash": actor_email_hash,
        "actor": actor,
        "client_platform": client_platform,
        "client_os": client_os,
        "client_host": client_host,
        "auth_subject": auth_subject,
        "source_ip": source_ip,
        "attribution_trust": attribution_trust,
        "span_count": t.span_count(),
        "input_bytes": t.input_bytes(),
        "output_bytes": t.output_bytes(),
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_tokens,
        "payload_token_accounting": payload_token_accounting(input_tokens, output_tokens),
        "payload_token_estimator": TOKEN_ESTIMATOR,
        "slowest_span_name": slowest_span_name,
        "slowest_span_ms": slowest_span_ms,
    });
    apply_token_fields(&mut row, t.token_accounting.as_ref());
    if let Some(llm) = t.llm_usage.as_ref() {
        row["llm_usage"] = serde_json::to_value(llm).unwrap_or_default();
    }
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
