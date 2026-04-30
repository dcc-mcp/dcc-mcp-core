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
                "resources": {"listChanged": true, "subscribe": true}
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
    let registry = gs.registry.read().await;
    let resources: Vec<Value> = gs
        .live_instances(&registry)
        .into_iter()
        .filter(|entry| entry.dcc_type != "__gateway__")
        .map(|entry| {
            let name = match entry.scene.as_deref() {
                Some(scene) if !scene.is_empty() => {
                    format!(
                        "{} — {} ({}:{})",
                        entry.dcc_type, scene, entry.host, entry.port
                    )
                }
                _ => format!("{} @ {}:{}", entry.dcc_type, entry.host, entry.port),
            };
            json!({
                "uri":         format!("dcc://{}/{}", entry.dcc_type, entry.instance_id),
                "name":        name,
                "description": format!("Live {} DCC instance. Version: {}.",
                    entry.dcc_type,
                    entry.version.as_deref().unwrap_or("unknown")),
                "mimeType":    "application/json"
            })
        })
        .collect();
    json!({"jsonrpc":"2.0","id":id,"result":{"resources": resources}})
}

async fn handle_resources_read(gs: &GatewayState, id: Value, req: &JsonRpcRequest) -> Value {
    let uri = req
        .params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
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
    let mut subscriptions = gs.resource_subscriptions.write().await;
    if subscribe {
        subscriptions
            .entry(session_id.to_owned())
            .or_default()
            .insert(uri);
    } else if let Some(set) = subscriptions.get_mut(session_id) {
        set.remove(&uri);
    }
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
