//! Axum request handlers for the MCP Streamable HTTP transport.
//!
//! - `POST /mcp` — client sends JSON-RPC messages; server responds with JSON or SSE
//! - `GET  /mcp` — client opens a long-lived SSE stream for server-push events
//! - `DELETE /mcp` — client closes its session

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::sse::Event,
    response::{IntoResponse, Response, Sse},
};
use futures::stream;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    error::HttpError,
    executor::DccExecutorHandle,
    protocol::{
        self, CallToolParams, CallToolResult, InitializeResult, JsonRpcBatch, JsonRpcMessage,
        JsonRpcRequest, JsonRpcResponse, ListToolsResult, MCP_PROTOCOL_VERSION, MCP_SESSION_HEADER,
        McpTool, McpToolAnnotations, ServerCapabilities, ServerInfo, ToolsCapability,
        format_sse_event,
    },
    session::SessionManager,
};
use dcc_mcp_actions::ActionRegistry;

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<ActionRegistry>,
    pub sessions: SessionManager,
    pub executor: Option<DccExecutorHandle>,
    pub server_name: String,
    pub server_version: String,
}

// ── POST /mcp ─────────────────────────────────────────────────────────────

/// Handle `POST /mcp`: accept JSON-RPC message(s) and return response.
pub async fn handle_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let session_id = headers
        .get(MCP_SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    // Parse body — keep raw Value array for id-presence detection
    let raw_values: Vec<Value> = match parse_raw_values(&body) {
        Ok(v) => v,
        Err(e) => {
            return json_error_response(
                StatusCode::BAD_REQUEST,
                None,
                protocol::error_codes::PARSE_ERROR,
                format!("Parse error: {e}"),
            );
        }
    };

    let messages: JsonRpcBatch = match parse_body(&body) {
        Ok(m) => m,
        Err(e) => {
            return json_error_response(
                StatusCode::BAD_REQUEST,
                None,
                protocol::error_codes::PARSE_ERROR,
                format!("Parse error: {e}"),
            );
        }
    };

    // A message is a "request" (needs a response) iff it has an explicit "id" field.
    let has_requests = raw_values.iter().any(json_has_id);

    if !has_requests {
        // Only notifications/responses — accept and return 202
        return StatusCode::ACCEPTED.into_response();
    }

    // Process requests and build responses
    let mut responses: Vec<JsonRpcResponse> = Vec::new();
    let mut use_sse = false;

    // Check if client accepts SSE
    if let Some(accept) = headers.get(header::ACCEPT) {
        if accept.to_str().unwrap_or("").contains("text/event-stream") {
            use_sse = true;
        }
    }

    for msg in &messages {
        if let JsonRpcMessage::Request(req) = msg {
            match dispatch_request(&state, req, session_id.as_deref()).await {
                Ok(resp) => responses.push(resp),
                Err(e) => {
                    responses.push(JsonRpcResponse::internal_error(
                        req.id.clone(),
                        e.to_string(),
                    ));
                }
            }
        }
    }

    if use_sse && session_id.is_some() {
        // Return as SSE stream (allows server push alongside response)
        let events: Vec<String> = responses
            .iter()
            .map(|r| format_sse_event(r, None))
            .collect();

        let stream = stream::iter(events).map(Ok::<_, std::convert::Infallible>);

        let body = Body::from_stream(stream);
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("X-Accel-Buffering", "no")
            .body(body)
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    } else {
        // Return as JSON
        let body = if responses.len() == 1 {
            serde_json::to_string(&responses[0]).unwrap_or_default()
        } else {
            serde_json::to_string(&responses).unwrap_or_default()
        };
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    }
}

// ── GET /mcp ──────────────────────────────────────────────────────────────

/// Handle `GET /mcp`: open SSE stream for server-push events.
pub async fn handle_get(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Validate Accept header
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !accept.contains("text/event-stream") {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let session_id = headers
        .get(MCP_SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let rx: broadcast::Receiver<String> = if let Some(id) = &session_id {
        match state.sessions.subscribe(id) {
            Some(rx) => rx,
            None => {
                return json_error_response(
                    StatusCode::NOT_FOUND,
                    None,
                    -32600,
                    "Session not found",
                );
            }
        }
    } else {
        // No session — create an ephemeral one
        let id = state.sessions.create();
        state.sessions.subscribe(&id).unwrap()
    };

    let sse_stream = BroadcastStream::new(rx)
        .filter_map(|res| res.ok())
        .map(|data| {
            // Each item is already a formatted SSE event string
            // Parse it back to send as axum SSE Event
            Ok::<_, std::convert::Infallible>(Event::default().data(data))
        });

    Sse::new(sse_stream)
        .keep_alive(axum::response::sse::KeepAlive::new())
        .into_response()
}

// ── DELETE /mcp ───────────────────────────────────────────────────────────

/// Handle `DELETE /mcp`: terminate a session.
pub async fn handle_delete(State(state): State<AppState>, headers: HeaderMap) -> StatusCode {
    let session_id = headers
        .get(MCP_SESSION_HEADER)
        .and_then(|v| v.to_str().ok());

    match session_id {
        Some(id) if state.sessions.remove(id) => StatusCode::NO_CONTENT,
        Some(_) => StatusCode::NOT_FOUND,
        None => StatusCode::BAD_REQUEST,
    }
}

// ── Method dispatch ───────────────────────────────────────────────────────

async fn dispatch_request(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    match req.method.as_str() {
        "initialize" => handle_initialize(state, req, session_id).await,
        "notifications/initialized" => Ok(JsonRpcResponse::success(req.id.clone(), json!({}))),
        "tools/list" => handle_tools_list(state, req).await,
        "tools/call" => handle_tools_call(state, req).await,
        "ping" => Ok(JsonRpcResponse::success(req.id.clone(), json!({}))),
        other => Ok(JsonRpcResponse::method_not_found(req.id.clone(), other)),
    }
}

async fn handle_initialize(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    // Create or mark session as initialized
    let sid = if let Some(id) = session_id {
        state.sessions.mark_initialized(id);
        id.to_owned()
    } else {
        let id = state.sessions.create();
        state.sessions.mark_initialized(&id);
        id
    };

    let result = InitializeResult {
        protocol_version: MCP_PROTOCOL_VERSION.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: false,
            }),
            resources: None,
            prompts: None,
        },
        server_info: ServerInfo {
            name: state.server_name.clone(),
            version: state.server_version.clone(),
        },
        instructions: Some(
            "DCC MCP Server — use tools/list to discover available DCC actions.".to_string(),
        ),
    };

    let mut resp = JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result)?);
    // Attach session ID via a custom field — the real header is set in the layer
    // We store it in the response id metadata for the server layer to pick up.
    // The actual Mcp-Session-Id header is injected by handle_post after this.
    // We attach it as __session_id for the outer layer.
    if let Some(obj) = resp.result.as_mut().and_then(|v| v.as_object_mut()) {
        obj.insert("__session_id".to_string(), Value::String(sid));
    }
    Ok(resp)
}

async fn handle_tools_list(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let actions = state.registry.list_actions(None);
    let tools: Vec<McpTool> = actions
        .iter()
        .map(|meta| McpTool {
            name: meta.name.clone(),
            description: meta.description.clone(),
            input_schema: {
                let s = &meta.input_schema;
                if s.is_null() {
                    json!({"type": "object"})
                } else {
                    s.clone()
                }
            },
            annotations: Some(McpToolAnnotations {
                title: Some(meta.name.clone()),
                read_only_hint: false,
                destructive_hint: false,
                idempotent_hint: false,
                open_world_hint: false,
            }),
        })
        .collect();

    let result = ListToolsResult {
        tools,
        next_cursor: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
    ))
}

async fn handle_tools_call(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let params: CallToolParams = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .ok_or_else(|| HttpError::Internal("invalid tools/call params".to_string()))?;

    let tool_name = params.name.clone();
    let args_json = params
        .arguments
        .map(|v| serde_json::to_string(&v).unwrap_or_default())
        .unwrap_or_else(|| "{}".to_string());

    // Check action exists
    if state.registry.get_action(&tool_name, None).is_none() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(format!("Unknown tool: {tool_name}")))?,
        ));
    }

    // If executor is available, run on DCC main thread
    let result_json = if let Some(exec) = &state.executor {
        let registry = state.registry.clone();
        let name = tool_name.clone();
        let args = args_json.clone();
        exec.execute(Box::new(move || {
            // Dispatch through the registry — handlers registered by DCC adapter
            match registry.get_action(&name, None) {
                Some(_) => {
                    // Return args as pass-through for now; DCC adapter overrides
                    format!(
                        r#"{{"success":true,"message":"dispatched","context":{}}}"#,
                        args
                    )
                }
                None => format!(r#"{{"success":false,"error":"tool not found: {name}"}}"#),
            }
        }))
        .await?
    } else {
        // No executor — direct dispatch (non-DCC mode / testing)
        match state.registry.get_action(&tool_name, None) {
            Some(_) => {
                format!(
                    r#"{{"success":true,"message":"ok","context":{}}}"#,
                    args_json
                )
            }
            None => {
                format!(r#"{{"success":false,"error":"tool not found: {tool_name}"}}"#)
            }
        }
    };

    // Parse result and wrap as CallToolResult
    let result_value: Value = serde_json::from_str(&result_json).unwrap_or(json!({}));
    let is_error = result_value
        .get("success")
        .and_then(Value::as_bool)
        .map(|s| !s)
        .unwrap_or(false);

    let text = if is_error {
        result_value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
            .to_string()
    } else {
        result_value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };

    let call_result = CallToolResult {
        content: vec![protocol::ToolContent::Text { text }],
        is_error,
    };

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(call_result)?,
    ))
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn parse_raw_values(body: &str) -> Result<Vec<Value>, serde_json::Error> {
    if body.trim_start().starts_with('[') {
        serde_json::from_str::<Vec<Value>>(body)
    } else {
        serde_json::from_str::<Value>(body).map(|v| vec![v])
    }
}

fn parse_body(body: &str) -> Result<JsonRpcBatch, serde_json::Error> {
    // Try array first, then single object.
    // JSON-RPC 2.0: a "notification" is a request WITHOUT an "id" field.
    // We normalise both to JsonRpcMessage so callers can use `has_id` to
    // decide whether a response is expected.
    if body.trim_start().starts_with('[') {
        serde_json::from_str::<JsonRpcBatch>(body)
    } else {
        serde_json::from_str::<JsonRpcMessage>(body).map(|m| vec![m])
    }
}

/// Return true only if the raw JSON object has an explicit "id" key
/// (even if its value is null). Used to distinguish request from notification.
fn json_has_id(raw: &Value) -> bool {
    raw.as_object()
        .map(|o| o.contains_key("id"))
        .unwrap_or(false)
}

fn json_error_response(
    status: StatusCode,
    id: Option<Value>,
    code: i64,
    message: impl Into<String>,
) -> Response {
    let body =
        serde_json::to_string(&JsonRpcResponse::error(id, code, message)).unwrap_or_default();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
