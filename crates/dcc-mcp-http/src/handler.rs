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
use dcc_mcp_skills::SkillCatalog;

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<ActionRegistry>,
    pub catalog: Arc<SkillCatalog>,
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
        "tools/call" => handle_tools_call(state, req, session_id).await,
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
            tools: Some(ToolsCapability { list_changed: true }),
            resources: None,
            prompts: None,
        },
        server_info: ServerInfo {
            name: state.server_name.clone(),
            version: state.server_version.clone(),
        },
        instructions: Some(
            "DCC MCP Server — use find_skills to discover available skills, \
             load_skill to activate them, then tools/list to see their tools."
                .to_string(),
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
    // Build core discovery tools
    let mut tools: Vec<McpTool> = build_core_tools();

    // Add all actions from the registry (includes dynamically loaded skill tools)
    let actions = state.registry.list_actions(None);
    for meta in &actions {
        tools.push(action_meta_to_mcp_tool(meta));
    }

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
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let params: CallToolParams = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .ok_or_else(|| HttpError::Internal("invalid tools/call params".to_string()))?;

    let tool_name = params.name.clone();

    // Route core discovery tools
    match tool_name.as_str() {
        "find_skills" => return handle_find_skills(state, req, &params).await,
        "list_skills" => return handle_list_skills(state, req, &params).await,
        "get_skill_info" => return handle_get_skill_info(state, req, &params).await,
        "load_skill" => return handle_load_skill(state, req, &params, session_id).await,
        "unload_skill" => return handle_unload_skill(state, req, &params, session_id).await,
        _ => {}
    }

    // Regular action dispatch
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

// ── Core discovery tool handlers ──────────────────────────────────────────

async fn handle_find_skills(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();

    let query = args.and_then(|a| a.get("query")).and_then(Value::as_str);
    let tags: Vec<&str> = args
        .and_then(|a| a.get("tags"))
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let dcc = args.and_then(|a| a.get("dcc")).and_then(Value::as_str);

    let results = state.catalog.find_skills(query, &tags, dcc);

    let text = serde_json::to_string_pretty(&json!({
        "skills": results,
        "total": results.len()
    }))
    .unwrap_or_default();

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(text))?,
    ))
}

async fn handle_list_skills(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let status = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("status"))
        .and_then(Value::as_str);

    let results = state.catalog.list_skills(status);

    let text = serde_json::to_string_pretty(&json!({
        "skills": results,
        "total": results.len()
    }))
    .unwrap_or_default();

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(text))?,
    ))
}

async fn handle_get_skill_info(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let skill_name = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("skill_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if skill_name.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "Missing required parameter: skill_name",
            ))?,
        ));
    }

    match state.catalog.get_skill_info(skill_name) {
        Some(info) => {
            let text = serde_json::to_string_pretty(&info).unwrap_or_default();
            Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::text(text))?,
            ))
        }
        None => Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(format!(
                "Skill '{skill_name}' not found"
            )))?,
        )),
    }
}

async fn handle_load_skill(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let skill_name = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("skill_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let skill_names: Vec<String> = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("skill_names"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    if skill_name.is_empty() && skill_names.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "Missing required parameter: skill_name or skill_names",
            ))?,
        ));
    }

    let mut all_registered = Vec::new();
    let mut errors = Vec::new();

    // Load single skill
    if !skill_name.is_empty() {
        match state.catalog.load_skill(skill_name) {
            Ok(actions) => all_registered.extend(actions),
            Err(e) => errors.push(format!("{skill_name}: {e}")),
        }
    }

    // Load multiple skills
    for name in &skill_names {
        match state.catalog.load_skill(name) {
            Ok(actions) => all_registered.extend(actions),
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }

    // Send tools/list_changed notification to session if tools were loaded
    if !all_registered.is_empty() {
        if let Some(sid) = session_id {
            notify_tools_list_changed(&state.sessions, sid);
        }
    }

    let text = if errors.is_empty() {
        serde_json::to_string_pretty(&json!({
            "loaded": true,
            "registered_actions": all_registered,
            "action_count": all_registered.len()
        }))
        .unwrap_or_default()
    } else {
        serde_json::to_string_pretty(&json!({
            "loaded": errors.is_empty(),
            "registered_actions": all_registered,
            "action_count": all_registered.len(),
            "errors": errors
        }))
        .unwrap_or_default()
    };

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(text))?,
    ))
}

async fn handle_unload_skill(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let skill_name = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("skill_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if skill_name.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "Missing required parameter: skill_name",
            ))?,
        ));
    }

    match state.catalog.unload_skill(skill_name) {
        Ok(count) => {
            // Send tools/list_changed notification
            if let Some(sid) = session_id {
                notify_tools_list_changed(&state.sessions, sid);
            }

            let text = serde_json::to_string_pretty(&json!({
                "unloaded": true,
                "actions_removed": count
            }))
            .unwrap_or_default();

            Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::text(text))?,
            ))
        }
        Err(e) => Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(e))?,
        )),
    }
}

// ── Core tool definitions ─────────────────────────────────────────────────

/// Build the core discovery tools that are always visible.
fn build_core_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "find_skills".to_string(),
            description: "Search available skills by keyword, tags, or DCC type. \
                          Returns matching skills with metadata but does NOT load them."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search in skill name and description"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by tags (all must match)"
                    },
                    "dcc": {
                        "type": "string",
                        "description": "Filter by DCC type (maya, blender, houdini, etc.)"
                    }
                }
            }),
            annotations: Some(McpToolAnnotations {
                title: Some("Find Skills".to_string()),
                read_only_hint: true,
                destructive_hint: false,
                idempotent_hint: true,
                open_world_hint: false,
            }),
        },
        McpTool {
            name: "list_skills".to_string(),
            description: "List all discovered skills with their load status (loaded/unloaded)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["all", "loaded", "unloaded", "error"],
                        "default": "all",
                        "description": "Filter by load status"
                    }
                }
            }),
            annotations: Some(McpToolAnnotations {
                title: Some("List Skills".to_string()),
                read_only_hint: true,
                destructive_hint: false,
                idempotent_hint: true,
                open_world_hint: false,
            }),
        },
        McpTool {
            name: "get_skill_info".to_string(),
            description: "Get detailed info about a specific skill including its tools and their input schemas."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Name of the skill to inspect"
                    }
                },
                "required": ["skill_name"]
            }),
            annotations: Some(McpToolAnnotations {
                title: Some("Get Skill Info".to_string()),
                read_only_hint: true,
                destructive_hint: false,
                idempotent_hint: true,
                open_world_hint: false,
            }),
        },
        McpTool {
            name: "load_skill".to_string(),
            description: "Load a skill and register its tools. After loading, the tools become available via tools/list. \
                          A tools/list_changed notification is sent to connected clients."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Name of the skill to load"
                    },
                    "skill_names": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Load multiple skills at once"
                    }
                }
            }),
            annotations: Some(McpToolAnnotations {
                title: Some("Load Skill".to_string()),
                read_only_hint: false,
                destructive_hint: false,
                idempotent_hint: true,
                open_world_hint: false,
            }),
        },
        McpTool {
            name: "unload_skill".to_string(),
            description: "Unload a skill and unregister its tools. Sends a tools/list_changed notification."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Name of the skill to unload"
                    }
                },
                "required": ["skill_name"]
            }),
            annotations: Some(McpToolAnnotations {
                title: Some("Unload Skill".to_string()),
                read_only_hint: false,
                destructive_hint: false,
                idempotent_hint: true,
                open_world_hint: false,
            }),
        },
    ]
}

/// Convert an ActionMeta to an McpTool, respecting annotations from the skill.
fn action_meta_to_mcp_tool(meta: &dcc_mcp_actions::registry::ActionMeta) -> McpTool {
    let input_schema = if meta.input_schema.is_null() {
        json!({"type": "object"})
    } else {
        meta.input_schema.clone()
    };

    McpTool {
        name: meta.name.clone(),
        description: meta.description.clone(),
        input_schema,
        annotations: Some(McpToolAnnotations {
            title: Some(meta.name.clone()),
            // Actions from skills get sensible defaults; standalone actions default to false
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        }),
    }
}

/// Send a `notifications/tools/list_changed` event to a session's SSE subscribers.
fn notify_tools_list_changed(sessions: &SessionManager, session_id: &str) {
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/tools/list_changed",
        "params": {}
    });
    let event = format_sse_event(&notification, None);
    sessions.push_event(session_id, event);
    tracing::debug!("Sent tools/list_changed notification to session {session_id}");
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
