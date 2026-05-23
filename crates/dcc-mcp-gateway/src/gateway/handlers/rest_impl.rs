use super::*;

use crate::gateway::capability::{RefreshReason, parse_slug, tool_slug};
use crate::gateway::capability_service::{
    ServiceError, call_service, describe_tool_full, parse_search_payload,
    refresh_all_live_backends, search_service_rows, service_error_to_json,
};

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
        tracing::info!(
            challenger = %challenger_version,
            current = %gs.server_version,
            "Gateway yield requested — initiating graceful handoff"
        );
        let _ = gs.yield_tx.send(true);
        Json(json!({
            "ok": true,
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
    Json(json!({ "total": instances.len(), "instances": instances }))
}

// ── REST endpoints ────────────────────────────────────────────────────────

/// `GET /v1/healthz` — REST liveness probe compatible with dcc-mcp-skill-rest.
pub async fn handle_v1_healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"ok": true})))
}

/// `GET /v1/readyz` — gateway readiness probe.
pub async fn handle_v1_readyz(State(gs): State<GatewayState>) -> impl IntoResponse {
    let registry = gs.registry.read().await;
    let live_instances = gs.live_instances(&registry).len();
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "checks": [{"name": "gateway", "ok": true}],
            "live_instance_count": live_instances,
        })),
    )
}

/// `GET /v1/openapi.json` — gateway REST contract.
pub async fn handle_v1_openapi(State(gs): State<GatewayState>) -> impl IntoResponse {
    let doc =
        dcc_mcp_skill_rest::openapi::build_openapi_document("dcc-mcp-gateway", &gs.server_version);
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
    let doc =
        dcc_mcp_skill_rest::openapi::build_openapi_document("dcc-mcp-gateway", &gs.server_version);
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
    let loaded_skill_count = records
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
            "action_count": records.len(),
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
/// {"query": "sphere", "dcc_type": "maya", "tags": ["geo"],
///  "scene_hint": "rig", "limit": 20}
/// ```
///
/// Response body: `{"total": n, "hits": [SearchHit, ...]}`.
pub async fn handle_v1_search(
    State(gs): State<GatewayState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // Refresh-on-demand so the first call after startup or a skill
    // load sees fresh capabilities without waiting for a watcher tick.
    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;
    let query = parse_search_payload(&body);
    let hits = search_service_rows(&gs.capability_index, &query);
    (
        StatusCode::OK,
        Json(json!({
            "total": hits.len(),
            "hits": hits,
        })),
    )
}

/// `POST /v1/load_skill` — load a skill on a target backend instance
/// using the same routing arguments surfaced by `/v1/search.next_step`.
pub async fn handle_v1_load_skill(
    State(gs): State<GatewayState>,
    Json(body): Json<Value>,
) -> Response {
    skill_lifecycle_response(&gs, "load_skill", body).await
}

/// `POST /v1/unload_skill` — unload a skill on a target backend instance.
pub async fn handle_v1_unload_skill(
    State(gs): State<GatewayState>,
    Json(body): Json<Value>,
) -> Response {
    skill_lifecycle_response(&gs, "unload_skill", body).await
}

async fn skill_lifecycle_response(gs: &GatewayState, tool: &str, body: Value) -> Response {
    let (text, is_error) = crate::gateway::aggregator::skill_mgmt_dispatch(gs, tool, &body).await;
    if is_error {
        return (
            StatusCode::BAD_GATEWAY,
            Json(service_error_to_json(&ServiceError::new(
                "backend-error",
                text,
            ))),
        )
            .into_response();
    }
    let parsed = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({"message": text}));
    (StatusCode::OK, Json(parsed)).into_response()
}

/// `POST /v1/describe` — return the compact record and (optionally)
/// the full backend schema for one capability slug.
///
/// Request body: `{"tool_slug": "<dcc>.<id8>.<tool>"}`.
pub async fn handle_v1_describe(
    State(gs): State<GatewayState>,
    Json(body): Json<Value>,
) -> Response {
    let Some(slug) = body.get("tool_slug").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(service_error_to_json(&ServiceError::new(
                "bad-request",
                "missing required field: tool_slug",
            ))),
        )
            .into_response();
    };
    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;

    describe_slug_response(&gs, slug).await
}

/// `GET /v1/tools/{slug}` — path form of describe.
pub async fn handle_v1_describe_path(
    State(gs): State<GatewayState>,
    Path(slug): Path<String>,
) -> Response {
    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;
    describe_slug_response(&gs, &slug).await
}

async fn describe_slug_response(gs: &GatewayState, slug: &str) -> Response {
    match describe_tool_full(gs, slug).await {
        Ok((record, tool)) => (
            StatusCode::OK,
            Json(json!({
                "record": record,
                "tool": tool,
            })),
        )
            .into_response(),
        Err(err) => error_response(&err).into_response(),
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
    Path((dcc_type, instance_id)): Path<(String, String)>,
    Query(q): Query<DccInstanceDescribeQuery>,
) -> Response {
    let backend_tool = q.backend_tool.trim();
    if backend_tool.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(service_error_to_json(&ServiceError::new(
                "bad-request",
                "missing or empty required query parameter: backend_tool (aliases: tool, action)",
            ))),
        )
            .into_response();
    }

    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;

    let registry = gs.registry.read().await;
    let entry = match gs.resolve_instance(
        &registry,
        Some(instance_id.as_str()),
        Some(dcc_type.as_str()),
    ) {
        Ok(e) => e,
        Err(err) => {
            drop(registry);
            return resolve_instance_http_response(err).into_response();
        }
    };
    drop(registry);

    if !entry.dcc_type.eq_ignore_ascii_case(dcc_type.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(service_error_to_json(&ServiceError::new(
                "bad-request",
                "path dcc_type does not match resolved registry row",
            ))),
        )
            .into_response();
    }

    let slug = tool_slug(&entry.dcc_type, &entry.instance_id, backend_tool);
    describe_slug_response(&gs, &slug).await
}

/// `POST /v1/call` — invoke a backend action by slug.
///
/// Request body: `{"tool_slug": "...", "arguments": {...},
///                 "meta": {...}}` (meta optional).
pub async fn handle_v1_call(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let Some(slug) = body.get("tool_slug").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(service_error_to_json(&ServiceError::new(
                "bad-request",
                "missing required field: tool_slug",
            ))),
        )
            .into_response();
    };
    let arguments =
        match dcc_mcp_jsonrpc::coerce_tool_arguments_object(body.get("arguments").cloned()) {
            Ok(v) => v,
            Err(msg) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(service_error_to_json(&ServiceError::new(
                        "bad-request",
                        msg,
                    ))),
                )
                    .into_response();
            }
        };
    let meta = body.get("meta").cloned();

    match call_service_with_admin_trace(&gs, &headers, "v1/call", slug, arguments, meta, &body)
        .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(err) => error_response(&err).into_response(),
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
    let backend_tool = body
        .get("backend_tool")
        .or_else(|| body.get("tool"))
        .or_else(|| body.get("action"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(backend_tool) = backend_tool else {
        return (
            StatusCode::BAD_REQUEST,
            Json(service_error_to_json(&ServiceError::new(
                "bad-request",
                "missing required field: backend_tool (accepted aliases: tool, action)",
            ))),
        )
            .into_response();
    };

    refresh_all_live_backends(&gs, RefreshReason::Periodic).await;

    let registry = gs.registry.read().await;
    let entry = match gs.resolve_instance(
        &registry,
        Some(instance_id.as_str()),
        Some(dcc_type.as_str()),
    ) {
        Ok(e) => e,
        Err(err) => {
            drop(registry);
            return resolve_instance_http_response(err).into_response();
        }
    };
    drop(registry);

    if !entry.dcc_type.eq_ignore_ascii_case(dcc_type.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(service_error_to_json(&ServiceError::new(
                "bad-request",
                "path dcc_type does not match resolved registry row",
            ))),
        )
            .into_response();
    }

    let slug = tool_slug(&entry.dcc_type, &entry.instance_id, backend_tool);
    let arguments =
        match dcc_mcp_jsonrpc::coerce_tool_arguments_object(body.get("arguments").cloned()) {
            Ok(v) => v,
            Err(msg) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(service_error_to_json(&ServiceError::new(
                        "bad-request",
                        msg,
                    ))),
                )
                    .into_response();
            }
        };
    let meta = body.get("meta").cloned();

    match call_service_with_admin_trace(
        &gs,
        &headers,
        "v1/dcc/instances/call",
        &slug,
        arguments,
        meta,
        &body,
    )
    .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(err) => error_response(&err).into_response(),
    }
}

fn resolve_instance_http_response(err: ResolveInstanceError) -> impl IntoResponse {
    let refresh_hint = " After a DCC crash or reconnect the instance UUID usually changes — call \
        GET /v1/instances (or resources/read gateway://instances), then search_tools / POST /v1/search \
        again; do not reuse old tool_slug strings.";
    match &err {
        ResolveInstanceError::PrefixTooShort { .. } => (
            StatusCode::BAD_REQUEST,
            Json(service_error_to_json(&ServiceError::new(
                "bad-request",
                err.to_string(),
            ))),
        ),
        ResolveInstanceError::NoMatch { .. } => (
            StatusCode::NOT_FOUND,
            Json(service_error_to_json(
                &ServiceError::new("instance-offline", format!("{err}.{refresh_hint}"))
                    .with_instance_provenance("never-registered", None),
            )),
        ),
        ResolveInstanceError::MultipleMatches { .. } => (
            StatusCode::CONFLICT,
            Json(service_error_to_json(&ServiceError::new(
                "ambiguous",
                err.to_string(),
            ))),
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
    match call_batch_with_admin_trace(&gs, &headers, &body).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(message) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": {"kind": "bad-request", "message": message},
            })),
        )
            .into_response(),
    }
}

async fn call_service_with_admin_trace(
    gs: &GatewayState,
    headers: &HeaderMap,
    method: &str,
    slug: &str,
    arguments: Value,
    meta: Option<Value>,
    request_body: &Value,
) -> Result<Value, ServiceError> {
    use crate::gateway::admin::trace::{
        AgentContext, MAX_INPUT_BYTES, MAX_OUTPUT_BYTES, TraceContext, TracePayload,
    };
    use crate::gateway::middleware::{CallContext, CallResult};

    let trace_context = TraceContext::from_headers(headers);
    let mut ctx = CallContext::new(method, trace_context.request_id.clone(), arguments.clone())
        .with_tool_slug(slug)
        .with_transport("rest")
        .with_agent_context(AgentContext::from_request_parts(
            headers,
            Some(request_body),
            meta.as_ref(),
        ))
        .with_trace_context(trace_context);
    if let Some(session_id) = session_id_from_headers(headers) {
        ctx = ctx.with_session_id(session_id);
    }
    if let Some((dcc_type, instance_hint, _)) = parse_slug(slug) {
        ctx = ctx.with_backend(dcc_type, instance_hint);
    }
    if !gs.middleware_chain.is_empty()
        && let Err(err) = gs.middleware_chain.run_before(&mut ctx).await
    {
        return Err(ServiceError::new("middleware-error", err.to_string()));
    }

    ctx.input_payload = Some(TracePayload::from_value(&ctx.args, MAX_INPUT_BYTES));

    let effective_arguments = if gs.middleware_chain.is_empty() {
        arguments
    } else {
        ctx.args.clone()
    };
    emit_rest_traffic_frame(
        gs,
        &ctx,
        headers,
        RestTrafficFrame {
            path: method,
            direction: "inbound",
            leg: "client_to_gateway",
            status: None,
            body: json!({
            "tool_slug": slug,
            "arguments": effective_arguments.clone(),
            "meta": meta.clone(),
            }),
        },
    );
    let dispatch_ns = now_ns();
    let mut result = call_service(
        gs,
        slug,
        effective_arguments.clone(),
        meta.clone(),
        Some(&ctx.trace_context),
    )
    .await;
    if matches!(&result, Err(err) if err.kind == "unknown-slug") {
        refresh_all_live_backends(gs, RefreshReason::Periodic).await;
        result = call_service(
            gs,
            slug,
            effective_arguments,
            meta,
            Some(&ctx.trace_context),
        )
        .await;
    }

    let response_ns = now_ns();
    let (preview_text, is_error, output_value) = match &result {
        Ok(value) => (
            serde_json::to_string(value).unwrap_or_default(),
            false,
            value.clone(),
        ),
        Err(err) => (err.message.clone(), true, service_error_to_json(err)),
    };
    let mut span = ctx
        .trace_context
        .child_span(
            "backend.execute",
            dispatch_ns,
            response_ns.saturating_sub(dispatch_ns),
        )
        .with_attr("tool_slug", slug)
        .with_attr("transport", "rest")
        .with_attr("ok", !is_error);
    if is_error {
        span = span.with_error();
    }
    ctx.push_span(span);
    ctx.output_payload = Some(TracePayload::from_value(&output_value, MAX_OUTPUT_BYTES));

    let mut call_result = CallResult::from_tuple(preview_text, is_error);
    if !gs.middleware_chain.is_empty()
        && let Err(err) = gs.middleware_chain.run_after(&ctx, &mut call_result).await
    {
        return Err(ServiceError::new("middleware-error", err.to_string()));
    }

    emit_rest_traffic_frame(
        gs,
        &ctx,
        headers,
        RestTrafficFrame {
            path: method,
            direction: "outbound",
            leg: "gateway_to_client",
            status: Some(if is_error { 502 } else { 200 }),
            body: output_value,
        },
    );

    result
}

async fn call_batch_with_admin_trace(
    gs: &GatewayState,
    headers: &HeaderMap,
    request_body: &Value,
) -> Result<Value, String> {
    use crate::gateway::admin::trace::{
        AgentContext, MAX_INPUT_BYTES, MAX_OUTPUT_BYTES, TraceContext, TracePayload,
    };
    use crate::gateway::middleware::{CallContext, CallResult};

    let trace_context = TraceContext::from_headers(headers);
    let mut ctx = CallContext::new(
        "v1/call_batch",
        trace_context.request_id.clone(),
        request_body.clone(),
    )
    .with_tool_slug("call_batch")
    .with_transport("rest")
    .with_agent_context(AgentContext::from_request_parts(
        headers,
        Some(request_body),
        request_body.get("meta"),
    ))
    .with_trace_context(trace_context);
    if let Some(session_id) = session_id_from_headers(headers) {
        ctx = ctx.with_session_id(session_id);
    }
    if !gs.middleware_chain.is_empty()
        && let Err(err) = gs.middleware_chain.run_before(&mut ctx).await
    {
        return Err(err.to_string());
    }

    ctx.input_payload = Some(TracePayload::from_value(&ctx.args, MAX_INPUT_BYTES));
    emit_rest_traffic_frame(
        gs,
        &ctx,
        headers,
        RestTrafficFrame {
            path: "v1/call_batch",
            direction: "inbound",
            leg: "client_to_gateway",
            status: None,
            body: ctx.args.clone(),
        },
    );

    let dispatch_ns = now_ns();
    let result = crate::gateway::tools::gateway_call_batch_inner(
        gs,
        &ctx.args,
        None,
        Some(&ctx.trace_context),
    )
    .await;
    let response_ns = now_ns();
    let (preview_text, is_error, output_value) = match &result {
        Ok(value) => (
            serde_json::to_string(value).unwrap_or_default(),
            false,
            value.clone(),
        ),
        Err(message) => (
            message.clone(),
            true,
            json!({
                "success": false,
                "error": {"kind": "bad-request", "message": message},
            }),
        ),
    };
    let mut span = ctx
        .trace_context
        .child_span(
            "batch.execute",
            dispatch_ns,
            response_ns.saturating_sub(dispatch_ns),
        )
        .with_attr("tool_slug", "call_batch")
        .with_attr("transport", "rest")
        .with_attr("ok", !is_error);
    if is_error {
        span = span.with_error();
    }
    ctx.push_span(span);
    ctx.output_payload = Some(TracePayload::from_value(&output_value, MAX_OUTPUT_BYTES));

    let mut call_result = CallResult::from_tuple(preview_text, is_error);
    if !gs.middleware_chain.is_empty()
        && let Err(err) = gs.middleware_chain.run_after(&ctx, &mut call_result).await
    {
        return Err(err.to_string());
    }

    emit_rest_traffic_frame(
        gs,
        &ctx,
        headers,
        RestTrafficFrame {
            path: "v1/call_batch",
            direction: "outbound",
            leg: "gateway_to_client",
            status: Some(if is_error { 400 } else { 200 }),
            body: output_value,
        },
    );

    result
}

struct RestTrafficFrame<'a> {
    path: &'a str,
    direction: &'static str,
    leg: &'static str,
    status: Option<u16>,
    body: Value,
}

fn emit_rest_traffic_frame(
    gs: &GatewayState,
    ctx: &crate::gateway::middleware::CallContext,
    headers: &HeaderMap,
    frame: RestTrafficFrame<'_>,
) {
    gs.traffic_capture.emit_json_frame(
        crate::gateway::traffic::TrafficFrame::json(
            crate::gateway::traffic::gateway_source(
                &gs.server_name,
                &gs.server_version,
                &gs.own_host,
                gs.own_port,
            ),
            crate::gateway::traffic::correlation(
                Some(&ctx.trace_context.request_id),
                Some(&ctx.trace_context.trace_id),
                ctx.session_id.as_deref(),
            ),
            frame.direction,
            frame.leg,
            "http",
            frame.body,
        )
        .with_session_id(ctx.session_id.as_deref())
        .with_http(crate::gateway::traffic::http_post(
            frame.path,
            Some(headers),
            frame.status,
        ))
        .with_mcp(crate::gateway::traffic::mcp_message(
            if frame.direction == "inbound" {
                "request"
            } else {
                "response"
            },
            "tools/call",
            None,
        )),
    );
}

fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    header_string(headers, "mcp-session-id")
        .or_else(|| header_string(headers, "x-session-id"))
        .or_else(|| header_string(headers, "x-dcc-mcp-session-id"))
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn error_response(err: &ServiceError) -> (StatusCode, Json<Value>) {
    let status = match err.kind.as_str() {
        "unknown-slug" => StatusCode::NOT_FOUND,
        "ambiguous" => StatusCode::CONFLICT,
        "instance-offline" => StatusCode::SERVICE_UNAVAILABLE,
        "host-died" => StatusCode::BAD_GATEWAY,
        "backend-error" | "schema-unavailable" => StatusCode::BAD_GATEWAY,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, Json(service_error_to_json(err)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::{RwLock, broadcast, watch};

    #[derive(Default)]
    struct CaptureSink(Mutex<Vec<crate::gateway::middleware::AuditEntry>>);

    impl crate::gateway::middleware::AuditSink for CaptureSink {
        fn record(&self, entry: crate::gateway::middleware::AuditEntry) {
            self.0.lock().unwrap().push(entry);
        }
    }

    struct ReplaceArgs(serde_json::Value);

    impl crate::gateway::middleware::BeforeCallMiddleware for ReplaceArgs {
        fn before_call<'a>(
            &'a self,
            ctx: &'a mut crate::gateway::middleware::CallContext,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<(), crate::gateway::middleware::MiddlewareError>,
                    > + Send
                    + 'a,
            >,
        > {
            ctx.args = self.0.clone();
            Box::pin(async move { Ok::<(), crate::gateway::middleware::MiddlewareError>(()) })
        }
    }

    fn test_gateway_state(server_version: &str) -> GatewayState {
        test_gateway_state_with_debug_routes(server_version, false)
    }

    fn test_gateway_state_with_debug_routes(
        server_version: &str,
        debug_routes_enabled: bool,
    ) -> GatewayState {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
        let (yield_tx, _) = watch::channel(false);
        let (events_tx, _) = broadcast::channel::<String>(8);
        GatewayState {
            registry,
            stale_timeout: Duration::from_secs(30),
            backend_timeout: Duration::from_secs(10),
            async_dispatch_timeout: Duration::from_secs(60),
            wait_terminal_timeout: Duration::from_secs(600),
            server_name: "test".into(),
            server_version: server_version.into(),
            own_host: "127.0.0.1".into(),
            own_port: 9765,
            http_client: reqwest::Client::new(),
            yield_tx: Arc::new(yield_tx),
            events_tx: Arc::new(events_tx),
            protocol_version: Arc::new(RwLock::new(None)),
            resource_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            pending_calls: Arc::new(RwLock::new(HashMap::new())),
            subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
            allow_unknown_tools: false,
            adapter_version: None,
            adapter_dcc: None,
            capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
            event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
            middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
            instance_diagnostics: Arc::new(
                crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
            ),
            traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
            debug_routes_enabled,
            #[cfg(feature = "prometheus")]
            gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        }
    }

    async fn response_json(resp: Response) -> (StatusCode, Value) {
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    async fn response_text(resp: Response) -> (StatusCode, String) {
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    #[tokio::test]
    async fn gateway_docs_serves_scalar_openapi_ui() {
        let (status, body) =
            response_text(handle_v1_docs(State(test_gateway_state("1.2.3"))).await).await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("scalar") || body.contains("Scalar"));
        assert!(body.contains("dcc-mcp-gateway"));
        assert!(body.contains("/v1/search"));
        assert!(!body.contains("/v1/debug/instances"));
    }

    #[tokio::test]
    #[cfg(feature = "admin")]
    async fn gateway_docs_lists_debug_routes_when_runtime_debug_routes_enabled() {
        let (status, body) = response_text(
            handle_v1_docs(State(test_gateway_state_with_debug_routes("1.2.3", true))).await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("/v1/debug/instances"));
    }

    #[tokio::test]
    #[cfg(feature = "admin")]
    async fn gateway_openapi_lists_stable_debug_routes() {
        let (status, doc) = response_json(
            handle_v1_openapi(State(test_gateway_state_with_debug_routes("1.2.3", true)))
                .await
                .into_response(),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        for path in [
            "/v1/debug/instances",
            "/v1/debug/activity",
            "/v1/debug/calls",
            "/v1/debug/traces",
            "/v1/debug/traces/{request_id}",
            "/v1/debug/trace-context/{lookup_id}",
            "/v1/debug/tasks",
            "/v1/debug/bundles/{request_id}",
            "/v1/debug/issue-reports/{request_id}",
            "/v1/debug/logs",
            "/v1/debug/deregistered",
            "/v1/debug/stats",
            "/v1/debug/health",
        ] {
            assert!(
                doc["paths"].get(path).is_some(),
                "gateway OpenAPI doc missing debug path {path}: {doc:#}"
            );
        }
        assert!(
            doc["tags"]
                .as_array()
                .is_some_and(|tags| tags.iter().any(|tag| tag["name"] == "debug"))
        );
        assert!(
            doc["components"]["schemas"]
                .get("GatewayDebugPayload")
                .is_some()
        );
    }

    #[tokio::test]
    #[cfg(feature = "admin")]
    async fn gateway_openapi_omits_debug_routes_when_runtime_admin_disabled() {
        let (status, doc) = response_json(
            handle_v1_openapi(State(test_gateway_state("1.2.3")))
                .await
                .into_response(),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(doc["paths"].get("/v1/search").is_some());
        assert!(doc["paths"].get("/v1/debug/instances").is_none());
        assert!(
            doc["tags"]
                .as_array()
                .is_none_or(|tags| !tags.iter().any(|tag| tag["name"] == "debug"))
        );
        assert!(
            doc["components"]["schemas"]
                .get("GatewayDebugPayload")
                .is_none()
        );
    }

    #[tokio::test]
    #[cfg(not(feature = "admin"))]
    async fn gateway_openapi_omits_debug_routes_without_admin_feature() {
        let (status, doc) = response_json(
            handle_v1_openapi(State(test_gateway_state("1.2.3")))
                .await
                .into_response(),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(doc["paths"].get("/v1/search").is_some());
        assert!(doc["paths"].get("/v1/debug/instances").is_none());
        assert!(
            doc["tags"]
                .as_array()
                .is_none_or(|tags| !tags.iter().any(|tag| tag["name"] == "debug"))
        );
        assert!(
            doc["components"]["schemas"]
                .get("GatewayDebugPayload")
                .is_none()
        );
    }

    #[tokio::test]
    async fn gateway_yield_missing_challenger_is_structured_optional_capability() {
        let (status, body) = response_json(
            handle_gateway_yield(
                State(test_gateway_state("1.2.3")),
                axum::body::Bytes::from_static(b"{}"),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["success"], false);
        assert_eq!(body["fallback"], "polling");
        assert_eq!(body["error"]["kind"], "optional-capability-unsupported");
        assert_eq!(body["error"]["capability"], "cooperative_yield");
    }

    #[tokio::test]
    async fn gateway_yield_same_version_is_structured_optional_capability() {
        let (status, body) = response_json(
            handle_gateway_yield(
                State(test_gateway_state("1.2.3")),
                axum::body::Bytes::from_static(br#"{"challenger_version":"1.2.3"}"#),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["current_version"], "1.2.3");
        assert_eq!(body["challenger_version"], "1.2.3");
        assert_eq!(body["error"]["kind"], "optional-capability-unsupported");
    }

    #[tokio::test]
    async fn gateway_yield_newer_challenger_still_accepts() {
        let (status, body) = response_json(
            handle_gateway_yield(
                State(test_gateway_state("1.2.3")),
                axum::body::Bytes::from_static(br#"{"challenger_version":"1.2.4"}"#),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
    }

    #[tokio::test]
    async fn rest_trace_input_payload_uses_redacted_arguments() {
        use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain, RedactionMiddleware};

        let sink = Arc::new(CaptureSink::default());
        let chain = MiddlewareChain::new()
            .with_before(Arc::new(RedactionMiddleware::new(vec!["api_key", "token"])))
            .with_after(Arc::new(AuditMiddleware::new(sink.clone())));
        let mut gs = test_gateway_state("1.2.3");
        gs.middleware_chain = Arc::new(chain);

        let request_body = json!({
            "tool_slug": "maya.abcdef01.render",
            "arguments": {
                "api_key": "secret-key",
                "nested": {"token": "secret-token"}
            },
            "meta": {}
        });

        let result = call_service_with_admin_trace(
            &gs,
            &HeaderMap::new(),
            "v1/call",
            "maya.abcdef01.render",
            request_body["arguments"].clone(),
            request_body.get("meta").cloned(),
            &request_body,
        )
        .await;

        assert!(result.is_err());
        let entries = sink.0.lock().unwrap();
        assert_eq!(entries.len(), 1);
        let input = entries[0].input_payload.as_ref().unwrap().content.clone();
        assert!(input.contains("[REDACTED]"));
        assert!(!input.contains("secret-key"));
        assert!(!input.contains("secret-token"));
    }

    #[tokio::test]
    async fn rest_traceparent_does_not_replace_request_id() {
        use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain};

        let sink = Arc::new(CaptureSink::default());
        let chain = MiddlewareChain::new().with_after(Arc::new(AuditMiddleware::new(sink.clone())));
        let mut gs = test_gateway_state("1.2.3");
        gs.middleware_chain = Arc::new(chain);

        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", "req-rest-1".parse().unwrap());
        headers.insert(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
                .parse()
                .unwrap(),
        );
        let request_body = json!({
            "tool_slug": "maya.abcdef01.render",
            "arguments": {},
            "meta": {}
        });

        let _ = call_service_with_admin_trace(
            &gs,
            &headers,
            "v1/call",
            "maya.abcdef01.render",
            json!({}),
            request_body.get("meta").cloned(),
            &request_body,
        )
        .await;

        let entries = sink.0.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].request_id, "req-rest-1");
        assert_eq!(
            entries[0].trace_context.trace_id,
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert_eq!(
            entries[0].trace_context.parent_span_id.as_deref(),
            Some("00f067aa0ba902b7")
        );
        let root_span_id = entries[0]
            .trace_context
            .span_id
            .as_deref()
            .expect("REST trace context should create a gateway root span id");
        assert_eq!(root_span_id.len(), 16);
        let backend_span = entries[0]
            .trace_spans
            .iter()
            .find(|span| span.name == "backend.execute")
            .expect("REST call should record backend.execute span");
        let backend_span_id = backend_span
            .span_id
            .as_deref()
            .expect("backend.execute should carry its own span id");
        assert_eq!(backend_span_id.len(), 16);
        assert_ne!(backend_span_id, root_span_id);
        assert_eq!(backend_span.parent_span_id.as_deref(), Some(root_span_id));
    }

    #[tokio::test]
    async fn rest_call_batch_uses_arguments_mutated_by_before_middleware() {
        use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain, RedactionMiddleware};

        let sink = Arc::new(CaptureSink::default());
        let rewritten = json!({
            "calls": [{
                "tool_slug": "maya.abcdef01.render",
                "arguments": {"token": "secret-token"}
            }]
        });
        let chain = MiddlewareChain::new()
            .with_before(Arc::new(ReplaceArgs(rewritten)))
            .with_before(Arc::new(RedactionMiddleware::new(vec!["token"])))
            .with_after(Arc::new(AuditMiddleware::new(sink.clone())));
        let mut gs = test_gateway_state("1.2.3");
        gs.middleware_chain = Arc::new(chain);

        let result =
            call_batch_with_admin_trace(&gs, &HeaderMap::new(), &json!({"calls": []})).await;

        let body = result.expect("batch should use middleware-mutated args");
        assert_eq!(body["results"][0]["tool_slug"], "maya.abcdef01.render");
        let entries = sink.0.lock().unwrap();
        assert_eq!(entries.len(), 1);
        let input = entries[0].input_payload.as_ref().unwrap().content.clone();
        assert!(input.contains("[REDACTED]"));
        assert!(!input.contains("secret-token"));
    }
}
