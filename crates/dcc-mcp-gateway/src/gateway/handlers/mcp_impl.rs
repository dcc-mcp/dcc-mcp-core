use super::*;

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
    let client_session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("gw-{}", uuid::Uuid::new_v4().simple()));

    let body_value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => return parse_error_response(&client_session_id, format!("Parse error: {err}")),
    };

    if let Some(batch) = body_value.as_array() {
        return handle_batch_request(&gs, &client_session_id, batch).await;
    }

    let req = match serde_json::from_value::<JsonRpcRequest>(body_value) {
        Ok(req) => req,
        Err(err) => return parse_error_response(&client_session_id, format!("Parse error: {err}")),
    };

    if req.id.is_none() {
        handle_notification(&gs, &req, &client_session_id).await;
        return StatusCode::ACCEPTED.into_response();
    }

    if let Some(response) = dispatch_single_request(&gs, &req, &client_session_id).await {
        let mut response = Json(response).into_response();
        attach_session_header(&mut response, &client_session_id);
        response
    } else {
        StatusCode::ACCEPTED.into_response()
    }
}

async fn handle_batch_request(
    gs: &GatewayState,
    client_session_id: &str,
    batch: &[Value],
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

        if let Some(response) = dispatch_single_request(gs, &req, client_session_id).await {
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
) -> Option<Value> {
    let id = req.id.clone()?;
    let id_str = serde_json::to_string(&id).unwrap_or_default();

    match req.method.as_str() {
        "initialize" => Some(handle_initialize(gs, id, req).await),
        "ping" => Some(json!({"jsonrpc":"2.0","id":id,"result":{}})),
        "notifications/initialized" => Some(json!({"jsonrpc":"2.0","id":id,"result":{}})),
        "tools/list" => Some(handle_tools_list(gs, id, req).await),
        "instances/list" => Some(handle_instances_list(gs, id, req).await),
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
        "tools/call" => Some(handle_tools_call(gs, id, &id_str, req, session_id).await),
        other => Some(json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {other}")}
        })),
    }
}

async fn handle_initialize(gs: &GatewayState, id: Value, req: &JsonRpcRequest) -> Value {
    let client_version = req
        .params
        .as_ref()
        .and_then(|params| params.get("protocolVersion"))
        .and_then(|value| value.as_str());
    let negotiated = negotiate_protocol_version(client_version);
    {
        let mut protocol_version = gs.protocol_version.write().await;
        *protocol_version = Some(negotiated.to_string());
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
                 tools/list returns:\n\
                 • Gateway discovery and pooling meta-tools (list/get/connect/acquire/release DCC instance)\n\
                 • 6 skill-management tools (list/find/search/get_info/load/unload_skill)\n\
                 • Every backend tool, prefixed with an 8-char instance id\n\
                 \n\
                 Workflow:\n\
                 1. search_skills(query=...) — find relevant skills across every live DCC\n\
                 2. load_skill(skill_name=..., instance_id=... when multiple instances exist)\n\
                 3. Call the prefixed tool directly through this same endpoint\n\
                 \n\
                 Subscribe to GET /mcp (SSE) for push notifications."
        }
    })
}

async fn handle_instances_list(gs: &GatewayState, id: Value, req: &JsonRpcRequest) -> Value {
    let args = req.params.as_ref().cloned().unwrap_or_else(|| json!({}));
    match aggregator::tool_list_instances(gs, &args).await {
        Ok(text) => {
            let result =
                serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({"text": text}));
            json!({"jsonrpc": "2.0", "id": id, "result": result})
        }
        Err(message) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32000, "message": message}
        }),
    }
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
    // #732: fan-out `resources/list` to every live backend and merge the
    // results with the existing `dcc://<type>/<id>` admin pointers.
    // Fail-soft: a backend that cannot be reached contributes zero
    // entries; healthy backends' resources are still returned.
    let result = aggregator::aggregate_resources_list(gs).await;
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

async fn handle_resources_read(gs: &GatewayState, id: Value, req: &JsonRpcRequest) -> Value {
    let uri = req
        .params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();

    // #732: gateway-prefixed URIs (`<scheme>://<id8>/<rest>`) are
    // forwarded to the owning backend, preserving the raw `result`
    // payload — including `contents[].blob` entries for binary
    // mime-types — byte-for-byte.
    if let Some((id8, backend_uri)) = crate::gateway::namespace::decode_resource_uri(&uri) {
        let owning = aggregator::find_instance_by_prefix(gs, &id8).await;
        return match owning {
            Some(entry) => {
                let url = format!("http://{}:{}/mcp", entry.host, entry.port);
                match crate::gateway::backend_client::read_resource(
                    &gs.http_client,
                    &url,
                    &backend_uri,
                    gs.backend_timeout,
                )
                .await
                {
                    Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                    Err(e) => json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": {"code": -32002, "message": format!("Backend resources/read failed: {e}")}
                    }),
                }
            }
            None => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32002, "message": format!("Resource not found: {uri} (no live instance matches prefix '{id8}')")}
            }),
        };
    }

    // Fallback: the legacy `dcc://<type>/<id>` admin pointer format —
    // the gateway renders the same instance metadata it did pre-#732.
    let parts: Vec<&str> = uri.trim_start_matches("dcc://").splitn(2, '/').collect();
    let registry = gs.registry.read().await;
    let found = gs.live_instances(&registry).into_iter().find(|entry| {
        parts.len() == 2
            && entry.dcc_type == parts[0]
            && entry.instance_id.to_string().starts_with(parts[1])
    });

    match found {
        Some(entry) => {
            let detail = entry_to_json(&entry, gs.stale_timeout);
            json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "contents": [{
                        "uri":      uri,
                        "mimeType": "application/json",
                        "text":     serde_json::to_string_pretty(&detail).unwrap_or_default()
                    }]
                }
            })
        }
        None => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32002, "message": format!("Resource not found: {uri}")}
        }),
    }
}

async fn handle_resource_subscription(
    gs: &GatewayState,
    id: Value,
    req: &JsonRpcRequest,
    session_id: &str,
    subscribe: bool,
) -> Value {
    let uri = req
        .params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();

    // Always track the session-level subscription set so the legacy
    // behaviour (admin `dcc://` pointers, bookkeeping) is preserved
    // verbatim — callers pre-#732 relied on this map being authoritative.
    {
        let mut subscriptions = gs.resource_subscriptions.write().await;
        if subscribe {
            subscriptions
                .entry(session_id.to_owned())
                .or_default()
                .insert(uri.clone());
        } else if let Some(set) = subscriptions.get_mut(session_id) {
            set.remove(&uri);
        }
    }

    // #732: when the URI names a gateway-prefixed backend resource,
    // forward the subscription to the owning backend and register a
    // routing entry so the per-backend SSE loop can fan any
    // `notifications/resources/updated` frames back to this session.
    if let Some((id8, backend_uri)) = crate::gateway::namespace::decode_resource_uri(&uri) {
        let owning = aggregator::find_instance_by_prefix(gs, &id8).await;
        return match owning {
            Some(entry) => {
                let backend_url = format!("http://{}:{}/mcp", entry.host, entry.port);
                // Register the gateway-side routing table entry **before**
                // telling the backend to subscribe, so the very first
                // update the backend emits cannot race the bookkeeping.
                if subscribe {
                    gs.subscriber.bind_resource_subscription(
                        &backend_url,
                        &backend_uri,
                        session_id,
                        &uri,
                    );
                    // Ensure an SSE subscriber is running for this backend
                    // (idempotent — no-op when the periodic task already
                    // spawned one).
                    gs.subscriber.ensure_subscribed(&backend_url);
                } else {
                    gs.subscriber.unbind_resource_subscription(
                        &backend_url,
                        &backend_uri,
                        session_id,
                        &uri,
                    );
                }

                match crate::gateway::backend_client::subscribe_resource(
                    &gs.http_client,
                    &backend_url,
                    &backend_uri,
                    subscribe,
                    gs.backend_timeout,
                )
                .await
                {
                    Ok(_) => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                    Err(e) => {
                        // Forwarding failed — undo the routing entry so
                        // we do not leak a ghost subscriber. We keep the
                        // session-level bookkeeping either way; the next
                        // client-driven unsubscribe will tidy it.
                        if subscribe {
                            gs.subscriber.unbind_resource_subscription(
                                &backend_url,
                                &backend_uri,
                                session_id,
                                &uri,
                            );
                        }
                        json!({
                            "jsonrpc": "2.0", "id": id,
                            "error": {"code": -32002, "message": format!("Backend resources/{}: {e}", if subscribe { "subscribe" } else { "unsubscribe" })}
                        })
                    }
                }
            }
            None => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32002, "message": format!("Resource not found: {uri} (no live instance matches prefix '{id8}')")}
            }),
        };
    }

    // Legacy admin-pointer subscription (no backend fan-out needed —
    // the gateway itself does not emit updates for these URIs today).
    json!({"jsonrpc":"2.0","id":id,"result":{}})
}

async fn handle_tools_call(
    gs: &GatewayState,
    id: Value,
    id_str: &str,
    req: &JsonRpcRequest,
    session_id: &str,
) -> Value {
    let tool = req
        .params
        .as_ref()
        .and_then(|params| params.get("name"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let args = req
        .params
        .as_ref()
        .and_then(|params| params.get("arguments"))
        .cloned()
        .unwrap_or(json!({}));
    let meta = req
        .params
        .as_ref()
        .and_then(|params| params.get("_meta"))
        .cloned();

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

    let (text, is_error) = aggregator::route_tools_call(
        gs,
        tool,
        &args,
        meta.as_ref(),
        Some(id_str.to_string()),
        Some(session_id),
    )
    .await;

    {
        let mut pending = gs.pending_calls.write().await;
        pending.remove(id_str);
    }

    json!({
        "jsonrpc": "2.0", "id": id,
        "result": {"content": [{"type": "text", "text": text}], "isError": is_error}
    })
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
