use super::*;
use crate::gateway::capability::parse_slug;
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

    match req.method.as_str() {
        "initialize" => Some(handle_initialize_with_timeout(gs, id, req).await),
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
    }
}

async fn handle_initialize_with_timeout(
    gs: &GatewayState,
    id: Value,
    req: &JsonRpcRequest,
) -> Value {
    match tokio::time::timeout(
        GATEWAY_INITIALIZE_TIMEOUT,
        handle_initialize(gs, id.clone(), req),
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

async fn handle_initialize(gs: &GatewayState, id: Value, req: &JsonRpcRequest) -> Value {
    let client_version = req
        .params
        .as_ref()
        .and_then(|params| params.get("protocolVersion"))
        .and_then(|value| value.as_str());
    let negotiated = negotiate_protocol_version(client_version);
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
                "prompts": {"listChanged": true}
            },
            "serverInfo": {"name": gs.server_name, "version": gs.server_version},
            "instructions":
                "DCC-MCP Gateway — unified MCP endpoint across every live DCC.\n\
                 \n\
                 tools/list is intentionally bounded. It returns gateway discovery,\n\
                 skill lifecycle, pooling, and dynamic capability wrapper tools; it\n\
                 never fans out every backend action. Instance registry, diagnostics,\n\
                 and catalog views are MCP resources such as gateway://instances,\n\
                 gateway://diagnostics/*, gateway://catalog, and gateway://docs/agent-workflows.\n\
                 \n\
                 Workflow:\n\
                 1. Optional: resources/read uri=gateway://instances to inspect live DCCs\n\
                 1b. Optional: resources/read uri=gateway://docs/agent-workflows (MCP+REST patterns, path /v1/dcc/.../call, re-list instances after DCC restart)\n\
                 2. search_skills(...) then load_skill(..., instance_id=... when needed)\n\
                 3. search_tools(...) -> describe_tool(tool_slug=...) -> call_tool(tool_slug=..., arguments={...}); never put code/python/mel at the call_tool top level\n\
                 4. Optional: call_tools({calls:[{tool_slug, arguments}, ...], stop_on_error?}) for ordered batches (max 25)\n\
                 \n\
                 Subscribe to GET /mcp (SSE) for push notifications."
        }
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
    let agent_context = crate::gateway::admin::trace::AgentContext::from_request_parts(
        headers,
        req.params.as_ref(),
        meta.as_ref(),
    );
    let trace_context = crate::gateway::admin::trace::TraceContext::from_headers_with_request_id(
        headers,
        id_str.to_string(),
    );
    let resolved_slug = if tool == "call_tool" {
        args.get("tool_slug").and_then(Value::as_str)
    } else if tool == "call_tools" {
        args.get("calls")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.get("tool_slug"))
            .and_then(Value::as_str)
    } else {
        None
    };

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
        return json!({
            "jsonrpc": "2.0", "id": id,
            "result": {"content": [{"type": "text", "text": msg}], "isError": true}
        });
    }

    // Capture input after before-middlewares so trace storage sees redacted args.
    {
        use crate::gateway::admin::trace::{MAX_INPUT_BYTES, TracePayload};
        ctx.input_payload = Some(TracePayload::from_value(&ctx.args, MAX_INPUT_BYTES));
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
        Some(id_str.to_string()),
        Some(session_id),
        Some(&ctx.trace_context),
    )
    .await;

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
