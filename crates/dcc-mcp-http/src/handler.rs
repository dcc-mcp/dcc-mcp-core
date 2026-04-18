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
use dashmap::DashMap;
use futures::stream;
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    bridge_registry::BridgeRegistry,
    error::HttpError,
    executor::DccExecutorHandle,
    inflight::{CancelToken, InFlightEntry, InFlightRequests, ProgressReporter},
    protocol::{
        self, CallToolParams, CallToolResult, DELTA_TOOLS_METHOD, DELTA_TOOLS_UPDATE_CAP,
        InitializeResult, JsonRpcBatch, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse,
        ListToolsResult, MCP_SESSION_HEADER, McpTool, McpToolAnnotations, ServerCapabilities,
        ServerInfo, TOOLS_LIST_PAGE_SIZE, ToolsCapability, decode_cursor, encode_cursor,
        format_sse_event, negotiate_protocol_version,
    },
    session::SessionManager,
};
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_protocols::DccMcpError;
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_skills::catalog::SkillSummary;

/// How long a cancellation record is kept before being garbage-collected.
///
/// If a client sends `notifications/cancelled` for a request that has already
/// completed (common race condition), the entry would never be consumed by the
/// check in `handle_tools_call`.  This TTL bounds memory growth from such entries.
const CANCELLED_REQUEST_TTL: Duration = Duration::from_secs(30);

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<ActionRegistry>,
    pub dispatcher: Arc<ActionDispatcher>,
    pub catalog: Arc<SkillCatalog>,
    pub sessions: SessionManager,
    pub executor: Option<DccExecutorHandle>,
    pub bridge_registry: BridgeRegistry,
    pub server_name: String,
    pub server_version: String,
    /// Tracks request IDs that have been cancelled by the client via
    /// `notifications/cancelled`.
    ///
    /// Value is the `Instant` when the cancellation was recorded, used to
    /// garbage-collect entries that are never consumed (e.g. because the tool
    /// call already completed before the cancellation arrived).  A background
    /// task in `McpHttpServer::start()` runs `purge_expired_cancellations()`
    /// every 60 seconds to keep this map bounded.
    pub cancelled_requests: Arc<DashMap<String, Instant>>,
    pub in_flight: InFlightRequests,
}

impl AppState {
    /// Remove cancellation entries older than [`CANCELLED_REQUEST_TTL`].
    ///
    /// Call this from a background task to prevent unbounded memory growth when
    /// clients cancel requests that have already completed.
    pub fn purge_expired_cancellations(&self) {
        self.cancelled_requests
            .retain(|_, recorded_at| recorded_at.elapsed() < CANCELLED_REQUEST_TTL);
    }
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

    // Always process notifications (fire-and-forget — no id) so that
    // `notifications/cancelled` can abort in-flight tool calls.
    for msg in &messages {
        if let JsonRpcMessage::Notification(notif) = msg {
            handle_notification(&state, &notif.method, notif.params.as_ref()).await;
        }
    }

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

// ── Notification handling ─────────────────────────────────────────────────

/// Process a JSON-RPC notification (a message without an `id`).
///
/// Notifications are fire-and-forget; the server must never reply to them.
/// The main notification of interest is `notifications/cancelled`, which
/// records that the client no longer needs the result of a previous request.
async fn handle_notification(state: &AppState, method: &str, params: Option<&Value>) {
    match method {
        "notifications/cancelled" => {
            // Extract the `requestId` field (string or number)
            let id_str = params.and_then(|p| p.get("requestId")).map(|v| match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                other => serde_json::to_string(other).unwrap_or_default(),
            });

            if let Some(id) = id_str {
                if !id.is_empty() {
                    tracing::info!(request_id = %id, "MCP request cancelled by client");
                    state.cancelled_requests.insert(id.clone(), Instant::now());
                    if state.in_flight.request_cancel(&id) {
                        tracing::debug!(request_id = %id, "cancel flag set on in-flight request");
                    }
                }
            }
        }
        // Already handled as a request-shaped message; safe to ignore here.
        "notifications/initialized" => {}
        other => {
            tracing::debug!(method = other, "ignoring unknown MCP notification");
        }
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

    // Negotiate protocol version: honour client's preference if we support it,
    // otherwise fall back to our latest supported version.
    let client_version = req
        .params
        .as_ref()
        .and_then(|p| p.get("protocolVersion"))
        .and_then(|v| v.as_str());
    let negotiated = negotiate_protocol_version(client_version);

    // Store the negotiated version on the session for later handlers.
    state.sessions.set_protocol_version(&sid, negotiated);

    // Negotiate vendored delta-tools capability.
    let client_wants_delta = req
        .params
        .as_ref()
        .and_then(|p| p.get("capabilities"))
        .and_then(|c| c.get("experimental"))
        .and_then(|e| e.get(DELTA_TOOLS_UPDATE_CAP))
        .and_then(|d| d.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    state
        .sessions
        .set_supports_delta_tools(&sid, client_wants_delta);

    let experimental_caps = if client_wants_delta {
        Some(json!({ DELTA_TOOLS_UPDATE_CAP: { "enabled": true } }))
    } else {
        None
    };

    let result = InitializeResult {
        protocol_version: negotiated.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: true }),
            resources: None,
            prompts: None,
            experimental: experimental_caps,
        },
        server_info: ServerInfo {
            name: state.server_name.clone(),
            version: state.server_version.clone(),
        },
        instructions: Some(
            "Search skills with search_skills(query), load with load_skill(name). See get_skill_info or tools/list for details."
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
    // 1. Core discovery tools — always fully visible (static, cached once per process)
    let core = build_core_tools();
    let mut tools: Vec<McpTool> = Vec::with_capacity(core.len() + 16);
    tools.extend_from_slice(core);

    // 2. Loaded skill tools — full definitions from ActionRegistry.
    //    Tools in inactive groups are collapsed into one ``__group__<name>``
    //    stub per group to keep ``tools/list`` compact (progressive exposure).
    let actions = state.registry.list_actions(None);
    let mut inactive_groups: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for meta in &actions {
        if meta.enabled {
            tools.push(action_meta_to_mcp_tool(meta));
        } else if !meta.group.is_empty() {
            inactive_groups
                .entry(meta.group.clone())
                .or_default()
                .push(meta.name.clone());
        }
    }
    for (group, names) in &inactive_groups {
        tools.push(build_group_stub(group, names));
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

    // Cursor pagination
    let cursor: usize = req
        .params
        .as_ref()
        .and_then(|p| p.get("cursor"))
        .and_then(|v| v.as_str())
        .and_then(decode_cursor)
        .unwrap_or(0);
    let total = tools.len();
    let page_end = (cursor + TOOLS_LIST_PAGE_SIZE).min(total);
    let page: Vec<McpTool> = if cursor < total {
        tools.drain(cursor..page_end).collect()
    } else {
        Vec::new()
    };
    let next_cursor = if page_end < total {
        Some(encode_cursor(page_end))
    } else {
        None
    };
    let result = ListToolsResult {
        tools: page,
        next_cursor,
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
        "activate_tool_group" => {
            return handle_activate_tool_group(state, req, &params, session_id).await;
        }
        "deactivate_tool_group" => {
            return handle_deactivate_tool_group(state, req, &params, session_id).await;
        }
        "search_tools" => return handle_search_tools(state, req, &params).await,
        _ => {}
    }

    // Skill stub: __skill__<name> — guide model to call load_skill first
    if let Some(skill_name) = tool_name.strip_prefix("__skill__") {
        let envelope = DccMcpError::new(
            "gateway",
            "SKILL_NOT_LOADED",
            format!("Skill '{skill_name}' is not loaded."),
        )
        .with_hint(format!(
            "Call load_skill with skill_name=\"{skill_name}\" to register its tools, \
             then call the specific tool you need."
        ));
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    }

    // Group stub: __group__<group_name> — guide model to call activate_tool_group.
    if let Some(group_name) = tool_name.strip_prefix("__group__") {
        let envelope = DccMcpError::new(
            "gateway",
            "GROUP_NOT_ACTIVATED",
            format!("Tool group '{group_name}' is inactive."),
        )
        .with_hint(format!(
            "Call activate_tool_group with group=\"{group_name}\" to enable its tools, \
             then re-list with tools/list."
        ));
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    }

    // Resolve action params (default to empty object)
    let call_params = params.arguments.unwrap_or(json!({}));

    // Check action exists in registry before dispatch
    if state.registry.get_action(&tool_name, None).is_none() {
        let envelope = DccMcpError::new(
            "registry",
            "ACTION_NOT_FOUND",
            format!("Unknown tool: {tool_name}"),
        )
        .with_hint(
            "Use tools/list to see available tools, or load a skill first with load_skill."
                .to_string(),
        );
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    }

    // ── Register in-flight entry (#240 progress + #241 cancellation) ─────
    let req_id_str: Option<String> = req.id.as_ref().map(|id| match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    });

    let progress_token = params.meta.as_ref().and_then(|m| m.progress_token.clone());
    let cancel_token = CancelToken::new();
    let progress_reporter = ProgressReporter::new(
        progress_token.clone(),
        session_id.map(str::to_owned),
        state.sessions.clone(),
        req_id_str.clone().unwrap_or_default(),
    );

    if let Some(ref rid) = req_id_str {
        let entry = InFlightEntry::new(cancel_token.clone(), progress_reporter.clone());
        state.in_flight.insert(rid.clone(), entry);
        tracing::debug!(
            request_id = %rid,
            has_progress_token = progress_token.is_some(),
            "registered in-flight request"
        );
    }

    // ── Pre-dispatch early-cancel check ───────────────────────────────────
    if let Some(ref rid) = req_id_str {
        let already_cancelled = state
            .cancelled_requests
            .get(rid)
            .is_some_and(|ts| ts.elapsed() < CANCELLED_REQUEST_TTL);
        if already_cancelled {
            state.in_flight.remove(rid);
            state.cancelled_requests.remove(rid);
            tracing::info!(request_id = %rid, "request cancelled before dispatch");
            let envelope = DccMcpError::new(
                "registry",
                "CANCELLED",
                format!("Request {rid} was cancelled before dispatch."),
            )
            .with_hint("Re-send the request if you still need the result.");
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
            ));
        }
    }

    // Dispatch — cancel token is checked before entering the action.
    let cancel_token_for_dispatch = cancel_token.clone();
    let dispatch_outcome = if let Some(exec) = &state.executor {
        // DCC main-thread path
        let dispatcher = state.dispatcher.clone();
        let name = tool_name.clone();
        let p = call_params.clone();
        let ct = cancel_token_for_dispatch;
        exec.execute(Box::new(move || {
            if ct.is_cancelled() {
                return serde_json::to_string(&json!({"__dispatch_error": "CANCELLED"}))
                    .unwrap_or_default();
            }
            match dispatcher.dispatch(&name, p) {
                Ok(r) => serde_json::to_string(&r.output).unwrap_or_else(|_| "null".to_string()),
                Err(e) => serde_json::to_string(&json!({"__dispatch_error": e.to_string()}))
                    .unwrap_or_default(),
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
        // Non-DCC path: spawn_blocking with cooperative cancel monitor.
        let dispatcher = state.dispatcher.clone();
        let name = tool_name.clone();
        let p = call_params.clone();
        let ct_for_block = cancel_token_for_dispatch.clone();
        let dispatch_fut = tokio::task::spawn_blocking(move || {
            if ct_for_block.is_cancelled() {
                return Err("CANCELLED".to_string());
            }
            dispatcher
                .dispatch(&name, p)
                .map(|r| r.output)
                .map_err(|e| e.to_string())
        });
        tokio::select! {
            result = dispatch_fut => { result.map_err(|e| e.to_string()).and_then(|r| r) }
            _ = async {
                let deadline = tokio::time::Instant::now() + crate::inflight::CANCEL_GRACE_PERIOD;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    if cancel_token_for_dispatch.is_cancelled() || tokio::time::Instant::now() >= deadline { break; }
                }
            } => { Err("CANCELLED".to_string()) }
        }
    };

    if let Some(ref rid) = req_id_str {
        state.in_flight.remove(rid);
    }

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
        Err(err_msg) if err_msg == "CANCELLED" => {
            let rid = req_id_str.as_deref().unwrap_or("unknown");
            tracing::info!(request_id = %rid, "tool call cancelled cooperatively");
            if let Some(ref r) = req_id_str {
                state.cancelled_requests.remove(r);
            }
            let envelope = DccMcpError::new(
                "registry",
                "CANCELLED",
                format!("Request {rid} was cancelled by the client."),
            )
            .with_hint("Re-send the request if you still need the result.");
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
            ));
        }
        Err(err_msg) => {
            let envelope = if err_msg.contains("no handler registered") {
                DccMcpError::new(
                    "instance",
                    "NO_HANDLER",
                    format!("Tool '{tool_name}' is registered but has no handler."),
                )
                .with_hint("Register a handler via ActionDispatcher.register_handler().")
            } else {
                DccMcpError::new("instance", "EXECUTION_FAILED", &err_msg)
            };
            CallToolResult {
                content: vec![protocol::ToolContent::Text {
                    text: envelope.to_json(),
                }],
                is_error: true,
            }
        }
    };

    if let Some(ref rid) = req_id_str {
        let cancelled = state
            .cancelled_requests
            .remove(rid)
            .is_some_and(|(_, recorded_at)| recorded_at.elapsed() < CANCELLED_REQUEST_TTL);
        if cancelled {
            tracing::info!(request_id = %rid, "Suppressing result — request was cancelled");
            let envelope = DccMcpError::new(
                "gateway",
                "REQUEST_CANCELLED",
                format!("Request {rid} was cancelled by the client."),
            )
            .with_hint("Re-send the request if you still need the result.");
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
            ));
        }
    }

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

    // Collect the full set of requested skills, deduping `skill_name` vs the
    // `skill_names` array so callers passing both don't trigger the work twice.
    let mut requested: Vec<String> = Vec::new();
    if !skill_name.is_empty() {
        requested.push(skill_name.to_string());
    }
    for name in &skill_names {
        if !requested.contains(name) {
            requested.push(name.clone());
        }
    }

    let mut all_registered_tools: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut newly_loaded: Vec<String> = Vec::new();
    let mut already_loaded: Vec<String> = Vec::new();

    for name in &requested {
        let was_loaded = state.catalog.is_loaded(name);
        match state.catalog.load_skill(name) {
            Ok(tools) => {
                all_registered_tools.extend(tools);
                if was_loaded {
                    already_loaded.push(name.clone());
                } else {
                    newly_loaded.push(name.clone());
                }
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }

    // Only notify when a skill actually transitioned to loaded.
    if !newly_loaded.is_empty() {
        if let Some(sid) = session_id {
            let added = all_registered_tools.clone();
            let removed: Vec<String> = newly_loaded
                .iter()
                .map(|n| format!("__skill__{n}"))
                .collect();
            notify_tools_changed(&state.sessions, sid, &added, &removed);
        }
    }

    // Build the full tool metadata so agents can invoke the new tools without
    // a second round-trip to `tools/list`.  One registry read per newly or
    // previously loaded skill; keeps the payload self-contained.
    let mut tool_schemas: Vec<Value> = Vec::new();
    for name in newly_loaded.iter().chain(already_loaded.iter()) {
        for meta in state.catalog.registry().list_actions_by_skill(name) {
            tool_schemas.push(json!({
                "name":          meta.name,
                "description":   meta.description,
                "inputSchema":   meta.input_schema,
                "outputSchema":  meta.output_schema,
                "skill_name":    meta.skill_name,
            }));
        }
    }

    // Response semantics:
    // - `loaded` is true when at least one requested skill ended up loaded
    //   (even if some others failed). This matches the caller's intuition
    //   that "tools were registered" rather than treating any failure as total.
    // - `partial` distinguishes mixed success/failure from clean success.
    let loaded_ok = !all_registered_tools.is_empty();
    let partial = loaded_ok && !errors.is_empty();

    let mut body = json!({
        "loaded":           loaded_ok,
        "partial":          partial,
        "registered_tools": all_registered_tools,
        "tool_count":       all_registered_tools.len(),
        "newly_loaded":     newly_loaded,
        "already_loaded":   already_loaded,
        "tools":            tool_schemas,
    });
    if !errors.is_empty() {
        body.as_object_mut()
            .unwrap()
            .insert("errors".to_string(), json!(errors));
    }

    let text = serde_json::to_string_pretty(&body).unwrap_or_default();

    // `isError` only when every requested skill failed — partial success is
    // still reported as success so clients see the registered-tool list.
    let result = if loaded_ok {
        CallToolResult::text(text)
    } else {
        CallToolResult::error(text)
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
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
            if let Some(sid) = session_id {
                let removed: Vec<String> = state
                    .registry
                    .list_actions_by_skill(skill_name)
                    .iter()
                    .map(|m| m.name.clone())
                    .collect();
                let added = vec![format!("__skill__{skill_name}")];
                notify_tools_changed(&state.sessions, sid, &added, &removed);
            }

            let text = serde_json::to_string_pretty(&json!({
                "unloaded": true,
                "tools_removed": count
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

/// Process-global cache for the core discovery tools.
///
/// The core tools (`find_skills`, `load_skill`, `unload_skill`, `get_skill_info`,
/// `search_skills`) have static schemas that never change at runtime.  We build
/// them once on the first `tools/list` call and reuse the result for every
/// subsequent request, eliminating a handful of `String::from` / `json!` allocations
/// per request.
static CORE_TOOLS_CACHE: OnceLock<Vec<McpTool>> = OnceLock::new();

/// Return the core discovery tools, building and caching them on the first call.
fn build_core_tools() -> &'static [McpTool] {
    CORE_TOOLS_CACHE.get_or_init(build_core_tools_inner)
}

/// Inner builder — called exactly once per process lifetime.
fn build_core_tools_inner() -> Vec<McpTool> {
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
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
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
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
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
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
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
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
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
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
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
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
        },
        McpTool {
            name: "activate_tool_group".to_string(),
            description: "Activate a tool group so its tools become callable. \
                          Tools in inactive groups are collapsed into __group__<name> stubs. \
                          Sends a tools/list_changed notification on success."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "Name of the tool group to activate"
                    }
                },
                "required": ["group"]
            }),
            annotations: Some(McpToolAnnotations {
                title: Some("Activate Tool Group".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
        },
        McpTool {
            name: "deactivate_tool_group".to_string(),
            description: "Deactivate a tool group — its tools become uncallable until reactivated. \
                          Useful to reduce the active tool surface for token budget reasons."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "Name of the tool group to deactivate"
                    }
                },
                "required": ["group"]
            }),
            annotations: Some(McpToolAnnotations {
                title: Some("Deactivate Tool Group".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
        },
        McpTool {
            name: "search_tools".to_string(),
            description: "Full-text search across every registered tool (name/description/tags). \
                          Matches against enabled tools first and includes group stubs when relevant."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword to match in tool name, description, category, or tags"
                    },
                    "dcc": {
                        "type": "string",
                        "description": "Optional DCC filter"
                    },
                    "include_disabled": {
                        "type": "boolean",
                        "default": false,
                        "description": "Include tools inside inactive groups"
                    }
                },
                "required": ["query"]
            }),
            annotations: Some(McpToolAnnotations {
                title: Some("Search Tools".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
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
            read_only_hint: Some(false),
            destructive_hint: Some(false),
            idempotent_hint: Some(false),
            open_world_hint: Some(false),
            deferred_hint: Some(false),
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
    // When an explicit search-hint was provided in SKILL.md, surface it in the
    // stub description so the agent can match skills by keyword without an
    // extra round-trip.  The hint is considered explicit when it differs from
    // the description (the catalog falls back to description when no hint is
    // set).  When no explicit hint exists, keep the compact tool-name preview.
    let has_explicit_hint =
        !summary.search_hint.is_empty() && summary.search_hint != summary.description;

    let description = if has_explicit_hint {
        format!(
            "[{}] {} tools • keywords: {} • Call load_skill(\"{}\")",
            summary.dcc, summary.tool_count, summary.search_hint, summary.name
        )
    } else {
        const PREVIEW_LIMIT: usize = 5;
        let preview = if summary.tool_names.is_empty() {
            String::new()
        } else if summary.tool_names.len() <= PREVIEW_LIMIT {
            format!(" ({})", summary.tool_names.join(", "))
        } else {
            format!(
                " ({}, …+{} more)",
                summary.tool_names[..PREVIEW_LIMIT].join(", "),
                summary.tool_names.len() - PREVIEW_LIMIT
            )
        };

        format!(
            "[{}] {} tools{} • Call load_skill(\"{}\")",
            summary.dcc, summary.tool_count, preview, summary.name
        )
    };

    McpTool {
        name: format!("__skill__{}", summary.name),
        description,
        input_schema: json!({"type": "object", "properties": {}}),
        // Skill stubs are not callable tools: they exist solely to hint the agent
        // to call `load_skill` first. Full annotation blocks add ~40-60 tokens
        // per stub × 64 skills = measurable `tools/list` bloat with zero routing
        // value for the model. (#235)
        annotations: None,
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

    // RTK-inspired: ultra-compact JSON format to reduce token consumption
    let compact_skills: Vec<serde_json::Value> = matches
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "tools": s.tool_count,
                "loaded": s.loaded,
                "dcc": s.dcc
            })
        })
        .collect();

    let result = serde_json::json!({
        "total": matches.len(),
        "query": query,
        "skills": compact_skills
    });

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string(&result)?))?,
    ))
}

/// Build a compact stub that replaces all tools of an inactive group in
/// ``tools/list``. Collapses the group into one entry the agent can reason
/// about without paying the schema cost for every member tool.
fn build_group_stub(group: &str, tool_names: &[String]) -> McpTool {
    const PREVIEW_LIMIT: usize = 5;
    let preview = if tool_names.len() <= PREVIEW_LIMIT {
        format!(" [{}]", tool_names.join(", "))
    } else {
        format!(
            " [{}, … +{} more]",
            tool_names[..PREVIEW_LIMIT].join(", "),
            tool_names.len() - PREVIEW_LIMIT
        )
    };
    let description = format!(
        "Inactive group '{group}' • {} tools{preview} • Call activate_tool_group(\"{group}\")",
        tool_names.len(),
    );
    McpTool {
        name: format!("__group__{group}"),
        description,
        input_schema: json!({"type": "object", "properties": {}}),
        // Same rationale as `build_skill_stub`: group stubs are not callable
        // tools, so their annotations are pure protocol noise. (#235)
        annotations: None,
    }
}

/// Handle ``activate_tool_group`` — flips every action in the named group
/// to ``enabled = true`` and fires a ``tools/list_changed`` notification.
async fn handle_activate_tool_group(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let group = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("group"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if group.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error("Missing required parameter: group"))?,
        ));
    }

    let changed = state.catalog.activate_group(group);
    if let Some(sid) = session_id {
        let added: Vec<String> = state
            .registry
            .list_actions_in_group(group)
            .iter()
            .map(|m| m.name.clone())
            .collect();
        let removed = vec![format!("__group__{group}")];
        notify_tools_changed(&state.sessions, sid, &added, &removed);
    }
    let payload = json!({
        "success": true,
        "group": group,
        "changed": changed,
        "active_groups": state.catalog.active_groups(),
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(payload.to_string()))?,
    ))
}

/// Handle ``deactivate_tool_group`` — mirror of [`handle_activate_tool_group`].
async fn handle_deactivate_tool_group(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let group = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("group"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if group.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error("Missing required parameter: group"))?,
        ));
    }

    let changed = state.catalog.deactivate_group(group);
    if let Some(sid) = session_id {
        let removed: Vec<String> = state
            .registry
            .list_actions_in_group(group)
            .iter()
            .map(|m| m.name.clone())
            .collect();
        let added = vec![format!("__group__{group}")];
        notify_tools_changed(&state.sessions, sid, &added, &removed);
    }
    let payload = json!({
        "success": true,
        "group": group,
        "changed": changed,
        "active_groups": state.catalog.active_groups(),
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(payload.to_string()))?,
    ))
}

/// Handle ``search_tools`` — free-text search across every registered tool.
async fn handle_search_tools(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let query = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("query"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase();
    if query.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error("Missing required parameter: query"))?,
        ));
    }
    let dcc = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("dcc"))
        .and_then(Value::as_str);
    let include_disabled = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("include_disabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut matches: Vec<serde_json::Value> = Vec::new();
    for meta in state.registry.list_actions(dcc) {
        if !include_disabled && !meta.enabled {
            continue;
        }
        let haystack = format!(
            "{} {} {} {}",
            meta.name,
            meta.description,
            meta.category,
            meta.tags.join(" ")
        )
        .to_lowercase();
        if haystack.contains(&query) {
            matches.push(serde_json::json!({
                "name": meta.name,
                "description": meta.description,
                "category": meta.category,
                "group": meta.group,
                "enabled": meta.enabled,
                "dcc": meta.dcc,
            }));
        }
    }
    let result = serde_json::json!({
        "total": matches.len(),
        "query": query,
        "tools": matches,
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string(&result)?))?,
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

/// Send a delta or full list_changed notification depending on client capability.
fn notify_tools_changed(
    sessions: &SessionManager,
    session_id: &str,
    added: &[String],
    removed: &[String],
) {
    if sessions.supports_delta_tools(session_id) {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": DELTA_TOOLS_METHOD,
            "params": { "added": added, "removed": removed }
        });
        let event = format_sse_event(&notification, None);
        sessions.push_event(session_id, event);
        tracing::debug!(
            session_id,
            added = added.len(),
            removed = removed.len(),
            "Sent tools/delta notification"
        );
    } else {
        notify_tools_list_changed(sessions, session_id);
    }
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
