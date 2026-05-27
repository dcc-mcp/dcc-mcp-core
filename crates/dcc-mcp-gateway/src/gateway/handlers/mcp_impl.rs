use super::*;
use crate::gateway::capability::parse_slug;
use crate::gateway::response_codec::{
    ResponseFormat, TOON_MIME, compact_call_batch_payload, compact_describe_payload,
    compact_search_payload, encode_response, explicit_format, token_telemetry_for_response,
};
use std::time::{Duration, Instant};

/// Log when gateway `/mcp` dispatch exceeds this threshold (issue #1009).
const GATEWAY_MCP_SLOW_DISPATCH_MS: u128 = 250;

/// Server-side deadline for `initialize` before returning `gateway-busy` (#1009).
const GATEWAY_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(5);

fn log_gateway_mcp_slow_dispatch(started: Instant, method: &str) {
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms > GATEWAY_MCP_SLOW_DISPATCH_MS {
        tracing::warn!(
            elapsed_ms = elapsed_ms as u64,
            method = method,
            "gateway MCP dispatch slow"
        );
    }
}

/// Minimal JSON-RPC 2.0 request shape accepted by the gateway `/mcp` endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// `POST /mcp` — gateway's own MCP endpoint (facade over every live DCC).
pub async fn handle_gateway_mcp(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let dispatch_started = Instant::now();
    let client_session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("gw-{}", uuid::Uuid::new_v4().simple()));

    let body_value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            let response = parse_error_response(&client_session_id, format!("Parse error: {err}"));
            log_gateway_mcp_slow_dispatch(dispatch_started, "parse_error");
            return response;
        }
    };

    if let Some(batch) = body_value.as_array() {
        let label = format!("batch[{}]", batch.len());
        let response = handle_batch_request(&gs, &client_session_id, batch, &headers).await;
        log_gateway_mcp_slow_dispatch(dispatch_started, &label);
        return response;
    }

    let req = match serde_json::from_value::<JsonRpcRequest>(body_value) {
        Ok(req) => req,
        Err(err) => {
            let response = parse_error_response(&client_session_id, format!("Parse error: {err}"));
            log_gateway_mcp_slow_dispatch(dispatch_started, "parse_error");
            return response;
        }
    };

    let method_label = req.method.clone();

    if req.id.is_none() {
        handle_notification(&gs, &req, &client_session_id).await;
        log_gateway_mcp_slow_dispatch(dispatch_started, &method_label);
        return StatusCode::ACCEPTED.into_response();
    }

    let response = if let Some(response) =
        dispatch_single_request(&gs, &req, &client_session_id, &headers).await
    {
        let mut response = Json(response).into_response();
        attach_session_header(&mut response, &client_session_id);
        response
    } else {
        StatusCode::ACCEPTED.into_response()
    };
    log_gateway_mcp_slow_dispatch(dispatch_started, &method_label);
    response
}

async fn handle_batch_request(
    gs: &GatewayState,
    client_session_id: &str,
    batch: &[Value],
    headers: &HeaderMap,
) -> Response {
    let mut responses = Vec::with_capacity(batch.len());

    for item in batch {
        let req = match serde_json::from_value::<JsonRpcRequest>(item.clone()) {
            Ok(req) => req,
            Err(_) => {
                responses.push(json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32700, "message": "Parse error"}
                }));
                continue;
            }
        };

        if req.id.is_none() {
            handle_notification(gs, &req, client_session_id).await;
            continue;
        }

        if let Some(response) = dispatch_single_request(gs, &req, client_session_id, headers).await
        {
            responses.push(response);
        }
    }

    if responses.is_empty() {
        return StatusCode::ACCEPTED.into_response();
    }

    let mut response = Json(responses).into_response();
    attach_session_header(&mut response, client_session_id);
    response
}

fn parse_error_response(client_session_id: &str, message: String) -> Response {
    let mut response = (
        StatusCode::BAD_REQUEST,
        Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":message}})),
    )
        .into_response();
    attach_session_header(&mut response, client_session_id);
    response
}

fn attach_session_header(response: &mut Response, client_session_id: &str) {
    if let Ok(header_value) = client_session_id.parse() {
        response
            .headers_mut()
            .insert("Mcp-Session-Id", header_value);
    }
}

/// Dispatch one JSON-RPC request (not notification) and return the response value.
pub(crate) async fn dispatch_single_request(
    gs: &GatewayState,
    req: &JsonRpcRequest,
    session_id: &str,
    headers: &HeaderMap,
) -> Option<Value> {
    let id = req.id.clone()?;
    let id_str = serde_json::to_string(&id).unwrap_or_default();

    let response = match req.method.as_str() {
        "initialize" => Some(handle_initialize_with_timeout(gs, id, req, session_id).await),
        "ping" => Some(json!({"jsonrpc":"2.0","id":id,"result":{}})),
        "notifications/initialized" => Some(json!({"jsonrpc":"2.0","id":id,"result":{}})),
        "tools/list" => Some(handle_tools_list(gs, id, req).await),
        "resources/list" => Some(handle_resources_list(gs, id).await),
        "resources/read" => Some(handle_resources_read(gs, id, req).await),
        "resources/subscribe" => {
            Some(handle_resource_subscription(gs, id, req, session_id, true).await)
        }
        "resources/unsubscribe" => {
            Some(handle_resource_subscription(gs, id, req, session_id, false).await)
        }
        "prompts/list" => Some(handle_prompts_list(gs, id).await),
        "prompts/get" => Some(handle_prompts_get(gs, id, &id_str, req).await),
        "tools/call" => Some(handle_tools_call(gs, id, &id_str, req, session_id, headers).await),
        other => Some(json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {other}")}
        })),
    };
    response.map(|response| apply_mcp_compact_response(req, response))
}

async fn handle_initialize_with_timeout(
    gs: &GatewayState,
    id: Value,
    req: &JsonRpcRequest,
    session_id: &str,
) -> Value {
    match tokio::time::timeout(
        GATEWAY_INITIALIZE_TIMEOUT,
        handle_initialize(gs, id.clone(), req, session_id),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => gateway_busy_initialize_response(id),
    }
}

fn gateway_busy_initialize_response(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": dcc_mcp_jsonrpc::error_codes::GATEWAY_BUSY,
            "message": "gateway-busy: initialize did not complete within 5s; \
                the gateway may be starved by a busy DCC host — retry with fewer \
                concurrent MCP clients or connect directly to a per-instance port",
            "data": {
                "reason": "gateway-busy",
                "timeout_secs": GATEWAY_INITIALIZE_TIMEOUT.as_secs()
            }
        }
    })
}

async fn handle_initialize(
    gs: &GatewayState,
    id: Value,
    req: &JsonRpcRequest,
    session_id: &str,
) -> Value {
    let client_version = req
        .params
        .as_ref()
        .and_then(|params| params.get("protocolVersion"))
        .and_then(|value| value.as_str());
    let negotiated = negotiate_protocol_version(client_version);
    gs.client_attribution
        .record_mcp_initialize(session_id, req.params.as_ref())
        .await;
    match gs.protocol_version.try_write() {
        Ok(mut protocol_version) => {
            *protocol_version = Some(negotiated.to_string());
        }
        Err(_) => {
            tracing::warn!(
                protocol_version = negotiated,
                "gateway initialize: protocol version lock busy; continuing without updating cached value"
            );
        }
    }

    json!({
        "jsonrpc": "2.0", "id": id,
        "result": {
            "protocolVersion": negotiated,
            "capabilities": {
                "tools": {"listChanged": true},
                "resources": {"listChanged": true, "subscribe": true},
                "prompts": {"listChanged": true},
                "experimental": {
                    "dcc-mcp": {
                        "compactResponses": {
                            "formats": ["json", "toon"],
                            "default": "json-rpc-json",
                            "request": "Compact-capable clients should set params._meta.response_format=\"toon\" or params._meta.compact=true on high-token requests. Set params._meta.response_format=\"json\" to opt out. The JSON-RPC envelope stays JSON."
                        }
                    }
                }
            },
            "serverInfo": {"name": gs.server_name, "version": gs.server_version},
            "instructions":
                "DCC-MCP Gateway — unified MCP endpoint across every live DCC.\n\
                 \n\
                 tools/list is intentionally bounded. It returns exactly four gateway\n\
                 workflow tools: search, describe, load_skill, and call. It never\n\
                 fans out every backend action. Instance registry, diagnostics,\n\
                 and catalog views are MCP resources such as gateway://instances,\n\
                 gateway://diagnostics/*, gateway://catalog, and gateway://docs/agent-workflows.\n\
                 \n\
                 Workflow:\n\
                 1. Optional: resources/read uri=gateway://instances to inspect live DCCs\n\
                 1b. Optional: resources/read uri=gateway://docs/agent-workflows (MCP+REST patterns, path /v1/dcc/.../call, re-list instances after DCC restart)\n\
                 2. search(kind=\"skill\", ...) then load_skill(skill_name=..., instance_id=... when needed)\n\
                 3. search(kind=\"tool\", ...) -> describe(tool_slug=...) -> call(tool_slug=..., arguments={...}); never put code/python/mel at the call top level\n\
                 4. Optional: call({calls:[{tool_slug, arguments}, ...], stop_on_error?}) for ordered batches (max 25)\n\
                 5. Compact-capable clients should request compact TOON payloads on high-token calls with params._meta.response_format=\"toon\" or params._meta.compact=true; use params._meta.response_format=\"json\" to opt out. JSON-RPC jsonrpc/id/result/error stay JSON.\n\
                 \n\
                 Subscribe to GET /mcp (SSE) for push notifications."
        }
    })
}

fn apply_mcp_compact_response(req: &JsonRpcRequest, response: Value) -> Value {
    if matches!(
        req.method.as_str(),
        "initialize" | "ping" | "notifications/initialized"
    ) {
        return response;
    }
    if !matches!(mcp_requested_response_format(req), ResponseFormat::Toon)
        || response.get("error").is_some()
    {
        return response;
    }

    let Some(result) = response.get("result") else {
        return response;
    };
    let Some(compact_result) = (match req.method.as_str() {
        "tools/call" => compact_mcp_tool_result(req, result),
        _ => compact_mcp_result(result, result),
    }) else {
        return response;
    };

    let mut compact_response = response;
    if let Some(obj) = compact_response.as_object_mut() {
        obj.insert("result".to_string(), compact_result);
    }
    compact_response
}

fn mcp_requested_response_format(req: &JsonRpcRequest) -> ResponseFormat {
    let Some(params) = req.params.as_ref() else {
        return ResponseFormat::Json;
    };

    [
        Some(params),
        params.get("_meta"),
        params.get("meta"),
        params.get("arguments"),
        params.get("arguments").and_then(|args| args.get("_meta")),
        params.get("arguments").and_then(|args| args.get("meta")),
    ]
    .into_iter()
    .flatten()
    .find_map(explicit_format)
    .unwrap_or(ResponseFormat::Json)
}

fn compact_mcp_result(legacy_result: &Value, compact_result: &Value) -> Option<Value> {
    let encoded =
        encode_response(legacy_result, Some(compact_result), ResponseFormat::Toon).ok()?;
    let text = String::from_utf8(encoded.body).ok()?;
    Some(json!({
        "response_format": "toon",
        "mimeType": TOON_MIME,
        "text": text,
        "_meta": mcp_compact_meta(encoded.accounting.to_json(encoded.format))
    }))
}

fn compact_mcp_tool_result(req: &JsonRpcRequest, result: &Value) -> Option<Value> {
    let mut compact_result = result.clone();
    let content = compact_result
        .get_mut("content")
        .and_then(Value::as_array_mut)?;
    let text_content = content
        .iter_mut()
        .find(|entry| entry.get("type").and_then(Value::as_str) == Some("text"))?;
    let legacy_text = text_content.get("text").and_then(Value::as_str)?;
    let legacy_payload =
        serde_json::from_str::<Value>(legacy_text).unwrap_or_else(|_| json!({"text": legacy_text}));
    let compact_payload = compact_tool_text_payload(tool_name_from_request(req), &legacy_payload);
    let encoded = encode_response(
        &legacy_payload,
        Some(&compact_payload),
        ResponseFormat::Toon,
    )
    .ok()?;
    let text = String::from_utf8(encoded.body).ok()?;

    if let Some(obj) = text_content.as_object_mut() {
        obj.insert("mimeType".to_string(), Value::String(TOON_MIME.to_string()));
        obj.insert("text".to_string(), Value::String(text));
    }
    if let Some(obj) = compact_result.as_object_mut() {
        obj.insert(
            "_meta".to_string(),
            mcp_compact_meta(encoded.accounting.to_json(encoded.format)),
        );
    }
    Some(compact_result)
}

fn mcp_tool_token_telemetry(
    req: &JsonRpcRequest,
    result: &Value,
) -> Option<crate::gateway::admin::trace::TokenTelemetry> {
    let text_content = result
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .find(|entry| entry.get("type").and_then(Value::as_str) == Some("text"))?;
    let legacy_text = text_content.get("text").and_then(Value::as_str)?;
    let legacy_payload =
        serde_json::from_str::<Value>(legacy_text).unwrap_or_else(|_| json!({"text": legacy_text}));
    let format = mcp_requested_response_format(req);
    let compact_payload = if matches!(format, ResponseFormat::Toon) {
        Some(compact_tool_text_payload(
            tool_name_from_request(req),
            &legacy_payload,
        ))
    } else {
        None
    };
    token_telemetry_for_response(&legacy_payload, compact_payload.as_ref(), format)
}

fn compact_tool_text_payload(tool_name: Option<&str>, legacy_payload: &Value) -> Value {
    match tool_name {
        Some("search" | "search_tools") => {
            if let Some(hits) = legacy_payload.get("hits").and_then(Value::as_array) {
                let total = legacy_payload
                    .get("total")
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(hits.len());
                compact_search_payload(total, hits)
            } else {
                legacy_payload.clone()
            }
        }
        Some("describe" | "describe_tool")
            if legacy_payload.get("record").is_some() || legacy_payload.get("tool").is_some() =>
        {
            compact_describe_payload(legacy_payload)
        }
        Some("call" | "call_tools") if legacy_payload.get("results").is_some() => {
            compact_call_batch_payload(legacy_payload)
        }
        _ => legacy_payload.clone(),
    }
}

fn tool_name_from_request(req: &JsonRpcRequest) -> Option<&str> {
    req.params
        .as_ref()
        .and_then(|params| params.get("name"))
        .and_then(Value::as_str)
}

fn mcp_compact_meta(token_accounting: Value) -> Value {
    json!({
        "schema_version": "dcc-mcp.mcp.compact-result.v1",
        "response_format": "toon",
        "token_accounting": token_accounting,
    })
}

async fn handle_tools_list(gs: &GatewayState, id: Value, req: &JsonRpcRequest) -> Value {
    let cursor = req
        .params
        .as_ref()
        .and_then(|params| params.get("cursor"))
        .and_then(|value| value.as_str());
    let result = aggregator::aggregate_tools_list(gs, cursor).await;
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

async fn handle_resources_list(gs: &GatewayState, id: Value) -> Value {
    super::resources::handle_resources_list(gs, id).await
}

async fn handle_resources_read(gs: &GatewayState, id: Value, req: &JsonRpcRequest) -> Value {
    super::resources::handle_resources_read(gs, id, req).await
}

async fn handle_resource_subscription(
    gs: &GatewayState,
    id: Value,
    req: &JsonRpcRequest,
    session_id: &str,
    subscribe: bool,
) -> Value {
    super::resources::handle_resource_subscription(gs, id, req, session_id, subscribe).await
}

async fn handle_tools_call(
    gs: &GatewayState,
    id: Value,
    id_str: &str,
    req: &JsonRpcRequest,
    session_id: &str,
    headers: &HeaderMap,
) -> Value {
    let tool = req
        .params
        .as_ref()
        .and_then(|params| params.get("name"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let args_raw = req
        .params
        .as_ref()
        .and_then(|params| params.get("arguments"))
        .cloned();
    let args = match dcc_mcp_jsonrpc::coerce_tool_arguments_object(args_raw) {
        Ok(v) => v,
        Err(message) => {
            return json!({
                "jsonrpc": "2.0", "id": id,
                "error": {
                    "code": dcc_mcp_jsonrpc::error_codes::INVALID_PARAMS,
                    "message": message
                }
            });
        }
    };
    let meta = req
        .params
        .as_ref()
        .and_then(|params| params.get("_meta"))
        .cloned();
    let agent_context =
        crate::gateway::admin::trace::AgentContext::from_request_parts_with_server_network(
            headers,
            req.params.as_ref(),
            meta.as_ref(),
        );
    let agent_context = gs
        .client_attribution
        .augment_mcp_context(session_id, agent_context)
        .await;
    let trace_context = crate::gateway::admin::trace::TraceContext::from_headers_with_request_id(
        headers,
        id_str.to_string(),
    );
    let first_batch_slug = || {
        args.get("calls")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.get("tool_slug"))
            .and_then(Value::as_str)
    };
    let resolved_slug = match tool {
        "call" => args
            .get("tool_slug")
            .and_then(Value::as_str)
            .or_else(first_batch_slug),
        "call_tool" => args.get("tool_slug").and_then(Value::as_str),
        "call_tools" => first_batch_slug(),
        _ => None,
    };
    let target_dcc = if let Some((dcc_type, _, _)) = resolved_slug.and_then(parse_slug) {
        Some(dcc_type.to_string())
    } else {
        args.get("dcc_type")
            .or_else(|| args.get("dcc"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    if let Err(err) = gs.security.authorize(
        headers,
        crate::gateway::GatewayAuthScope::Call,
        target_dcc.as_deref(),
    ) {
        return json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": err.message
                }],
                "isError": true
            }
        });
    }

    {
        let mut pending = gs.pending_calls.write().await;
        pending.insert(
            id_str.to_string(),
            super::super::state::PendingCall {
                backend_url: String::new(),
                backend_request_id: id_str.to_string(),
            },
        );
    }

    // ── Middleware: BeforeCall ────────────────────────────────────────────
    let mut ctx = crate::gateway::middleware::CallContext::new("tools/call", id_str, args.clone())
        .with_tool_slug(resolved_slug.unwrap_or(tool))
        .with_session_id(session_id)
        .with_transport("mcp")
        .with_agent_context(agent_context)
        .with_trace_context(trace_context);
    if let Some((dcc_type, instance_hint, _)) = resolved_slug.and_then(parse_slug) {
        ctx = ctx.with_backend(dcc_type, instance_hint);
    } else if let Some(dcc_type) = args
        .get("dcc_type")
        .or_else(|| args.get("dcc"))
        .and_then(Value::as_str)
    {
        ctx.dcc_type = Some(dcc_type.to_string());
    }

    // Run before-middlewares; abort the call if any rejects.
    if !gs.middleware_chain.is_empty()
        && let Err(e) = gs.middleware_chain.run_before(&mut ctx).await
    {
        let mut pending = gs.pending_calls.write().await;
        pending.remove(id_str);
        let msg = e.to_string();
        crate::gateway::metrics::record_gateway_governance_event(e.governance_category(), e.kind());
        crate::gateway::agent_telemetry::AgentWorkflowEvent::new("gateway.call", "mcp")
            .with_trace_context(Some(&ctx.trace_context))
            .with_agent_context(ctx.agent_context.as_ref())
            .with_session_id(ctx.session_id.as_deref())
            .with_route(
                ctx.tool_slug.as_deref(),
                None,
                ctx.dcc_type.as_deref(),
                ctx.instance_id.as_deref(),
            )
            .with_outcome(false, Some(e.kind()))
            .with_policy_reason(Some(e.governance_category()))
            .emit();
        {
            use crate::gateway::admin::trace::{MAX_INPUT_BYTES, MAX_OUTPUT_BYTES, TracePayload};
            ctx.input_payload = Some(TracePayload::from_input_value(&ctx.args, MAX_INPUT_BYTES));
            ctx.output_payload = Some(TracePayload::from_str(&msg, MAX_OUTPUT_BYTES));
        }
        let rejected_result =
            json!({"content": [{"type": "text", "text": msg.clone()}], "isError": true});
        ctx.token_accounting = mcp_tool_token_telemetry(req, &rejected_result);
        let mut rejected = crate::gateway::middleware::CallResult::from_tuple(&msg, true);
        if let Err(after_err) = gs.middleware_chain.run_after(&ctx, &mut rejected).await {
            tracing::warn!(error = %after_err, "gateway middleware after-call failed for rejected MCP call");
        }
        return json!({
            "jsonrpc": "2.0", "id": id,
            "result": {"content": [{"type": "text", "text": msg}], "isError": true}
        });
    }

    // Capture input after before-middlewares so trace storage sees redacted args.
    {
        use crate::gateway::admin::trace::{MAX_INPUT_BYTES, TracePayload};
        ctx.input_payload = Some(TracePayload::from_input_value(&ctx.args, MAX_INPUT_BYTES));
    }

    // Use potentially-redacted args from context.
    let effective_args = if gs.middleware_chain.is_empty() {
        args
    } else {
        ctx.args.clone()
    };
    emit_mcp_traffic_frame(
        gs,
        &ctx,
        headers,
        McpTrafficFrame {
            id: &id,
            direction: "inbound",
            leg: "client_to_gateway",
            status: None,
            body: json!({
            "jsonrpc": req.jsonrpc.clone().unwrap_or_else(|| "2.0".to_string()),
            "id": id.clone(),
            "method": "tools/call",
            "params": {
                "name": tool,
                "arguments": effective_args.clone(),
                "_meta": meta.clone(),
            },
            }),
        },
    );

    // Phase 2: backend.execute span
    let dispatch_ns = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    let (text, is_error) = aggregator::route_tools_call(
        gs,
        tool,
        &effective_args,
        meta.as_ref(),
        Some(session_id),
        Some(&ctx.trace_context),
        ctx.agent_context.as_ref(),
    )
    .await;

    crate::gateway::agent_telemetry::emit_mcp_tool_event(
        crate::gateway::agent_telemetry::McpToolTelemetryInput {
            search_telemetry: &gs.search_telemetry,
            tool,
            args: &effective_args,
            meta: meta.as_ref(),
            trace_context: Some(&ctx.trace_context),
            agent_context: ctx.agent_context.as_ref(),
            session_id: Some(session_id),
            text: &text,
            is_error,
        },
    );

    {
        use crate::gateway::admin::trace::{MAX_OUTPUT_BYTES, TracePayload};
        let response_ns = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        ctx.push_span(
            ctx.trace_context
                .child_span(
                    "backend.execute",
                    dispatch_ns,
                    response_ns.saturating_sub(dispatch_ns),
                )
                .with_attr("tool_slug", tool)
                .with_attr("ok", !is_error),
        );
        ctx.output_payload = Some(TracePayload::from_str(&text, MAX_OUTPUT_BYTES));
    }

    // ── Middleware: AfterCall ────────────────────────────────────────────
    let mut call_result = crate::gateway::middleware::CallResult::from_tuple(&text, is_error);
    let audit_result = json!({
        "content": [{"type": "text", "text": call_result.text.clone()}],
        "isError": call_result.is_error,
    });
    ctx.token_accounting = mcp_tool_token_telemetry(req, &audit_result);

    if !gs.middleware_chain.is_empty()
        && let Err(e) = gs.middleware_chain.run_after(&ctx, &mut call_result).await
    {
        let mut pending = gs.pending_calls.write().await;
        pending.remove(id_str);
        let msg = e.to_string();
        return json!({
            "jsonrpc": "2.0", "id": id,
            "result": {"content": [{"type": "text", "text": msg}], "isError": true}
        });
    }

    let (final_text, final_is_error) = call_result.into_tuple();

    {
        let mut pending = gs.pending_calls.write().await;
        pending.remove(id_str);
    }

    let response = json!({
        "jsonrpc": "2.0", "id": id,
        "result": {"content": [{"type": "text", "text": final_text}], "isError": final_is_error}
    });
    emit_mcp_traffic_frame(
        gs,
        &ctx,
        headers,
        McpTrafficFrame {
            id: response.get("id").unwrap_or(&Value::Null),
            direction: "outbound",
            leg: "gateway_to_client",
            status: Some(200),
            body: response.clone(),
        },
    );
    response
}

struct McpTrafficFrame<'a> {
    id: &'a Value,
    direction: &'static str,
    leg: &'static str,
    status: Option<u16>,
    body: Value,
}

fn emit_mcp_traffic_frame(
    gs: &GatewayState,
    ctx: &crate::gateway::middleware::CallContext,
    headers: &HeaderMap,
    frame: McpTrafficFrame<'_>,
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
            "/mcp",
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
            Some(frame.id.clone()),
        )),
    );
}

/// `prompts/list` — fan out to every live backend, namespace entries by
/// instance, and return the merged list (issue #731).
///
/// A zero-backend gateway returns `{"prompts": []}` rather than a
/// `Method not found` so MCP clients can uniformly discover prompts
/// through the facade.
async fn handle_prompts_list(gs: &GatewayState, id: Value) -> Value {
    let result = aggregator::aggregate_prompts_list(gs).await;
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// `prompts/get` — decode the namespaced prompt name and forward to the
/// owning backend (issue #731). Errors are surfaced as JSON-RPC errors
/// with codes matching the resolution failure (`-32602` for routing,
/// `-32000` for backend failure).
async fn handle_prompts_get(
    gs: &GatewayState,
    id: Value,
    id_str: &str,
    req: &JsonRpcRequest,
) -> Value {
    let name = req
        .params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if name.is_empty() {
        return json!({
            "jsonrpc": "2.0", "id": id,
            "error": {
                "code": -32602,
                "message": "prompts/get requires a non-empty 'name' parameter"
            }
        });
    }
    let arguments = req
        .params
        .as_ref()
        .and_then(|p| p.get("arguments"))
        .cloned();

    match aggregator::route_prompts_get(gs, name, arguments, Some(id_str.to_string())).await {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(e) => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": e.code(), "message": e.message()}
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tokio::sync::{RwLock, broadcast, watch};

    #[derive(Default)]
    struct CaptureSink(Mutex<Vec<crate::gateway::middleware::AuditEntry>>);

    impl crate::gateway::middleware::AuditSink for CaptureSink {
        fn record(&self, entry: crate::gateway::middleware::AuditEntry) {
            self.0.lock().unwrap().push(entry);
        }
    }

    fn test_gateway_state() -> GatewayState {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
        let (yield_tx, _) = watch::channel(false);
        let (events_tx, _) = broadcast::channel::<String>(8);
        GatewayState {
            registry,
            http_instance_registry: Arc::new(parking_lot::RwLock::new(
                crate::gateway::http_registration::HttpInstanceRegistry::default(),
            )),
            mdns_instance_registry: Arc::new(parking_lot::RwLock::new(
                crate::gateway::mdns_discovery::MdnsInstanceRegistry::default(),
            )),
            relay_instance_registry: Arc::new(parking_lot::RwLock::new(
                crate::gateway::relay_discovery::RelayInstanceRegistry::default(),
            )),
            stale_timeout: Duration::from_secs(30),
            backend_timeout: Duration::from_secs(10),
            async_dispatch_timeout: Duration::from_secs(60),
            wait_terminal_timeout: Duration::from_secs(600),
            server_name: "test-gateway".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
            own_host: "127.0.0.1".into(),
            own_port: 9765,
            http_client: reqwest::Client::new(),
            yield_tx: Arc::new(yield_tx),
            events_tx: Arc::new(events_tx),
            protocol_version: Arc::new(RwLock::new(None)),
            resource_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            client_attribution: Arc::new(
                crate::gateway::caller_attribution::ClientAttributionStore::default(),
            ),
            pending_calls: Arc::new(RwLock::new(HashMap::new())),
            subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
            allow_unknown_tools: false,
            policy: Arc::new(crate::gateway::GatewayPolicy::default()),
            security: Arc::new(crate::gateway::GatewaySecurityPolicy::disabled()),
            adapter_version: None,
            adapter_dcc: None,
            capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
            event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
            middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
            instance_diagnostics: Arc::new(
                crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
            ),
            traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
            search_telemetry: Arc::new(
                crate::gateway::search_telemetry::SearchTelemetryStore::new(),
            ),
            debug_routes_enabled: false,
            #[cfg(feature = "prometheus")]
            gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        }
    }

    fn request(method: &str, id: Value, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(id),
            method: method.into(),
            params,
        }
    }

    async fn dispatch(req: &JsonRpcRequest) -> Value {
        let gs = test_gateway_state();
        dispatch_single_request(&gs, req, "test-session", &HeaderMap::new())
            .await
            .expect("request has id")
    }

    fn decode_toon_result(result: &Value) -> Value {
        let text = result["text"].as_str().expect("compact result has text");
        toon_format::decode_default(text).expect("compact result is valid TOON")
    }

    #[tokio::test]
    async fn initialize_advertises_mcp_compact_response_capability() {
        let req = request(
            "initialize",
            json!(1),
            Some(json!({"protocolVersion": "2025-03-26"})),
        );
        let response = dispatch(&req).await;

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(
            response["result"]["capabilities"]["experimental"]["dcc-mcp"]["compactResponses"]["formats"]
                [1],
            "toon"
        );
        assert!(
            response["result"].get("text").is_none(),
            "initialize must remain legacy JSON so clients can negotiate capabilities"
        );
    }

    #[tokio::test]
    async fn tools_list_legacy_response_stays_json() {
        let req = request("tools/list", json!(2), None);
        let response = dispatch(&req).await;

        let tools = response["result"]["tools"]
            .as_array()
            .expect("legacy tools/list returns tools array");
        assert_eq!(tools.len(), 4, "gateway tools/list must stay bounded");
        assert!(response["result"].get("text").is_none());
    }

    #[tokio::test]
    async fn tools_list_can_return_json_rpc_safe_compact_toon() {
        let req = request(
            "tools/list",
            json!("compact-list"),
            Some(json!({"_meta": {"response_format": "toon"}})),
        );
        let response = dispatch(&req).await;

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], "compact-list");
        assert_eq!(response["result"]["response_format"], "toon");
        assert_eq!(response["result"]["mimeType"], TOON_MIME);
        assert_eq!(
            response["result"]["_meta"]["token_accounting"]["response_format"],
            "toon"
        );

        let decoded = decode_toon_result(&response["result"]);
        let tools = decoded["tools"]
            .as_array()
            .expect("compact tools/list decodes to tools array");
        assert_eq!(
            tools.len(),
            4,
            "compact mode must not fan out backend tools"
        );
    }

    #[tokio::test]
    async fn explicit_mcp_json_opt_out_wins_over_compact_alias() {
        let req = request(
            "tools/list",
            json!("json-opt-out"),
            Some(json!({"_meta": {"response_format": "json", "compact": true}})),
        );
        let response = dispatch(&req).await;

        let tools = response["result"]["tools"]
            .as_array()
            .expect("explicit JSON opt-out keeps the legacy tools/list shape");
        assert_eq!(tools.len(), 4);
        assert!(response["result"].get("text").is_none());
        assert!(response["result"].get("response_format").is_none());
    }

    #[tokio::test]
    async fn batch_request_compacts_only_opted_in_items() {
        let gs = test_gateway_state();
        let batch = vec![
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {"_meta": {"compact": true}}
            }),
            json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
        ];

        let response = handle_batch_request(&gs, "test-session", &batch, &HeaderMap::new()).await;
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("batch response body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC batch body");
        let items = body.as_array().expect("batch response is array");

        assert_eq!(items[0]["result"]["response_format"], "toon");
        assert_eq!(items[1]["result"], json!({}));
        assert!(items[1]["result"].get("text").is_none());
    }

    #[tokio::test]
    async fn resources_read_compact_preserves_content_hints_inside_toon() {
        let req = request(
            "resources/read",
            json!("docs"),
            Some(json!({
                "uri": "gateway://docs/agent-workflows",
                "_meta": {"response_format": "toon"}
            })),
        );
        let response = dispatch(&req).await;

        assert_eq!(response["result"]["response_format"], "toon");
        let decoded = decode_toon_result(&response["result"]);
        assert_eq!(
            decoded["contents"][0]["uri"],
            "gateway://docs/agent-workflows"
        );
        assert_eq!(decoded["contents"][0]["mimeType"], "application/json");
        assert!(decoded["contents"][0]["text"].as_str().is_some());
    }

    #[tokio::test]
    async fn tools_call_search_compact_preserves_call_tool_result_shape() {
        let req = request(
            "tools/call",
            json!(3),
            Some(json!({
                "name": "search",
                "arguments": {"kind": "tool", "query": "sphere"},
                "_meta": {"responseFormat": "toon"}
            })),
        );
        let response = dispatch(&req).await;

        assert_eq!(response["result"]["isError"], false);
        assert_eq!(response["result"]["content"][0]["type"], "text");
        assert_eq!(response["result"]["content"][0]["mimeType"], TOON_MIME);
        assert_eq!(
            response["result"]["_meta"]["token_accounting"]["response_format"],
            "toon"
        );
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("compact tool content has text");
        let decoded: Value = toon_format::decode_default(text).expect("tool content is TOON");
        assert_eq!(decoded["total"], 0);
        assert!(decoded["hits"].as_array().is_some());
    }

    #[tokio::test]
    async fn tools_call_search_records_meta_and_server_network_attribution() {
        let gs = test_gateway_state();
        let mut headers = HeaderMap::new();
        headers.insert(
            crate::gateway::caller_attribution::INTERNAL_SOURCE_IP_HEADER,
            "192.0.2.44".parse().unwrap(),
        );
        let req = request(
            "tools/call",
            json!("attributed-search"),
            Some(json!({
                "name": "search",
                "arguments": {"kind": "tool", "query": "sphere"},
                "_meta": {
                    "agent_context": {
                        "actor_id": "artist-1",
                        "client_platform": "cursor",
                        "sourceIp": "203.0.113.100"
                    }
                }
            })),
        );

        let response = dispatch_single_request(&gs, &req, "test-session", &headers)
            .await
            .expect("request has id");

        assert_eq!(response["result"]["isError"], false);
        let telemetry = gs.search_telemetry.snapshot(10);
        let agent = telemetry.recent[0]
            .agent_context
            .as_ref()
            .expect("MCP search should keep attribution");
        assert_eq!(agent.actor_id.as_deref(), Some("artist-1"));
        assert_eq!(agent.client_platform.as_deref(), Some("cursor"));
        assert_eq!(agent.source_ip.as_deref(), Some("192.0.2.44"));
    }

    #[tokio::test]
    async fn initialize_client_info_flows_to_mcp_call_admin_stats() {
        let trace_log = Arc::new(crate::gateway::admin::TraceLog::new(10));
        let audit_log: Arc<crate::gateway::admin::AuditLog> =
            Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = Arc::new(
            crate::gateway::admin::AdminAuditSink::new(audit_log, 10)
                .with_trace_log(trace_log.clone()),
        );
        let audit = Arc::new(crate::gateway::middleware::AuditMiddleware::new(sink));
        let mut gs = test_gateway_state();
        gs.middleware_chain = Arc::new(
            crate::gateway::middleware::MiddlewareChain::new()
                .with_before(audit.clone())
                .with_after(audit),
        );
        let init = request(
            "initialize",
            json!("init"),
            Some(json!({
                "protocolVersion": "2025-03-26",
                "clientInfo": {"name": "Codex Desktop", "version": "1.2.3"}
            })),
        );
        let init_response =
            dispatch_single_request(&gs, &init, "client-session-a", &HeaderMap::new())
                .await
                .expect("initialize request has id");
        assert_eq!(
            init_response["result"]["serverInfo"]["name"],
            "test-gateway"
        );

        let call = request(
            "tools/call",
            json!("client-call"),
            Some(json!({
                "name": "search",
                "arguments": {"kind": "tool", "query": "sphere"}
            })),
        );
        let call_response =
            dispatch_single_request(&gs, &call, "client-session-a", &HeaderMap::new())
                .await
                .expect("call request has id");
        assert_eq!(call_response["result"]["isError"], false);

        let traces = trace_log.recent(10);
        assert_eq!(traces.len(), 1);
        let agent = traces[0]
            .agent_context
            .as_ref()
            .expect("MCP call should inherit initialize client attribution");
        assert_eq!(agent.agent_name.as_deref(), Some("Codex Desktop"));
        assert_eq!(agent.agent_version.as_deref(), Some("1.2.3"));
        assert_eq!(agent.agent_kind.as_deref(), Some("mcp-client"));
        assert_eq!(agent.client_platform.as_deref(), Some("Codex Desktop"));

        let stats = crate::gateway::admin::StatsAggregator::new(trace_log)
            .compute(crate::gateway::admin::StatsRange::All);
        assert_eq!(stats.top_agents[0].name, "Codex Desktop@1.2.3");
        assert_eq!(stats.top_client_platforms[0].name, "Codex Desktop");
    }

    #[tokio::test]
    async fn tools_call_audit_records_compact_token_accounting() {
        let sink = Arc::new(CaptureSink::default());
        let audit_middleware = Arc::new(crate::gateway::middleware::AuditMiddleware::new(
            sink.clone(),
        ));
        let mut gs = test_gateway_state();
        gs.middleware_chain = Arc::new(
            crate::gateway::middleware::MiddlewareChain::new()
                .with_before(audit_middleware.clone())
                .with_after(audit_middleware),
        );
        let req = request(
            "tools/call",
            json!("compact-audit"),
            Some(json!({
                "name": "call",
                "arguments": {
                    "tool_slug": "maya.abcdef01.render",
                    "arguments": {}
                },
                "_meta": {"response_format": "toon"}
            })),
        );

        let response = dispatch_single_request(&gs, &req, "test-session", &HeaderMap::new())
            .await
            .expect("request has id");

        assert_eq!(
            response["result"]["_meta"]["token_accounting"]["response_format"],
            "toon"
        );
        let entries = sink.0.lock().unwrap();
        let tokens = entries[0]
            .token_accounting
            .as_ref()
            .expect("MCP audit should capture compact token accounting");
        assert_eq!(tokens.response_format, "toon");
        assert_eq!(tokens.token_estimator, "dcc-mcp-byte4-v1");
        assert!(tokens.original_tokens >= tokens.returned_tokens);
    }

    #[tokio::test]
    async fn tools_call_compact_preserves_text_error_payloads() {
        let req = request(
            "tools/call",
            json!("describe-error"),
            Some(json!({
                "name": "describe",
                "arguments": {},
                "_meta": {"response_format": "toon"}
            })),
        );
        let response = dispatch(&req).await;

        assert_eq!(response["result"]["isError"], true);
        assert_eq!(response["result"]["content"][0]["mimeType"], TOON_MIME);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("compact tool error has text");
        let decoded: Value = toon_format::decode_default(text).expect("tool error is TOON");
        assert!(
            decoded["text"]
                .as_str()
                .is_some_and(|message| message.contains("describe requires")),
            "compact error should preserve original message, got {decoded}"
        );
    }

    #[tokio::test]
    async fn compact_tools_call_keeps_json_rpc_errors_unchanged() {
        let req = request(
            "tools/call",
            json!(4),
            Some(json!({
                "name": "search",
                "arguments": [1, 2, 3],
                "_meta": {"response_format": "toon"}
            })),
        );
        let response = dispatch(&req).await;

        assert_eq!(
            response["error"]["code"],
            dcc_mcp_jsonrpc::error_codes::INVALID_PARAMS
        );
        assert!(response.get("result").is_none());
        assert!(response["error"].get("_meta").is_none());
    }
}
