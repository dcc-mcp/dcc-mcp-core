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
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_skills::catalog::SkillSummary;

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<ActionRegistry>,
    pub dispatcher: Arc<ActionDispatcher>,
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
        state
            .sessions
            .subscribe(&id)
            .expect("subscribe on a freshly created session cannot fail")
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
    // Refresh session TTL on every request so active sessions are not evicted.
    if let Some(id) = session_id {
        state.sessions.touch(id);
    }
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
            "DCC MCP Server — on-demand skill discovery workflow:\n\
             1. Use search_skills(query) to find relevant skills by keyword.\n\
             2. Use load_skill(skill_name) to activate a skill and register its full tool schemas.\n\
             3. After loading, tools/list will include the skill's tools with complete input schemas.\n\
             4. Unloaded skills appear as __skill__<name> stubs in tools/list (name + description only).\n\
             5. Use list_skills/find_skills for broader discovery; get_skill_info for full metadata."
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
    // 1. Core discovery tools — always fully visible
    let mut tools: Vec<McpTool> = build_core_tools();

    // 2. Loaded skill tools — full definitions from ActionRegistry
    let actions = state.registry.list_actions(None);
    for meta in &actions {
        tools.push(action_meta_to_mcp_tool(meta));
    }

    // 3. Unloaded skills — one lightweight stub per skill.
    //    The stub lets the model see what skills exist and what tools they expose
    //    without flooding the context with full input schemas.
    //    Format: name="__skill__<skill_name>", description summarises tools,
    //    input_schema is a minimal passthrough (use load_skill to get full tools).
    let unloaded = state.catalog.list_skills(Some("unloaded"));
    for summary in &unloaded {
        tools.push(build_skill_stub(summary));
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
        "search_skills" => return handle_search_skills(state, req, &params).await,
        _ => {}
    }

    // Skill stub: __skill__<name> — guide model to call load_skill first
    if let Some(skill_name) = tool_name.strip_prefix("__skill__") {
        let msg = format!(
            "Skill '{skill_name}' is not loaded. Call load_skill with skill_name=\"{skill_name}\" \
             to register its tools, then call the specific tool you need."
        );
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(msg))?,
        ));
    }

    // Resolve action params (default to empty object)
    let call_params = params.arguments.unwrap_or(json!({}));

    // Check action exists in registry before dispatch
    if state.registry.get_action(&tool_name, None).is_none() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(format!("Unknown tool: {tool_name}")))?,
        ));
    }

    // Dispatch via ActionDispatcher
    let dispatch_outcome = if let Some(exec) = &state.executor {
        // DCC main-thread path: run synchronous dispatch inside DeferredExecutor
        let dispatcher = state.dispatcher.clone();
        let name = tool_name.clone();
        let p = call_params.clone();
        exec.execute(Box::new(move || {
            match dispatcher.dispatch(&name, p) {
                Ok(r) => serde_json::to_string(&r.output).unwrap_or_else(|_| "null".to_string()),
                Err(e) => {
                    // Encode error in a sentinel JSON object
                    let err_obj = json!({"__dispatch_error": e.to_string()});
                    serde_json::to_string(&err_obj).unwrap_or_default()
                }
            }
        }))
        .await
        .map(|json_str| {
            let v: Value = serde_json::from_str(&json_str).unwrap_or(json!({}));
            if let Some(err) = v.get("__dispatch_error") {
                Err(err.as_str().unwrap_or("dispatch error").to_string())
            } else {
                Ok(v)
            }
        })
        .unwrap_or_else(|e| Err(e.to_string()))
    } else {
        // Non-DCC path: use spawn_blocking so we don't block the async runtime
        let dispatcher = state.dispatcher.clone();
        let name = tool_name.clone();
        let p = call_params.clone();
        tokio::task::spawn_blocking(move || dispatcher.dispatch(&name, p))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map(|d| d.output).map_err(|e| e.to_string()))
    };

    // Build MCP CallToolResult from dispatch outcome
    let call_result = match dispatch_outcome {
        Ok(output) => {
            let text = match &output {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
            };
            CallToolResult {
                content: vec![protocol::ToolContent::Text { text }],
                is_error: false,
            }
        }
        Err(err_msg) => {
            // "no handler registered" means tool exists in registry but has no
            // callable handler — guide the user to register one.
            let text = if err_msg.contains("no handler registered") {
                format!(
                    "Tool '{tool_name}' is registered but has no handler. \
                     Register a handler via ActionDispatcher.register_handler()."
                )
            } else {
                err_msg
            };
            CallToolResult {
                content: vec![protocol::ToolContent::Text { text }],
                is_error: true,
            }
        }
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
        McpTool {
            name: "search_skills".to_string(),
            description: "Search for skills by keyword. Matches against skill name, description, \
                          search_hint, and tool names. Returns matching skills with a one-line \
                          summary. Use load_skill to activate a skill and get its full tool schemas."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword to search in skill name, description, search_hint, and tool names"
                    },
                    "dcc": {
                        "type": "string",
                        "description": "Optional DCC filter (maya, blender, houdini, etc.)"
                    }
                },
                "required": ["query"]
            }),
            annotations: Some(McpToolAnnotations {
                title: Some("Search Skills".to_string()),
                read_only_hint: true,
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

/// Build a lightweight stub McpTool for an unloaded skill.
///
/// The stub is surfaced in `tools/list` so the model knows the skill exists
/// and what tools it contains — without emitting full input schemas.
/// When called, the stub responds with a hint to call `load_skill` first.
///
/// Name format: `__skill__<skill_name>`
fn build_skill_stub(summary: &SkillSummary) -> McpTool {
    let tool_list = if summary.tool_names.is_empty() {
        String::new()
    } else {
        format!(" Tools: {}.", summary.tool_names.join(", "))
    };
    let hint = if summary.search_hint.is_empty() || summary.search_hint == summary.description {
        String::new()
    } else {
        format!(" Keywords: {}.", summary.search_hint)
    };
    let description = format!(
        "[Skill: {}] {}{} \u{2022} {} tools available. \
         Call load_skill(skill_name=\"{}\") to activate full tool schemas.",
        summary.name, summary.description, hint, summary.tool_count, summary.name,
    );
    // Append tool list to description if not too long
    let description = if tool_list.is_empty() || description.len() + tool_list.len() < 512 {
        format!("{}{}", description, tool_list)
    } else {
        description
    };

    McpTool {
        name: format!("__skill__{}", summary.name),
        description,
        input_schema: json!({"type": "object", "properties": {}}),
        annotations: Some(McpToolAnnotations {
            title: Some(format!("Skill: {}", summary.name)),
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        }),
    }
}

/// Handle `search_skills` tool call.
///
/// Searches skill name, description, search_hint, and tool names.
/// Returns a compact list: one line per matching skill.
async fn handle_search_skills(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let query = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("query"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let dcc_filter = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("dcc"))
        .and_then(Value::as_str);

    if query.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error("Missing required parameter: query"))?,
        ));
    }

    let matches = state.catalog.find_skills(Some(query), &[], dcc_filter);

    if matches.is_empty() {
        let text = format!("No skills found matching '{query}'.");
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::text(text))?,
        ));
    }

    // Return compact one-line-per-skill summary
    let lines: Vec<String> = matches
        .iter()
        .map(|s| {
            let status = if s.loaded { "loaded" } else { "unloaded" };
            let tools = s.tool_names.join(", ");
            format!(
                "- {} [{}] ({} tools: {}) — {}",
                s.name, status, s.tool_count, tools, s.description
            )
        })
        .collect();

    let text = format!(
        "Found {} skill(s) matching '{}':\n{}",
        matches.len(),
        query,
        lines.join("\n")
    );

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(text))?,
    ))
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
