use super::*;

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::gateway::admin::trace::{AgentContext, TraceContext};
use crate::gateway::agent_telemetry::{
    AgentWorkflowEvent, error_kind_from_text, policy_reason_from_value,
};
use crate::gateway::capability::{RefreshReason, tool_slug};
use crate::gateway::capability_service::{
    SearchResponseContext, ServiceError, describe_tool_full, index_generation,
    parse_search_payload, refresh_all_live_backends, search_hit_to_value_with_context,
    search_service_hits_for_policy, service_error_to_json,
};
use crate::gateway::response_codec::{compact_call_batch_payload, compact_describe_payload};
use crate::gateway::search_telemetry::{SearchTelemetryInput, search_id_from_payload};
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};

use super::rest_support::*;
use super::rest_trace::*;

const GATEWAY_HANDOFF_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
pub struct DccInstanceDescribeQuery {
    /// Backend tool / callable id (e.g. `maya_scripting__execute_python`).
    #[serde(alias = "tool", alias = "action")]
    backend_tool: String,
}

/// `GET /health` — simple liveness probe.
pub async fn handle_health() -> impl IntoResponse {
    Json(json!({"ok": true, "service": "dcc-mcp-gateway"}))
}

/// `POST /gateway/yield` — ask this gateway to voluntarily release its port.
pub async fn handle_gateway_yield(
    State(gs): State<GatewayState>,
    body: axum::body::Bytes,
) -> Response {
    #[derive(Deserialize)]
    struct YieldRequest {
        challenger_version: Option<String>,
        reason: Option<String>,
        suggested_successor: Option<String>,
    }

    let request: YieldRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"ok": false, "error": format!("Invalid body: {err}")})),
            )
                .into_response();
        }
    };

    let challenger_version = request.challenger_version.unwrap_or_default();
    if challenger_version.trim().is_empty() {
        return gateway_yield_unavailable_response(
            &gs.server_version,
            None,
            "missing challenger_version; cooperative gateway yield requires a newer challenger",
        );
    }

    if is_newer_version(&challenger_version, &gs.server_version) {
        let reason = request
            .reason
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("version_preempt");
        tracing::info!(
            challenger = %challenger_version,
            current = %gs.server_version,
            reason = %reason,
            "Gateway yield requested — initiating graceful handoff"
        );
        announce_gateway_handoff(
            &gs,
            &challenger_version,
            reason,
            request.suggested_successor.as_deref(),
        )
        .await;
        let _ = gs.yield_tx.send(true);
        Json(json!({
            "ok": true,
            "handoff": true,
            "message": format!(
                "Gateway v{} yielding to challenger v{}. Port will be free shortly.",
                gs.server_version, challenger_version
            )
        }))
        .into_response()
    } else {
        gateway_yield_unavailable_response(
            &gs.server_version,
            Some(&challenger_version),
            &format!(
                "challenger version {challenger_version} is not newer than gateway {}",
                gs.server_version
            ),
        )
    }
}

async fn announce_gateway_handoff(
    gs: &GatewayState,
    challenger_version: &str,
    reason: &str,
    suggested_successor: Option<&str>,
) {
    let issued = SystemTime::now();
    let deadline = issued + GATEWAY_HANDOFF_GRACE;
    let sentinel = mark_active_gateway_sentinel_shutting_down(gs).await;
    let from_instance_id = sentinel
        .as_ref()
        .map(|entry| entry.instance_id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let in_flight_calls = gs.pending_calls.read().await.len();
    let subscribed_clients = gs.events_tx.receiver_count();

    crate::gateway::event_log::record_event(
        &gs.event_log,
        #[cfg(feature = "prometheus")]
        &gs.gateway_metrics,
        crate::gateway::event_log::EventKind::VoluntaryYield,
        GATEWAY_SENTINEL_DCC_TYPE,
        short_instance_id(&from_instance_id),
        Some(format!(
            "handoff reason={reason}; challenger_version={challenger_version}"
        )),
    );

    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/gateway/handoff",
        "params": {
            "from": from_instance_id,
            "reason": reason,
            "challenger_version": challenger_version,
            "issued_unix_secs": unix_secs(issued),
            "deadline_unix_secs": unix_secs(deadline),
            "grace_ms": GATEWAY_HANDOFF_GRACE.as_millis() as u64,
            "endpoint_after_handoff_will_be_same": true,
            "in_flight_calls": in_flight_calls,
            "subscribed_clients": subscribed_clients,
            "suggested_successor": suggested_successor,
        }
    });

    if gs.events_tx.receiver_count() > 0 {
        let _ = gs.events_tx.send(notification.to_string());
    }
}

async fn mark_active_gateway_sentinel_shutting_down(gs: &GatewayState) -> Option<ServiceEntry> {
    let sentinel = {
        let registry = gs.registry.read().await;
        select_active_gateway_sentinel(
            registry.list_instances(GATEWAY_SENTINEL_DCC_TYPE),
            &gs.own_host,
            gs.own_port,
        )
    }?;
    let key = sentinel.key();
    {
        let registry = gs.registry.read().await;
        if let Err(err) = registry.update_status(&key, ServiceStatus::ShuttingDown) {
            tracing::warn!(
                error = %err,
                instance_id = %sentinel.instance_id,
                "Failed to mark gateway sentinel as shutting down"
            );
            return Some(sentinel);
        }
    }
    let mut updated = sentinel;
    updated.status = ServiceStatus::ShuttingDown;
    Some(updated)
}

fn select_active_gateway_sentinel(
    sentinels: Vec<ServiceEntry>,
    own_host: &str,
    own_port: u16,
) -> Option<ServiceEntry> {
    sentinels
        .iter()
        .find(|entry| {
            entry.host == own_host
                && entry.port == own_port
                && entry
                    .metadata
                    .get("gateway_role")
                    .is_some_and(|role| role == "active")
        })
        .cloned()
        .or_else(|| {
            sentinels
                .iter()
                .find(|entry| entry.host == own_host && entry.port == own_port)
                .cloned()
        })
        .or_else(|| sentinels.into_iter().next())
}

fn unix_secs(time: SystemTime) -> f64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_default()
}

fn short_instance_id(instance_id: &str) -> String {
    instance_id.chars().take(8).collect()
}

fn gateway_yield_unavailable_response(
    current_version: &str,
    challenger_version: Option<&str>,
    reason: &str,
) -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "ok": false,
            "success": false,
            "capability": "cooperative_yield",
            "fallback": "polling",
            "current_version": current_version,
            "challenger_version": challenger_version,
            "error": {
                "kind": "optional-capability-unsupported",
                "capability": "cooperative_yield",
                "message": format!(
                    "Cooperative gateway yield is unavailable for this request: {reason}. \
                     This is non-fatal; callers should poll for gateway availability."
                ),
            },
        })),
    )
        .into_response()
}

/// `GET /instances` — return all live instances as JSON.
///
/// Also served under `GET /v1/instances` for consistency with the
/// REST-backed dynamic-capability API (#654).
pub async fn handle_instances(State(gs): State<GatewayState>) -> impl IntoResponse {
    let registry = gs.registry.read().await;
    let instances: Vec<Value> = gs
        .live_instances(&registry)
        .into_iter()
        .map(|entry| gs.instance_json(&entry))
        .collect();
    Json(json!({
        "total": instances.len(),
        "by_source": crate::gateway::state::instance_source_counts(&instances),
        "instances": instances
    }))
}

// ── REST endpoints ────────────────────────────────────────────────────────

/// `GET /v1/healthz` — REST liveness probe compatible with dcc-mcp-skill-rest.
pub async fn handle_v1_healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"ok": true})))
}

/// `GET /v1/readyz` — gateway readiness probe.
pub async fn handle_v1_readyz(State(gs): State<GatewayState>) -> impl IntoResponse {
    let registry = gs.registry.read().await;
    let instances: Vec<Value> = gs
        .live_instances(&registry)
        .into_iter()
        .map(|entry| {
            let row = gs.instance_json(&entry);
            let instance_short = entry.instance_id.simple().to_string()[..8].to_string();
            json!({
                "instance_id": row["instance_id"].clone(),
                "instance_short": instance_short,
                "display_id": row["display_id"].clone(),
                "dcc_type": row["dcc_type"].clone(),
                "status": row["status"].clone(),
                "mcp_url": row["mcp_url"].clone(),
                "readiness": row
                    .get("diagnostics")
                    .and_then(|diag| diag.get("readiness"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "dispatch": row["dispatch"].clone(),
                "gateway": row["gateway"].clone(),
                "lifecycle": row["lifecycle"].clone(),
            })
        })
        .collect();
    let ready_instance_count = instances
        .iter()
        .filter(|instance| readiness_value_is_ready(&instance["readiness"]))
        .count();
    let dispatch_reported_instance_count = instances
        .iter()
        .filter(|instance| dispatch_value_is_reported(&instance["dispatch"]))
        .count();
    let dispatch_ready_instance_count = instances
        .iter()
        .filter(|instance| dispatch_value_is_ready(&instance["dispatch"]))
        .count();
    let gateway_recovery_driver_counts =
        gateway_string_counts(&instances, "gateway", "recovery_driver");
    let registration_refresh_mode_counts =
        gateway_string_counts(&instances, "gateway", "registration_refresh_mode");
    let gateway_daemon_guardian_instance_count = gateway_recovery_driver_counts
        .get("daemon_guardian")
        .copied()
        .unwrap_or(0);
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "checks": [{"name": "gateway", "ok": true}],
            "live_instance_count": instances.len(),
            "ready_instance_count": ready_instance_count,
            "not_ready_instance_count": instances.len().saturating_sub(ready_instance_count),
            "dispatch_reported_instance_count": dispatch_reported_instance_count,
            "dispatch_ready_instance_count": dispatch_ready_instance_count,
            "dispatch_not_ready_instance_count": dispatch_reported_instance_count
                .saturating_sub(dispatch_ready_instance_count),
            "gateway_recovery_driver_counts": gateway_recovery_driver_counts,
            "registration_refresh_mode_counts": registration_refresh_mode_counts,
            "gateway_daemon_guardian_instance_count": gateway_daemon_guardian_instance_count,
            "gateway_daemon_guardian_ready": gateway_daemon_guardian_instance_count > 0,
            "gateway_lifecycle": {
                "persist": std::env::var("DCC_MCP_GATEWAY_PERSIST")
                    .ok()
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false),
                "idle_timeout_secs": std::env::var("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(30),
            },
            "instances": instances,
        })),
    )
}

fn readiness_value_is_ready(readiness: &Value) -> bool {
    readiness.get("process").and_then(Value::as_bool) == Some(true)
        && readiness.get("dcc").and_then(Value::as_bool) == Some(true)
        && readiness.get("skill_catalog").and_then(Value::as_bool) == Some(true)
        && readiness.get("dispatcher").and_then(Value::as_bool) == Some(true)
}

fn dispatch_value_is_reported(dispatch: &Value) -> bool {
    dispatch.get("reported").and_then(Value::as_bool) == Some(true)
}

fn dispatch_value_is_ready(dispatch: &Value) -> bool {
    dispatch.get("ready").and_then(Value::as_bool) == Some(true)
}

fn gateway_string_counts(
    instances: &[Value],
    object_key: &str,
    field_key: &str,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for instance in instances {
        if let Some(value) = instance
            .get(object_key)
            .and_then(|object| object.get(field_key))
            .and_then(Value::as_str)
        {
            *counts.entry(value.to_string()).or_insert(0) += 1;
        }
    }
    counts
}

/// `GET /v1/openapi.json` — gateway REST contract.
pub async fn handle_v1_openapi(State(gs): State<GatewayState>) -> impl IntoResponse {
    let doc = crate::gateway::rest_openapi::build_gateway_openapi_document(&gs.server_version);
    #[cfg(feature = "admin")]
    let doc = {
        let mut doc = doc;
        if gs.debug_routes_enabled {
            super::debug_openapi::add_gateway_debug_openapi_paths(&mut doc);
        }
        doc
    };
    (StatusCode::OK, Json(doc))
}

/// `GET /docs` — gateway REST API reference.
pub async fn handle_v1_docs(State(gs): State<GatewayState>) -> Response {
    let doc = crate::gateway::rest_openapi::build_gateway_openapi_document(&gs.server_version);
    #[cfg(feature = "admin")]
    let doc = {
        let mut doc = doc;
        if gs.debug_routes_enabled {
            super::debug_openapi::add_gateway_debug_openapi_paths(&mut doc);
        }
        doc
    };
    let html = dcc_mcp_skill_rest::openapi::build_docs_html_for_document(doc);
    (StatusCode::OK, Html(html)).into_response()
}

/// `GET /v1/skills` — aggregate gateway capability index as skill entries.
pub async fn handle_v1_skills(State(gs): State<GatewayState>) -> impl IntoResponse {
    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;
    let records = gs.capability_index.snapshot().records;
    let skills: Vec<Value> = records
        .iter()
        .filter(|record| {
            gs.policy
                .enforce_record(
                    dcc_mcp_gateway_core::policy::GatewayPolicyOperation::Search,
                    record,
                )
                .is_ok()
        })
        .map(|record| {
            json!({
                "slug": record.tool_slug,
                "skill": record.skill_name.clone().unwrap_or_else(|| record.backend_tool.clone()),
                "action": &record.backend_tool,
                "dcc": &record.dcc_type,
                "summary": &record.summary,
                "loaded": true,
                "scope": "gateway",
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(json!({"total": skills.len(), "skills": skills})),
    )
}

/// `GET /v1/context` — aggregate gateway context snapshot plus **live
/// instance rows** (same shape as `GET /v1/instances`) so scripts can read
/// `instance_id` / `mcp_url` before calling path-style `/v1/dcc/.../call`
/// without a second HTTP round-trip.
pub async fn handle_v1_context(State(gs): State<GatewayState>) -> impl IntoResponse {
    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;
    let registry = gs.registry.read().await;
    let live_instances = gs.live_instances(&registry);
    let instances: Vec<Value> = live_instances.iter().map(|e| gs.instance_json(e)).collect();
    drop(registry);
    let records = gs.capability_index.snapshot().records;
    let policy_visible_records: Vec<_> = records
        .iter()
        .filter(|record| {
            gs.policy
                .enforce_record(
                    dcc_mcp_gateway_core::policy::GatewayPolicyOperation::Search,
                    record,
                )
                .is_ok()
        })
        .collect();
    let loaded_skill_count = policy_visible_records
        .iter()
        .filter_map(|record| record.skill_name.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .len();
    (
        StatusCode::OK,
        Json(json!({
            "scene": null,
            "version": gs.server_version,
            "dcc": "gateway",
            "display_name": gs.server_name,
            "capabilities": {
                "cooperative_yield": true,
            },
            "documents": [],
            "loaded_skill_count": loaded_skill_count,
            "action_count": policy_visible_records.len(),
            "live_instance_count": instances.len(),
            "instances": instances,
        })),
    )
}

/// `POST /v1/list_skills` — progressive skill listing across live backends.
pub async fn handle_v1_list_skills(
    State(gs): State<GatewayState>,
    Json(body): Json<Value>,
) -> Response {
    let (text, is_error) =
        crate::gateway::aggregator::skill_mgmt_dispatch(&gs, "list_skills", &body).await;
    if is_error {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"success": false, "error": {"kind": "backend-error", "message": text}})),
        )
            .into_response()
    } else {
        match serde_json::from_str::<Value>(&text) {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(_) => (StatusCode::OK, Json(json!({"raw": text}))).into_response(),
        }
    }
}

/// `POST /v1/search` — keyword + filter search over the capability
/// index.
///
/// Request body (every field optional):
///
/// ```json
/// {"query": "sphere", "dcc_type": "maya", "instance_id": "abc12345", "tags": ["geo"],
///  "scene_hint": "rig", "limit": 20}
/// ```
///
/// Response body: `{"total": n, "hits": [SearchHit, ...]}`.
pub async fn handle_v1_search(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let trace_context = TraceContext::from_headers(&headers);
    let agent_context = AgentContext::from_request_parts_with_server_network(
        &headers,
        Some(&body),
        body.get("meta").or_else(|| body.get("_meta")),
    );
    // Refresh-on-demand so the first call after startup or a skill
    // load sees fresh capabilities without waiting for a watcher tick.
    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;
    let mut query = parse_search_payload(&body);
    if query.instance_id.is_none()
        && let Some(raw_instance_id) = body
            .get("instance_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    {
        let registry = gs.registry.read().await;
        match gs.resolve_instance(&registry, Some(raw_instance_id), query.dcc_type.as_deref()) {
            Ok(entry) => {
                query.instance_id = Some(entry.instance_id);
            }
            Err(err) => {
                drop(registry);
                return resolve_instance_negotiated_response(&headers, &body, err);
            }
        }
    }
    let index_generation = index_generation(&gs.capability_index);
    let search_context = SearchResponseContext::new(
        crate::gateway::search_telemetry::SearchTelemetryStore::new_search_id(),
        index_generation,
    );
    let hits = search_service_hits_for_policy(&gs.capability_index, &query, &gs.policy);
    let telemetry_hits = search_hits_for_telemetry(&hits);
    let total = hits.len();
    let hits: Vec<Value> = hits
        .into_iter()
        .map(|hit| search_hit_to_value_with_context(hit, Some(&search_context)))
        .collect();
    let session_id = session_id_from_headers(&headers).or_else(|| {
        agent_context
            .as_ref()
            .and_then(|ctx| ctx.session_id.clone())
    });
    let query_dcc_type = query.dcc_type.clone();
    let query_instance_id = query.instance_id.as_ref().map(ToString::to_string);
    gs.search_telemetry.record_search(SearchTelemetryInput {
        search_id: search_context.search_id.clone(),
        transport: "rest".to_string(),
        kind: "tool".to_string(),
        query: query.query.clone(),
        dcc_type: query_dcc_type.clone(),
        instance_id: query_instance_id.clone(),
        limit: query.limit,
        total,
        ranker_version: search_context.ranker_version.to_string(),
        index_generation: search_context.index_generation.clone(),
        hits: telemetry_hits,
        trace_context: Some(trace_context.clone()),
        session_id: session_id.clone(),
        agent_context: agent_context.clone(),
    });
    AgentWorkflowEvent::new("gateway.search", "rest")
        .with_trace_context(Some(&trace_context))
        .with_agent_context(agent_context.as_ref())
        .with_session_id(session_id.as_deref())
        .with_route(
            None,
            None,
            query_dcc_type.as_deref(),
            query_instance_id.as_deref(),
        )
        .with_search_id(Some(&search_context.search_id))
        .with_ranker_version(Some(search_context.ranker_version))
        .with_search_result(&json!({"total": total, "hits": hits}))
        .with_outcome(true, None)
        .emit();
    let metadata = RestResponseMetadata::from_trace_context(&trace_context)
        .with_index_generation(search_context.index_generation.clone())
        .with_search(
            search_context.search_id.clone(),
            search_context.ranker_version.to_string(),
        );
    search_response_with_metadata(&headers, &body, hits, &metadata)
}

/// `POST /v1/load_skill` — load a skill on a target backend instance
/// using the same routing arguments surfaced by `/v1/search.next_step`.
pub async fn handle_v1_load_skill(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    skill_lifecycle_response(&gs, &headers, "load_skill", body).await
}

/// `POST /v1/unload_skill` — unload a skill on a target backend instance.
pub async fn handle_v1_unload_skill(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    skill_lifecycle_response(&gs, &headers, "unload_skill", body).await
}

async fn skill_lifecycle_response(
    gs: &GatewayState,
    headers: &HeaderMap,
    tool: &str,
    body: Value,
) -> Response {
    let trace_context = TraceContext::from_headers(headers);
    let agent_context = AgentContext::from_request_parts_with_server_network(
        headers,
        Some(&body),
        body.get("meta").or_else(|| body.get("_meta")),
    );
    let search_id = search_id_from_payload(&body);
    let skill_name = skill_name_from_payload(&body);
    let (text, is_error) = crate::gateway::aggregator::skill_mgmt_dispatch(gs, tool, &body).await;
    if tool == "load_skill" {
        record_search_followup(
            gs,
            search_id.as_deref(),
            "load_skill",
            None,
            skill_name.clone(),
            !is_error,
            &trace_context,
        );
        let selected_hit = search_id.as_deref().and_then(|search_id| {
            gs.search_telemetry
                .selected_hit(search_id, None, skill_name.as_deref())
        });
        let parsed = serde_json::from_str::<Value>(&text).ok();
        let error_kind = if is_error {
            error_kind_from_text(&text)
                .or_else(|| Some(classify_skill_lifecycle_error(tool, &text).to_string()))
        } else {
            None
        };
        let policy_reason = parsed.as_ref().and_then(policy_reason_from_value);
        AgentWorkflowEvent::new("gateway.load_skill", "rest")
            .with_trace_context(Some(&trace_context))
            .with_agent_context(agent_context.as_ref())
            .with_search_id(search_id.as_deref())
            .with_ranker_version(Some(crate::gateway::search_telemetry::RANKER_VERSION))
            .with_route(None, skill_name.as_deref(), None, None)
            .with_selected_hit(selected_hit.as_ref())
            .with_outcome(!is_error, error_kind.as_deref())
            .with_policy_reason(policy_reason.as_deref())
            .emit();
    }
    let metadata = RestResponseMetadata::from_trace_context(&trace_context)
        .with_index_generation(index_generation(&gs.capability_index));
    if is_error {
        if let Ok(legacy) = serde_json::from_str::<Value>(&text)
            && let Some(kind) = legacy
                .get("error")
                .and_then(|error| error.get("kind"))
                .and_then(Value::as_str)
        {
            let status = service_error_status(&ServiceError::new(kind, text));
            return negotiated_response_with_metadata(
                headers, &body, status, legacy, None, &metadata, true,
            );
        }
        let legacy = service_error_to_json(&ServiceError::new(
            classify_skill_lifecycle_error(tool, &text),
            text,
        ));
        return negotiated_response_with_metadata(
            headers,
            &body,
            StatusCode::BAD_GATEWAY,
            legacy,
            None,
            &metadata,
            true,
        );
    }
    let mut parsed =
        serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({"message": text}));
    if let Some(obj) = parsed.as_object_mut() {
        obj.entry("index_generation".to_string())
            .or_insert_with(|| json!(metadata.index_generation.as_deref().unwrap_or("")));
    }
    negotiated_response_with_metadata(
        headers,
        &body,
        StatusCode::OK,
        parsed,
        None,
        &metadata,
        true,
    )
}

fn classify_skill_lifecycle_error(tool: &str, text: &str) -> &'static str {
    let lowered = text.to_ascii_lowercase();
    if lowered.contains("multiple-instances-match") || lowered.contains("ambiguous") {
        return "ambiguous-instance";
    }
    if lowered.contains("no-live-instance-match")
        || lowered.contains("unreachable")
        || lowered.contains("booting")
    {
        return "instance-offline";
    }
    if lowered.contains("schema") {
        return "schema-unavailable";
    }
    if lowered.contains("group")
        && (tool.contains("group") || lowered.contains("not found") || lowered.contains("unknown"))
    {
        return "group-not-found";
    }
    if lowered.contains("already loaded") || lowered.contains("already_loaded") {
        return "skill-already-loaded";
    }
    if lowered.contains("not found") || lowered.contains("unknown skill") {
        return "skill-not-found";
    }
    "backend-error"
}

/// `POST /v1/describe` — return the compact record and (optionally)
/// the full backend schema for one capability slug.
///
/// Request body: `{"tool_slug": "<dcc>.<id8>.<tool>"}`.
pub async fn handle_v1_describe(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let trace_context = TraceContext::from_headers(&headers);
    let agent_context = AgentContext::from_request_parts_with_server_network(
        &headers,
        Some(&body),
        body.get("meta").or_else(|| body.get("_meta")),
    );
    let Some(slug) = body.get("tool_slug").and_then(Value::as_str) else {
        let metadata = RestResponseMetadata::from_trace_context(&trace_context)
            .with_index_generation(index_generation(&gs.capability_index));
        return service_error_response_with_metadata(
            &headers,
            &body,
            &ServiceError::new("bad-request", "missing required field: tool_slug"),
            &metadata,
            true,
        );
    };
    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;
    let metadata = RestResponseMetadata::from_trace_context(&trace_context)
        .with_index_generation(index_generation(&gs.capability_index));

    describe_slug_response(
        &gs,
        &headers,
        &body,
        slug,
        &metadata,
        &trace_context,
        agent_context.as_ref(),
    )
    .await
}

/// `GET /v1/tools/{slug}` — path form of describe.
pub async fn handle_v1_describe_path(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Response {
    let trace_context = TraceContext::from_headers(&headers);
    let agent_context = AgentContext::from_request_parts_with_server_network(&headers, None, None);
    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;
    let body = json!({});
    let metadata = RestResponseMetadata::from_trace_context(&trace_context)
        .with_index_generation(index_generation(&gs.capability_index));
    describe_slug_response(
        &gs,
        &headers,
        &body,
        &slug,
        &metadata,
        &trace_context,
        agent_context.as_ref(),
    )
    .await
}

async fn describe_slug_response(
    gs: &GatewayState,
    headers: &HeaderMap,
    body: &Value,
    slug: &str,
    metadata: &RestResponseMetadata,
    trace_context: &TraceContext,
    agent_context: Option<&AgentContext>,
) -> Response {
    let search_id = search_id_from_payload(body);
    match describe_tool_full(gs, slug).await {
        Ok((record, tool)) => {
            let dcc_type = record.dcc_type.clone();
            let instance_id = record.instance_id.to_string();
            let skill_name = record.skill_name.clone();
            record_search_followup(
                gs,
                search_id.as_deref(),
                "describe",
                Some(slug),
                None,
                true,
                &TraceContext {
                    trace_id: metadata.trace_id.clone(),
                    request_id: metadata.request_id.clone(),
                    span_id: None,
                    parent_span_id: None,
                    parent_request_id: None,
                    trace_flags: None,
                    trace_state: None,
                },
            );
            let selected_hit = search_id.as_deref().and_then(|search_id| {
                gs.search_telemetry
                    .selected_hit(search_id, Some(slug), None)
            });
            AgentWorkflowEvent::new("gateway.describe", "rest")
                .with_trace_context(Some(trace_context))
                .with_agent_context(agent_context)
                .with_search_id(search_id.as_deref())
                .with_ranker_version(Some(crate::gateway::search_telemetry::RANKER_VERSION))
                .with_route(
                    Some(slug),
                    skill_name.as_deref(),
                    Some(dcc_type.as_str()),
                    Some(instance_id.as_str()),
                )
                .with_selected_hit(selected_hit.as_ref())
                .with_outcome(true, None)
                .emit();
            let mut legacy = json!({
                "record": record,
                "tool": tool,
            });
            if let Some(search_id) = search_id.as_deref() {
                legacy["next_step"] = call_next_step(slug, search_id, metadata);
            }
            let compact = compact_describe_payload(&legacy);
            negotiated_response_with_metadata(
                headers,
                body,
                StatusCode::OK,
                legacy,
                Some(compact),
                metadata,
                true,
            )
        }
        Err(err) => {
            let err_payload = crate::gateway::capability_service::service_error_to_json(&err);
            let policy_reason = policy_reason_from_value(&err_payload);
            record_search_followup(
                gs,
                search_id.as_deref(),
                "describe",
                Some(slug),
                None,
                false,
                &TraceContext {
                    trace_id: metadata.trace_id.clone(),
                    request_id: metadata.request_id.clone(),
                    span_id: None,
                    parent_span_id: None,
                    parent_request_id: None,
                    trace_flags: None,
                    trace_state: None,
                },
            );
            let selected_hit = search_id.as_deref().and_then(|search_id| {
                gs.search_telemetry
                    .selected_hit(search_id, Some(slug), None)
            });
            AgentWorkflowEvent::new("gateway.describe", "rest")
                .with_trace_context(Some(trace_context))
                .with_agent_context(agent_context)
                .with_search_id(search_id.as_deref())
                .with_ranker_version(Some(crate::gateway::search_telemetry::RANKER_VERSION))
                .with_route(Some(slug), None, None, None)
                .with_selected_hit(selected_hit.as_ref())
                .with_outcome(false, Some(err.kind.as_str()))
                .with_policy_reason(policy_reason.as_deref())
                .emit();
            service_error_response_with_metadata(headers, body, &err, metadata, true)
        }
    }
}

/// `GET /v1/dcc/{dcc_type}/instances/{instance_id}/describe?backend_tool=...` —
/// describe one capability without assembling a dotted `tool_slug`.
///
/// Query: **`backend_tool`** (required) — backend action name. Aliases: **`tool`**, **`action`**.
///
/// Response matches [`handle_v1_describe_path`] (`GET /v1/tools/{slug}`).
pub async fn handle_v1_dcc_instance_describe(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Path((dcc_type, instance_id)): Path<(String, String)>,
    Query(q): Query<DccInstanceDescribeQuery>,
) -> Response {
    let body = json!({});
    let trace_context = TraceContext::from_headers(&headers);
    let agent_context = AgentContext::from_request_parts_with_server_network(&headers, None, None);
    let backend_tool = q.backend_tool.trim();
    if backend_tool.is_empty() {
        let metadata = RestResponseMetadata::from_headers(&headers)
            .with_index_generation(index_generation(&gs.capability_index));
        return service_error_response_with_metadata(
            &headers,
            &body,
            &ServiceError::new(
                "bad-request",
                "missing or empty required query parameter: backend_tool (aliases: tool, action)",
            ),
            &metadata,
            true,
        );
    }

    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;
    let metadata = RestResponseMetadata::from_trace_context(&trace_context)
        .with_index_generation(index_generation(&gs.capability_index));

    let registry = gs.registry.read().await;
    let entry = match gs.resolve_instance(
        &registry,
        Some(instance_id.as_str()),
        Some(dcc_type.as_str()),
    ) {
        Ok(e) => e,
        Err(err) => {
            drop(registry);
            return resolve_instance_negotiated_response(&headers, &body, err);
        }
    };
    drop(registry);

    if !entry.dcc_type.eq_ignore_ascii_case(dcc_type.as_str()) {
        return service_error_response_with_metadata(
            &headers,
            &body,
            &ServiceError::new(
                "bad-request",
                "path dcc_type does not match resolved registry row",
            ),
            &metadata,
            true,
        );
    }

    let slug = tool_slug(&entry.dcc_type, &entry.instance_id, backend_tool);
    describe_slug_response(
        &gs,
        &headers,
        &body,
        &slug,
        &metadata,
        &trace_context,
        agent_context.as_ref(),
    )
    .await
}

/// `POST /v1/call` — invoke a backend action by slug, or an ordered batch
/// via `calls[]`.
///
/// **Single call** (backward compatible):
/// `{"tool_slug": "...", "arguments": {...}, "meta": {...}}` (meta optional).
///
/// **Batch call** (same semantics as `POST /v1/call_batch`):
/// `{"calls": [{ "tool_slug", "arguments"?, "meta"?, "id"? }, ...],
///   "stop_on_error"?: bool, "meta"?: {...}}`.
/// When `calls` is present `tool_slug` at the top level is ignored.
pub async fn handle_v1_call(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let trace_context = TraceContext::from_headers(&headers);

    // Batch path: when `calls` array is present, delegate to batch infrastructure.
    if body.get("calls").and_then(Value::as_array).is_some() {
        return match call_batch_with_admin_trace(&gs, &headers, &body, trace_context.clone()).await
        {
            Ok(value) => {
                let compact = compact_call_batch_payload(&value);
                let metadata = RestResponseMetadata::from_trace_context(&trace_context)
                    .with_index_generation(index_generation(&gs.capability_index));
                negotiated_response_with_metadata(
                    &headers,
                    &body,
                    StatusCode::OK,
                    value,
                    Some(compact),
                    &metadata,
                    true,
                )
            }
            Err(err) => {
                let mut legacy = service_error_to_json(&err);
                if let Some(obj) = legacy.as_object_mut() {
                    obj.insert("success".to_string(), json!(false));
                }
                let metadata = RestResponseMetadata::from_trace_context(&trace_context)
                    .with_index_generation(index_generation(&gs.capability_index));
                negotiated_response_with_metadata(
                    &headers,
                    &body,
                    service_error_status(&err),
                    legacy,
                    None,
                    &metadata,
                    true,
                )
            }
        };
    }

    // Single-call path (backward compatible).
    let Some(slug) = body.get("tool_slug").and_then(Value::as_str) else {
        let metadata = RestResponseMetadata::from_trace_context(&trace_context)
            .with_index_generation(index_generation(&gs.capability_index));
        return service_error_response_with_metadata(
            &headers,
            &body,
            &ServiceError::new("bad-request", "missing required field: tool_slug or calls"),
            &metadata,
            false,
        );
    };
    let arguments =
        match dcc_mcp_jsonrpc::coerce_tool_arguments_object(body.get("arguments").cloned()) {
            Ok(v) => v,
            Err(msg) => {
                let metadata = RestResponseMetadata::from_trace_context(&trace_context)
                    .with_index_generation(index_generation(&gs.capability_index));
                return service_error_response_with_metadata(
                    &headers,
                    &body,
                    &ServiceError::new("bad-request", msg),
                    &metadata,
                    false,
                );
            }
        };
    let meta = body.get("meta").cloned();

    match call_service_with_admin_trace(
        &gs,
        &headers,
        RestCallTraceRequest {
            method: "v1/call",
            slug,
            arguments,
            meta,
            request_body: &body,
            trace_context: trace_context.clone(),
        },
    )
    .await
    {
        Ok(result) => {
            let metadata = RestResponseMetadata::from_trace_context(&trace_context)
                .with_index_generation(index_generation(&gs.capability_index));
            negotiated_response_with_metadata(
                &headers,
                &body,
                StatusCode::OK,
                result,
                None,
                &metadata,
                false,
            )
        }
        Err(err) => {
            let metadata = RestResponseMetadata::from_trace_context(&trace_context)
                .with_index_generation(index_generation(&gs.capability_index));
            service_error_response_with_metadata(&headers, &body, &err, &metadata, false)
        }
    }
}

/// `POST /v1/dcc/{dcc_type}/instances/{instance_id}/call` — invoke one backend
/// tool without assembling a dotted `tool_slug`.
///
/// Path: `dcc_type` must match the registry row; `instance_id` is a full UUID
/// or a unique ≥4-character hex prefix (same rules as MCP routing).
///
/// JSON body: `{ "backend_tool": "<name>", "arguments"?: {...}, "meta"?: {...} }`.
/// Accepts `tool` or `action` as an alias for `backend_tool`.
///
/// Semantics match [`handle_v1_call`] after composing
/// `tool_slug(dcc, instance_uuid, backend_tool)`.
pub async fn handle_v1_dcc_instance_call(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Path((dcc_type, instance_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let trace_context = TraceContext::from_headers(&headers);
    let backend_tool = body
        .get("backend_tool")
        .or_else(|| body.get("tool"))
        .or_else(|| body.get("action"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(backend_tool) = backend_tool else {
        let metadata = RestResponseMetadata::from_trace_context(&trace_context)
            .with_index_generation(index_generation(&gs.capability_index));
        return service_error_response_with_metadata(
            &headers,
            &body,
            &ServiceError::new(
                "bad-request",
                "missing required field: backend_tool (accepted aliases: tool, action)",
            ),
            &metadata,
            false,
        );
    };

    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;
    let metadata = RestResponseMetadata::from_trace_context(&trace_context)
        .with_index_generation(index_generation(&gs.capability_index));

    let registry = gs.registry.read().await;
    let entry = match gs.resolve_instance(
        &registry,
        Some(instance_id.as_str()),
        Some(dcc_type.as_str()),
    ) {
        Ok(e) => e,
        Err(err) => {
            drop(registry);
            return resolve_instance_negotiated_response(&headers, &body, err);
        }
    };
    drop(registry);

    if !entry.dcc_type.eq_ignore_ascii_case(dcc_type.as_str()) {
        return service_error_response_with_metadata(
            &headers,
            &body,
            &ServiceError::new(
                "bad-request",
                "path dcc_type does not match resolved registry row",
            ),
            &metadata,
            false,
        );
    }

    let slug = tool_slug(&entry.dcc_type, &entry.instance_id, backend_tool);
    let arguments =
        match dcc_mcp_jsonrpc::coerce_tool_arguments_object(body.get("arguments").cloned()) {
            Ok(v) => v,
            Err(msg) => {
                return service_error_response_with_metadata(
                    &headers,
                    &body,
                    &ServiceError::new("bad-request", msg),
                    &metadata,
                    false,
                );
            }
        };
    let meta = body.get("meta").cloned();

    match call_service_with_admin_trace(
        &gs,
        &headers,
        RestCallTraceRequest {
            method: "v1/dcc/instances/call",
            slug: &slug,
            arguments,
            meta,
            request_body: &body,
            trace_context: trace_context.clone(),
        },
    )
    .await
    {
        Ok(result) => negotiated_response_with_metadata(
            &headers,
            &body,
            StatusCode::OK,
            result,
            None,
            &metadata,
            false,
        ),
        Err(err) => service_error_response_with_metadata(&headers, &body, &err, &metadata, false),
    }
}

pub(crate) fn resolve_instance_http_response(err: ResolveInstanceError) -> impl IntoResponse {
    let (status, body) = resolve_instance_error_parts(&err);
    (status, Json(body))
}

fn resolve_instance_negotiated_response(
    headers: &HeaderMap,
    body: &Value,
    err: ResolveInstanceError,
) -> Response {
    let (status, legacy) = resolve_instance_error_parts(&err);
    let metadata = RestResponseMetadata::from_headers(headers);
    negotiated_response_with_metadata(headers, body, status, legacy, None, &metadata, false)
}

fn resolve_instance_error_parts(err: &ResolveInstanceError) -> (StatusCode, Value) {
    let refresh_hint = " After a DCC crash or reconnect the instance UUID usually changes — call \
        GET /v1/instances (or resources/read gateway://instances), then search_tools / POST /v1/search \
        again; do not reuse old tool_slug strings.";
    match err {
        ResolveInstanceError::PrefixTooShort { .. } => (
            StatusCode::BAD_REQUEST,
            service_error_to_json(&ServiceError::new("bad-request", err.to_string())),
        ),
        ResolveInstanceError::NoMatch { .. } => (
            StatusCode::NOT_FOUND,
            service_error_to_json(
                &ServiceError::new("instance-offline", format!("{err}.{refresh_hint}"))
                    .with_instance_provenance("never-registered", None),
            ),
        ),
        ResolveInstanceError::MultipleMatches { .. } => (
            StatusCode::CONFLICT,
            service_error_to_json(&ServiceError::new("ambiguous", err.to_string())),
        ),
    }
}

/// `POST /v1/call_batch` — invoke multiple backend actions in order.
///
/// Request body: `{ "calls": [ { "tool_slug", "arguments"?, "meta"? }, ... ],
/// "stop_on_error"?: bool }` — same semantics as MCP `call_tools`.
pub async fn handle_v1_call_batch(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let trace_context = TraceContext::from_headers(&headers);
    match call_batch_with_admin_trace(&gs, &headers, &body, trace_context.clone()).await {
        Ok(value) => {
            let compact = compact_call_batch_payload(&value);
            let metadata = RestResponseMetadata::from_trace_context(&trace_context)
                .with_index_generation(index_generation(&gs.capability_index));
            negotiated_response_with_metadata(
                &headers,
                &body,
                StatusCode::OK,
                value,
                Some(compact),
                &metadata,
                true,
            )
        }
        Err(err) => {
            let mut legacy = service_error_to_json(&err);
            if let Some(obj) = legacy.as_object_mut() {
                obj.insert("success".to_string(), json!(false));
            }
            let metadata = RestResponseMetadata::from_trace_context(&trace_context)
                .with_index_generation(index_generation(&gs.capability_index));
            negotiated_response_with_metadata(
                &headers,
                &body,
                service_error_status(&err),
                legacy,
                None,
                &metadata,
                true,
            )
        }
    }
}

#[cfg(test)]
#[path = "rest_impl_tests.rs"]
mod rest_impl_tests;
