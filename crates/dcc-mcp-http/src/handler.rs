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
use tokio::sync::oneshot;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    bridge_registry::BridgeRegistry,
    error::HttpError,
    executor::DccExecutorHandle,
    inflight::{CancelToken, InFlightEntry, InFlightRequests, ProgressReporter},
    prompts::{PromptError, PromptRegistry},
    protocol::{
        self, CallToolParams, CallToolResult, DELTA_TOOLS_METHOD, DELTA_TOOLS_UPDATE_CAP,
        ElicitationCapability, ElicitationCreateParams, ElicitationCreateResult, GetPromptParams,
        InitializeResult, JsonRpcBatch, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse,
        LOGGING_SET_LEVEL_METHOD, ListPromptsResult, ListResourcesResult, ListToolsResult,
        LoggingCapability, LoggingSetLevelParams, MCP_SESSION_HEADER, McpTool, McpToolAnnotations,
        PromptsCapability, RESOURCE_NOT_ENABLED_ERROR, ReadResourceParams, ResourcesCapability,
        ServerCapabilities, ServerInfo, SubscribeResourceParams, TOOLS_LIST_PAGE_SIZE,
        ToolsCapability, decode_cursor, encode_cursor, format_sse_event,
        negotiate_protocol_version,
    },
    resources::{ResourceError, ResourceRegistry},
    session::{SessionLogLevel, SessionLogMessage, SessionManager},
};
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_models::SkillScope;
use dcc_mcp_protocols::DccMcpError;
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_skills::catalog::SkillSummary;

use crate::gateway::namespace::{decode_skill_tool_name, extract_bare_tool_name, skill_tool_name};

/// How long a cancellation record is kept before being garbage-collected.
///
/// If a client sends `notifications/cancelled` for a request that has already
/// completed (common race condition), the entry would never be consumed by the
/// check in `handle_tools_call`.  This TTL bounds memory growth from such entries.
const CANCELLED_REQUEST_TTL: Duration = Duration::from_secs(30);
const ROOTS_REFRESH_TIMEOUT: Duration = Duration::from_secs(2);
const ELICITATION_TIMEOUT: Duration = Duration::from_secs(60);

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
    /// Pending `elicitation/create` requests keyed by the elicitation request id.
    pub pending_elicitations: Arc<DashMap<String, oneshot::Sender<ElicitationCreateResult>>>,
    /// When `true`, `tools/list` surfaces the three lazy-action meta-tools
    /// (`list_actions`, `describe_action`, `call_action`) and the dispatcher
    /// accepts them. See [`crate::McpHttpConfig::lazy_actions`] (#254).
    pub lazy_actions: bool,
    /// When `true` (default), `tools/list` emits bare action names whenever
    /// they are unique within the instance. See
    /// [`crate::McpHttpConfig::bare_tool_names`] (#307).
    pub bare_tool_names: bool,
    /// Registry of async jobs tracked by this server instance (#316).
    ///
    /// Actual dispatch-side wiring lands in #318; #316 only establishes the
    /// field so downstream changes can attach to it without touching
    /// `AppState` again.
    pub jobs: Arc<crate::job::JobManager>,
    /// Job / workflow lifecycle notifier (#326).
    ///
    /// Bridges `JobManager` transitions onto SSE. Also exposes
    /// [`JobNotifier::emit_workflow_update`](crate::notifications::JobNotifier::emit_workflow_update)
    /// for the #348 workflow executor to call when workflow-level
    /// transitions occur.
    pub job_notifier: crate::notifications::JobNotifier,
    /// MCP Resources primitive registry (issue #350).
    ///
    /// Populated regardless of `enable_resources` so producers can be
    /// added before the server starts; the capability is only advertised
    /// (and the JSON-RPC methods dispatched) when the flag is set.
    pub resources: ResourceRegistry,
    /// Whether the `resources/*` methods are dispatched and the
    /// `resources` capability is advertised in `initialize`.
    pub enable_resources: bool,
    /// MCP Prompts primitive registry (issues #351, #355).
    ///
    /// Always populated but only queried when `enable_prompts` is set.
    pub prompts: PromptRegistry,
    /// Whether the `prompts/*` methods are dispatched and the
    /// `prompts` capability is advertised in `initialize`.
    pub enable_prompts: bool,
    /// Prometheus exporter for tool-call observability (issue #331).
    ///
    /// Present only when the `prometheus` Cargo feature is enabled
    /// **and** [`McpHttpConfig::enable_prometheus`](crate::config::McpHttpConfig::enable_prometheus)
    /// is `true`. When `None`, every recording site is a cheap
    /// `Option::is_some` check so the overhead is negligible for
    /// servers that do not opt in.
    #[cfg(feature = "prometheus")]
    pub prometheus: Option<dcc_mcp_telemetry::PrometheusExporter>,
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
    // Client responses to server-initiated elicitation requests arrive as
    // JSON-RPC responses. Correlate and wake the waiting oneshot channel.
    for msg in &messages {
        if let JsonRpcMessage::Response(resp) = msg {
            handle_response_message(&state, resp);
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
        Some(id) if state.sessions.remove(id) => {
            if state.enable_resources {
                state.resources.drop_session(id);
            }
            StatusCode::NO_CONTENT
        }
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
        "notifications/roots/list_changed" => {
            let sid = params
                .and_then(|p| p.get("sessionId"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if sid.is_empty() {
                tracing::debug!(
                    "received notifications/roots/list_changed without sessionId; ignoring"
                );
                return;
            }
            if !state.sessions.supports_roots(sid) {
                tracing::debug!(
                    session_id = sid,
                    "ignoring roots/list_changed for session without roots support"
                );
                return;
            }
            let sid_owned = sid.to_string();
            let sessions = state.sessions.clone();
            tokio::spawn(async move {
                let refreshed = refresh_roots_cache_for_session(&sessions, &sid_owned).await;
                tracing::debug!(
                    session_id = sid_owned,
                    root_count = refreshed.len(),
                    "refreshed roots cache from roots/list_changed notification"
                );
            });
        }
        // Already handled as a request-shaped message; safe to ignore here.
        "notifications/initialized" => {}
        other => {
            tracing::debug!(method = other, "ignoring unknown MCP notification");
        }
    }
}

fn handle_response_message(state: &AppState, resp: &JsonRpcResponse) {
    let id = match &resp.id {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
        None => return,
    };
    if id.is_empty() {
        return;
    }
    let Some((_, tx)) = state.pending_elicitations.remove(&id) else {
        return;
    };
    let resolved = if let Some(result) = resp.result.clone() {
        serde_json::from_value::<ElicitationCreateResult>(result).unwrap_or(
            ElicitationCreateResult {
                action: "decline".to_string(),
                content: None,
            },
        )
    } else {
        ElicitationCreateResult {
            action: "decline".to_string(),
            content: None,
        }
    };
    let _ = tx.send(resolved);
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
        LOGGING_SET_LEVEL_METHOD => handle_logging_set_level(state, req, session_id).await,
        "tools/list" => handle_tools_list(state, req, session_id).await,
        "tools/call" => handle_tools_call(state, req, session_id).await,
        "resources/list" if state.enable_resources => handle_resources_list(state, req).await,
        "resources/read" if state.enable_resources => handle_resources_read(state, req).await,
        "resources/subscribe" if state.enable_resources => {
            handle_resources_subscribe(state, req, session_id).await
        }
        "resources/unsubscribe" if state.enable_resources => {
            handle_resources_unsubscribe(state, req, session_id).await
        }
        "prompts/list" if state.enable_prompts => handle_prompts_list(state, req).await,
        "prompts/get" if state.enable_prompts => handle_prompts_get(state, req).await,
        "elicitation/create" => handle_elicitation_create(state, req, session_id).await,
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

    // Negotiate MCP roots capability (2025-03-26+).
    let client_supports_roots = req
        .params
        .as_ref()
        .and_then(|p| p.get("capabilities"))
        .and_then(|c| c.get("roots"))
        .is_some();
    state
        .sessions
        .set_supports_roots(&sid, client_supports_roots);
    if client_supports_roots {
        let sessions = state.sessions.clone();
        let sid_owned = sid.clone();
        tokio::spawn(async move {
            let _ = refresh_roots_cache_for_session(&sessions, &sid_owned).await;
        });
    }

    let experimental_caps = if client_wants_delta {
        Some(json!({ DELTA_TOOLS_UPDATE_CAP: { "enabled": true } }))
    } else {
        None
    };

    let elicitation_cap = if negotiated == "2025-06-18" {
        Some(ElicitationCapability::default())
    } else {
        None
    };

    let resources_cap = if state.enable_resources {
        Some(ResourcesCapability {
            subscribe: true,
            list_changed: true,
        })
    } else {
        None
    };

    let prompts_cap = if state.enable_prompts {
        Some(PromptsCapability { list_changed: true })
    } else {
        None
    };

    let result = InitializeResult {
        protocol_version: negotiated.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: true }),
            resources: resources_cap,
            prompts: prompts_cap,
            logging: Some(LoggingCapability::default()),
            elicitation: elicitation_cap,
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

// ── Resources (issue #350) ─────────────────────────────────────────────────

async fn handle_resources_list(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let resources = state.resources.list();
    let result = ListResourcesResult {
        resources,
        next_cursor: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
    ))
}

async fn handle_resources_read(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<ReadResourceParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid resources/read params (expected {uri: string})",
        ));
    };

    match state.resources.read(&params.uri) {
        Ok(result) => Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(result)?,
        )),
        Err(ResourceError::NotEnabled(msg)) => Ok(JsonRpcResponse::error(
            req.id.clone(),
            RESOURCE_NOT_ENABLED_ERROR,
            msg,
        )),
        Err(ResourceError::NotFound(msg)) => Ok(JsonRpcResponse::error(
            req.id.clone(),
            RESOURCE_NOT_ENABLED_ERROR,
            format!("resource not found: {msg}"),
        )),
        Err(ResourceError::Read(msg)) => Ok(JsonRpcResponse::internal_error(
            req.id.clone(),
            format!("resource read failed: {msg}"),
        )),
    }
}

async fn handle_resources_subscribe(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(sid) = session_id else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "resources/subscribe requires Mcp-Session-Id header",
        ));
    };
    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<SubscribeResourceParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid resources/subscribe params (expected {uri: string})",
        ));
    };
    state.resources.subscribe(sid, &params.uri);
    Ok(JsonRpcResponse::success(req.id.clone(), json!({})))
}

async fn handle_resources_unsubscribe(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(sid) = session_id else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "resources/unsubscribe requires Mcp-Session-Id header",
        ));
    };
    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<SubscribeResourceParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid resources/unsubscribe params (expected {uri: string})",
        ));
    };
    state.resources.unsubscribe(sid, &params.uri);
    Ok(JsonRpcResponse::success(req.id.clone(), json!({})))
}

// ── Prompts (issues #351, #355) ────────────────────────────────────────────

async fn handle_prompts_list(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let catalog = state.catalog.clone();
    let prompts = state.prompts.list(|visit| {
        catalog.for_each_loaded_metadata(|md| visit(md));
    });
    let result = ListPromptsResult {
        prompts,
        next_cursor: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
    ))
}

async fn handle_prompts_get(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<GetPromptParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid prompts/get params (expected {name: string, arguments?: object})",
        ));
    };
    let catalog = state.catalog.clone();
    let lookup = state.prompts.get(&params.name, &params.arguments, |visit| {
        catalog.for_each_loaded_metadata(|md| visit(md));
    });
    match lookup {
        Ok(result) => Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(result)?,
        )),
        Err(PromptError::NotFound(name)) => Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            format!("prompt not found: {name}"),
        )),
        Err(PromptError::MissingArg(arg)) => Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            format!("missing required argument: {arg}"),
        )),
        Err(PromptError::Load(msg)) => Ok(JsonRpcResponse::internal_error(
            req.id.clone(),
            format!("prompts/get load failure: {msg}"),
        )),
    }
}

/// Emit `notifications/prompts/list_changed` to every session whose SSE
/// stream is live. Called from skill load / unload paths.
pub(crate) fn notify_prompts_list_changed_all(state: &AppState) {
    if !state.enable_prompts {
        return;
    }
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/prompts/list_changed",
        "params": {}
    });
    let event = format_sse_event(&notification, None);
    for sid in state.sessions.all_ids() {
        state.sessions.push_event(&sid, event.clone());
    }
}

async fn handle_logging_set_level(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(sid) = session_id else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "logging/setLevel requires Mcp-Session-Id header",
        ));
    };

    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<LoggingSetLevelParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid logging/setLevel params",
        ));
    };

    let Some(level) = SessionLogLevel::parse(&params.level) else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid logging level. Expected one of: debug, info, warning, error",
        ));
    };

    if !state.sessions.set_log_level(sid, level) {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Session not found",
        ));
    }

    let request_id = request_id_to_string(req.id.as_ref());
    notify_message(
        &state.sessions,
        sid,
        SessionLogMessage {
            level: SessionLogLevel::Info,
            logger: "dcc_mcp_http.logging".to_string(),
            data: json!({
                "event": "set_level",
                "level": level.as_str(),
            }),
            request_id,
        },
    );

    Ok(JsonRpcResponse::success(req.id.clone(), json!({})))
}

async fn handle_elicitation_create(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    // Spec gate: only exposed on 2025-06-18 sessions.
    let is_2025_06_18 = session_id
        .and_then(|sid| state.sessions.get_protocol_version(sid))
        .as_deref()
        == Some("2025-06-18");
    if !is_2025_06_18 {
        return Ok(JsonRpcResponse::method_not_found(
            req.id.clone(),
            "elicitation/create",
        ));
    }
    let sid = match session_id {
        Some(s) => s,
        None => {
            return Err(HttpError::Internal(
                "elicitation/create requires Mcp-Session-Id".to_string(),
            ));
        }
    };
    let elicit_id = req.id.clone().ok_or_else(|| {
        HttpError::Internal("elicitation/create requires a JSON-RPC request id".to_string())
    })?;
    let req_id = match &elicit_id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    if req_id.is_empty() {
        return Err(HttpError::Internal(
            "elicitation/create request id cannot be empty".to_string(),
        ));
    }

    let params: ElicitationCreateParams = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .ok_or_else(|| HttpError::Internal("invalid elicitation/create params".to_string()))?;

    let (tx, rx) = oneshot::channel::<ElicitationCreateResult>();
    state.pending_elicitations.insert(req_id.clone(), tx);

    let notification = json!({
        "jsonrpc": "2.0",
        "method": "elicitation/create",
        "params": {
            "id": elicit_id,
            "message": params.message,
            "requestedSchema": params.requested_schema,
        }
    });
    let event = format_sse_event(&notification, None);
    state.sessions.push_event(sid, event);

    let waited = tokio::time::timeout(ELICITATION_TIMEOUT, rx).await;
    state.pending_elicitations.remove(&req_id);

    let result = match waited {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => ElicitationCreateResult {
            action: "decline".to_string(),
            content: None,
        },
        Err(_) => {
            let envelope = DccMcpError::new(
                "dcc",
                "ELICITATION_TIMEOUT",
                format!(
                    "Client did not answer elicitation request {req_id} within {} seconds.",
                    ELICITATION_TIMEOUT.as_secs()
                ),
            )
            .with_hint("Ask the user again or proceed with a conservative default.");
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
            ));
        }
    };

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
    ))
}

async fn handle_tools_list(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    // 1. Core discovery tools — always fully visible (static, cached once per process)
    let core = build_core_tools();
    let mut tools: Vec<McpTool> = Vec::with_capacity(core.len() + 16);
    tools.extend_from_slice(core);

    // 1b. Optional lazy-actions fast-path (#254) — three extra meta-tools that
    //     let agents drive an arbitrarily large action catalog without paging
    //     through every skill's full schema. Opt-in via
    //     `McpHttpConfig::lazy_actions`.
    if state.lazy_actions {
        tools.extend(build_lazy_action_tools());
    }

    // #242 — ``outputSchema`` is only valid on 2025-06-18 sessions. On
    // 2025-03-26 we strip it so compliant clients never see a field they
    // cannot process.
    let include_output_schema = session_id
        .and_then(|sid| state.sessions.get_protocol_version(sid))
        .as_deref()
        == Some("2025-06-18");

    // 2. Loaded skill tools — full definitions from ActionRegistry.
    //    Tools in inactive groups are collapsed into one ``__group__<name>``
    //    stub per group to keep ``tools/list`` compact (progressive exposure).
    let actions = state.registry.list_actions(None);

    // #307 — decide which actions can publish under their **bare name** on
    // this instance. `bare_eligible` contains `(skill, action)` tuples for
    // every action whose bare name is unique across loaded skills.
    let bare_eligible: std::collections::HashSet<(String, String)> = if state.bare_tool_names {
        let inputs: Vec<crate::gateway::namespace::BareNameInput<'_>> = actions
            .iter()
            .filter(|m| m.enabled)
            .filter_map(|m| {
                m.skill_name
                    .as_deref()
                    .map(|sn| crate::gateway::namespace::BareNameInput {
                        skill_name: sn,
                        action_name: m.name.as_str(),
                    })
            })
            .collect();
        crate::gateway::namespace::resolve_bare_names(&inputs)
    } else {
        std::collections::HashSet::new()
    };

    let mut inactive_groups: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for meta in &actions {
        if meta.enabled {
            tools.push(action_meta_to_mcp_tool(
                meta,
                include_output_schema,
                &bare_eligible,
            ));
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
    // Observe tool-call duration / status when the Prometheus exporter
    // is enabled (issue #331). We extract the tool name eagerly so we
    // can still record a row for malformed params.
    #[cfg(feature = "prometheus")]
    let prom_start = std::time::Instant::now();
    #[cfg(feature = "prometheus")]
    let prom_tool_name: Option<String> = req
        .params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    let result = handle_tools_call_inner(state, req, session_id).await;

    #[cfg(feature = "prometheus")]
    if let Some(exporter) = state.prometheus.as_ref() {
        let tool = prom_tool_name.as_deref().unwrap_or("<unknown>");
        let status = match &result {
            Ok(resp) => {
                // A JSON-RPC success response with `result.isError == true`
                // is a tool-level error (MCP convention). Distinguish so
                // counters match what operators see in traces.
                if resp
                    .result
                    .as_ref()
                    .and_then(|r| r.get("isError"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    "error"
                } else {
                    "success"
                }
            }
            Err(_) => "error",
        };
        exporter.record_tool_call(tool, status, prom_start.elapsed());
    }

    result
}

async fn handle_tools_call_inner(
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
        "list_roots" => return handle_list_roots(state, req, session_id).await,
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
        // #319 — built-in job polling tool. Always available, regardless of
        // which skills are loaded or whether any jobs exist.
        "jobs.get_status" => return handle_jobs_get_status(state, req, &params).await,
        // #328 — built-in TTL pruning for tracked jobs.
        "jobs.cleanup" => return handle_jobs_cleanup(state, req, &params).await,
        // #254 — lazy-actions fast-path (opt-in).
        "list_actions" if state.lazy_actions => {
            return handle_list_actions(state, req, &params).await;
        }
        "describe_action" if state.lazy_actions => {
            return handle_describe_action(state, req, &params, session_id).await;
        }
        "call_action" if state.lazy_actions => {
            return handle_call_action(state, req, &params, session_id).await;
        }
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

    // Tool name resolution (#238 + #307):
    //   1. Exact registry hit (canonical `skill__action` form).
    //   2. `<skill>.<action>` shape — the legacy prefixed form. Accepted for
    //      one release even when `bare_tool_names` is on; emits a one-shot
    //      warning so operators find remaining hard-coded clients.
    //   3. Bare action name — the preferred #307 form when unique, or
    //      legacy fallback when the client predates #238.
    let resolved_name: String = if state.registry.get_action(&tool_name, None).is_some() {
        tool_name.clone()
    } else if let Some((skill_part, bare_tool)) = decode_skill_tool_name(&tool_name) {
        let matched = state
            .registry
            .list_actions_by_skill(skill_part)
            .into_iter()
            .find(|m| extract_bare_tool_name(skill_part, &m.name) == bare_tool);
        if let Some(m) = matched {
            if state.bare_tool_names {
                crate::gateway::namespace::warn_legacy_prefixed_once(&tool_name);
            }
            m.name
        } else {
            tool_name.clone()
        }
    } else {
        let lm = state.registry.list_actions(None).into_iter().find(|m| {
            m.skill_name
                .as_deref()
                .map(|sn| extract_bare_tool_name(sn, &m.name) == tool_name.as_str())
                .unwrap_or(false)
        });
        if let Some(ref matched) = lm {
            // When bare names are the blessed form (#307) this path is the
            // happy path — stay silent. Only warn when the server was
            // explicitly told to keep the prefixed form as the primary shape,
            // which means a bare call is the legacy escape hatch.
            if !state.bare_tool_names {
                let canonical =
                    skill_tool_name(matched.skill_name.as_deref().unwrap_or(""), &matched.name)
                        .unwrap_or_else(|| matched.name.clone());
                tracing::warn!(bare_name=%tool_name, "Deprecated bare name -- use {canonical}.");
            }
            matched.name.clone()
        } else {
            tool_name.clone()
        }
    };

    // Check action exists in registry before dispatch
    let action_meta_snapshot = state.registry.get_action(&resolved_name, None);
    if action_meta_snapshot.is_none() {
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

    // ── Async dispatch path (#318) ───────────────────────────────────────
    //
    // Opt-in conditions — any of these routes the call through `JobManager`
    // and returns immediately with `{job_id, status: "pending"}`:
    //
    // 1. `_meta.dcc.async == true` (explicit client opt-in).
    // 2. `_meta.progressToken` is set (MCP 2025-03-26 long-running hint).
    // 3. The tool declares `execution: async` in its `ActionMeta` (#317).
    // 4. The tool declares a non-zero `timeout_hint_secs` (#317) — the
    //    skill author signalled "expect this to take a while".
    //
    // Otherwise dispatch is synchronous (unchanged path below).
    let meta_dcc = params.meta.as_ref().and_then(|m| m.dcc.as_ref());
    let async_opt_in = meta_dcc.is_some_and(|d| d.r#async);
    let has_progress_token = params
        .meta
        .as_ref()
        .and_then(|m| m.progress_token.as_ref())
        .is_some();
    let action_meta_for_async = action_meta_snapshot.as_ref();
    let action_declares_async = action_meta_for_async
        .map(|m| {
            matches!(m.execution, dcc_mcp_models::ExecutionMode::Async)
                || m.timeout_hint_secs.unwrap_or(0) > 0
        })
        .unwrap_or(false);
    let should_dispatch_async = async_opt_in || has_progress_token || action_declares_async;
    if should_dispatch_async {
        let parent_job_id = meta_dcc.and_then(|d| d.parent_job_id.clone());
        let progress_token = params.meta.as_ref().and_then(|m| m.progress_token.clone());
        // #332 — inspect the tool's thread_affinity. Main-affined tools must
        // execute on the DCC main thread via DeferredExecutor even along the
        // async path; Any-affined tools execute on a Tokio worker.
        let thread_affinity = action_meta_for_async
            .map(|m| m.thread_affinity)
            .unwrap_or_default();
        return dispatch_async_job(
            state,
            req,
            resolved_name,
            call_params,
            parent_job_id,
            session_id,
            progress_token,
            thread_affinity,
        )
        .await;
    }

    // ── Register in-flight entry (#240 progress + #241 cancellation) ─────
    let req_id_str: Option<String> = req.id.as_ref().map(|id| match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    });

    if let Some(sid) = session_id {
        notify_message(
            &state.sessions,
            sid,
            SessionLogMessage {
                level: SessionLogLevel::Debug,
                logger: "dcc_mcp_http.tools".to_string(),
                data: json!({
                    "event": "tools_call_received",
                    "tool_name": tool_name.clone(),
                    "resolved_name": resolved_name.clone(),
                }),
                request_id: req_id_str.clone(),
            },
        );
    }

    let progress_token = params.meta.as_ref().and_then(|m| m.progress_token.clone());
    let cancel_token = CancelToken::new();
    let progress_reporter = ProgressReporter::new(
        progress_token.clone(),
        session_id.map(str::to_owned),
        state.sessions.clone(),
        req_id_str.clone().unwrap_or_default(),
    );

    // ── Job lifecycle tracking (#316 + #326) ─────────────────────────────
    // Create a Pending→Running→terminal job whenever either (a) the caller
    // supplied a `progressToken` (channel A will fire) or (b) the session
    // opted into `$/dcc.jobUpdated` via `enable_job_notifications`.
    let job_tracking_session = session_id.map(str::to_owned);
    let track_job = job_tracking_session.is_some()
        && (progress_token.is_some() || state.job_notifier.job_updates_enabled());
    let tracked_job_id: Option<String> = if track_job {
        let sid = job_tracking_session.as_deref().unwrap();
        state.job_notifier.subscribe_session(sid);
        let handle = state.jobs.create(tool_name.clone());
        let id = handle.read().id.clone();
        state
            .job_notifier
            .register_job(&id, sid, progress_token.clone());
        state.jobs.start(&id);
        Some(id)
    } else {
        None
    };

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
        let name = resolved_name.clone();
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
        let name = resolved_name.clone();
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

    // ── Drive the tracked job to its terminal state (#326) ──────────────
    if let Some(ref jid) = tracked_job_id {
        match &dispatch_outcome {
            Ok(v) => {
                state.jobs.complete(jid, v.clone());
            }
            Err(msg) if msg == "CANCELLED" => {
                state.jobs.cancel(jid);
            }
            Err(msg) => {
                state.jobs.fail(jid, msg.clone());
            }
        }
    }

    let mut call_result = match dispatch_outcome {
        Ok(output) => {
            let text = match &output {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
            };
            let mut content = vec![protocol::ToolContent::Text { text }];

            // #243/#242 — both features are gated on 2025-06-18 sessions.
            //   * resource_link: surface DCC artifact files without copying bytes
            //   * structuredContent: hand back machine-readable payloads so the
            //     agent skips the text→JSON re-parse step
            let is_2025_06_18 = session_id
                .and_then(|sid| state.sessions.get_protocol_version(sid))
                .as_deref()
                == Some("2025-06-18");

            if is_2025_06_18 {
                content.extend(crate::resource_link::extract_resource_links(&output));
            }

            // #242 — ``structuredContent`` carries the dispatch output verbatim
            // when it is a JSON object or array. Strings and nulls go through
            // ``content[].text`` only, matching the 2025-03-26 convention.
            // Older sessions never see the field (serde skips None).
            let structured_content =
                if is_2025_06_18 && matches!(&output, Value::Object(_) | Value::Array(_)) {
                    Some(output.clone())
                } else {
                    None
                };

            CallToolResult {
                content,
                structured_content,
                is_error: false,
                meta: None,
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
            if let Some(sid) = session_id {
                notify_message(
                    &state.sessions,
                    sid,
                    SessionLogMessage {
                        level: SessionLogLevel::Error,
                        logger: "dcc_mcp_http.tools".to_string(),
                        data: json!({
                            "event": "tools_call_failed",
                            "tool_name": tool_name.clone(),
                            "error": err_msg.clone(),
                        }),
                        request_id: req_id_str.clone(),
                    },
                );
            }

            let mut envelope = if err_msg.contains("no handler registered") {
                DccMcpError::new(
                    "instance",
                    "NO_HANDLER",
                    format!("Tool '{tool_name}' is registered but has no handler."),
                )
                .with_hint("Register a handler via ActionDispatcher.register_handler().")
            } else {
                DccMcpError::new("instance", "EXECUTION_FAILED", &err_msg)
            };

            if let (Some(sid), Some(rid)) = (session_id, req_id_str.as_deref()) {
                let log_tail = state.sessions.tail_logs_for_request(sid, rid, 20);
                if !log_tail.is_empty() {
                    envelope = envelope.with_details(json!({ "log_tail": log_tail }));
                }
            }
            CallToolResult {
                content: vec![protocol::ToolContent::Text {
                    text: envelope.to_json(),
                }],
                structured_content: None,
                is_error: true,
                meta: None,
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

    // Issue #342 — attach `_meta["dcc.next_tools"]` with the matching
    // on-success / on-failure list when the tool declared one. The slot
    // is asymmetric on purpose: success results never expose on-failure
    // suggestions and vice versa. Absent → no key, never an empty dict.
    if let Some(action_meta) = state.registry.get_action(&resolved_name, None) {
        attach_next_tools_meta(&mut call_result, &action_meta.next_tools);
    }

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(call_result)?,
    ))
}

/// Async job dispatch path for `tools/call` (issue #318).
///
/// Creates a [`crate::job::Job`] via `state.jobs`, spawns the actual tool
/// execution on Tokio, and returns immediately with a spec-compliant
/// `CallToolResult` envelope:
///
/// ```json
/// {
///   "content": [{"type": "text", "text": "Job <id> queued"}],
///   "structuredContent": {"job_id": "<uuid>", "status": "pending", "parent_job_id": "<uuid>|null"},
///   "isError": false,
///   "_meta": {"dcc": {"jobId": "<uuid>", "parentJobId": "<uuid>|null"}, "status": "pending"}
/// }
/// ```
///
/// Parent-job cascade: when `parent_job_id` resolves to a tracked job, the
/// child's `CancellationToken` is derived from the parent's via
/// [`tokio_util::sync::CancellationToken::child_token`]. Cancelling the
/// parent therefore cancels every descendant within one cooperative
/// checkpoint.
#[allow(clippy::too_many_arguments)]
async fn dispatch_async_job(
    state: &AppState,
    req: &JsonRpcRequest,
    resolved_name: String,
    call_params: Value,
    parent_job_id: Option<String>,
    session_id: Option<&str>,
    progress_token: Option<Value>,
    thread_affinity: dcc_mcp_models::ThreadAffinity,
) -> Result<JsonRpcResponse, HttpError> {
    let job_handle = state
        .jobs
        .create_with_parent(resolved_name.clone(), parent_job_id.clone());
    let (job_id, cancel_token) = {
        let j = job_handle.read();
        (j.id.clone(), j.cancel_token.clone())
    };

    // ── Wire job lifecycle notifications (#326) ──────────────────────────
    // Map job_id → (session_id, progress_token) so JobNotifier can fan out
    // both `notifications/progress` (if progress_token was supplied) and
    // `notifications/$/dcc.jobUpdated` on every status transition.
    if let Some(sid) = session_id {
        state.job_notifier.subscribe_session(sid);
        state
            .job_notifier
            .register_job(&job_id, sid, progress_token.clone());
    }

    tracing::info!(
        job_id = %job_id,
        tool = %resolved_name,
        parent_job_id = ?parent_job_id,
        affinity = %thread_affinity,
        "async job dispatched"
    );

    // Spawn the actual execution. The task owns clones of everything it
    // needs; the request task returns immediately with the pending envelope.
    let jobs = Arc::clone(&state.jobs);
    let dispatcher = Arc::clone(&state.dispatcher);
    let executor = state.executor.clone();
    let spawn_job_id = job_id.clone();
    let spawn_name = resolved_name.clone();
    let spawn_params = call_params;
    let use_main_thread = matches!(thread_affinity, dcc_mcp_models::ThreadAffinity::Main);
    if use_main_thread && executor.is_none() {
        tracing::warn!(
            tool = %spawn_name,
            "tool declares thread_affinity=main but no DeferredExecutor is wired; \
             falling back to Tokio worker — scene API calls will be unsafe"
        );
    }
    tokio::spawn(async move {
        // Pending → Running. If the job was cancelled before pick-up, skip.
        if cancel_token.is_cancelled() {
            tracing::debug!(job_id = %spawn_job_id, "job cancelled before execution");
            return;
        }
        if jobs.start(&spawn_job_id).is_none() {
            tracing::debug!(job_id = %spawn_job_id, "job could not enter Running state");
            return;
        }

        // #332 — pick the execution lane:
        //   * `Main` + executor available  → DeferredExecutor::submit_deferred
        //     (guarantees the handler runs on the DCC main thread)
        //   * `Main` + no executor         → Tokio worker (already warned above)
        //   * `Any`                        → Tokio worker
        let route_to_main = use_main_thread && executor.is_some();
        let exec_result: Result<Value, String> = if route_to_main {
            let exec = executor.as_ref().unwrap();
            let disp = Arc::clone(&dispatcher);
            let name = spawn_name.clone();
            let p = spawn_params.clone();
            let rx = exec.submit_deferred(
                &spawn_name,
                cancel_token.clone(),
                Box::new(move || match disp.dispatch(&name, p) {
                    Ok(r) => serde_json::to_string(&r.output).unwrap_or_else(|_| "null".into()),
                    Err(e) => serde_json::to_string(&json!({"__dispatch_error": e.to_string()}))
                        .unwrap_or_default(),
                }),
            );
            tokio::select! {
                out = rx => match out {
                    Ok(json_str) => {
                        let v: Value = serde_json::from_str(&json_str).unwrap_or(json!({}));
                        if let Some(err) = v.get("__dispatch_error") {
                            Err(err.as_str().unwrap_or("dispatch error").to_string())
                        } else {
                            Ok(v)
                        }
                    }
                    // oneshot dropped without sending → cancelled or executor down.
                    Err(_) => Err("CANCELLED".to_string()),
                },
                _ = cancel_token.cancelled() => Err("CANCELLED".to_string()),
            }
        } else {
            // `Any` affinity (or `Main` fallback): offload to a blocking
            // worker with cooperative cancel via `tokio::select!`.
            let disp = Arc::clone(&dispatcher);
            let name = spawn_name.clone();
            let p = spawn_params.clone();
            let ct = cancel_token.clone();
            let fut = tokio::task::spawn_blocking(move || {
                if ct.is_cancelled() {
                    return Err("CANCELLED".to_string());
                }
                disp.dispatch(&name, p)
                    .map(|r| r.output)
                    .map_err(|e| e.to_string())
            });
            tokio::select! {
                r = fut => r.map_err(|e| e.to_string()).and_then(|x| x),
                _ = cancel_token.cancelled() => Err("CANCELLED".to_string()),
            }
        };

        match exec_result {
            Ok(v) => {
                if jobs.complete(&spawn_job_id, v).is_none() {
                    tracing::debug!(
                        job_id = %spawn_job_id,
                        "job.complete rejected — likely cancelled concurrently"
                    );
                }
            }
            Err(msg) if msg == "CANCELLED" => {
                // `cancel_token` firing already transitioned the job via
                // JobManager::cancel if that path was taken. If the job is
                // still Running (e.g. the token fired via parent cascade
                // without a direct `cancel()` call), mark it cancelled now.
                if jobs
                    .get(&spawn_job_id)
                    .map(|h| h.read().status)
                    .is_some_and(|s| !s.is_terminal())
                {
                    jobs.cancel(&spawn_job_id);
                }
            }
            Err(msg) => {
                jobs.fail(&spawn_job_id, msg);
            }
        }
    });

    // Build the pending envelope.
    let structured = json!({
        "job_id": job_id,
        "status": "pending",
        "parent_job_id": parent_job_id,
    });
    let mut meta = serde_json::Map::new();
    meta.insert("status".to_string(), json!("pending"));
    let mut dcc_meta = serde_json::Map::new();
    dcc_meta.insert("jobId".to_string(), json!(job_id));
    dcc_meta.insert(
        "parentJobId".to_string(),
        parent_job_id
            .as_ref()
            .map(|p| json!(p))
            .unwrap_or(Value::Null),
    );
    meta.insert("dcc".to_string(), Value::Object(dcc_meta));

    // The CallToolResult shape doesn't carry a `_meta` field today; embed it
    // into `structured_content` so clients that read either surface find it.
    // This matches the "structuredContent carries job metadata" convention
    // spelled out in #318 while remaining spec-compliant (extra keys allowed).
    let structured_with_meta = {
        let mut s = structured.as_object().cloned().unwrap_or_default();
        s.insert("_meta".to_string(), Value::Object(meta));
        Value::Object(s)
    };

    let envelope = CallToolResult {
        content: vec![protocol::ToolContent::Text {
            text: format!("Job {job_id} queued"),
        }],
        structured_content: Some(structured_with_meta),
        is_error: false,
        meta: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(envelope)?,
    ))
}

/// Populate `CallToolResult._meta["dcc.next_tools"]` per issue #342.
///
/// The key is only emitted when the relevant list (on-success for a
/// success result, on-failure for an error result) is non-empty. Other
/// existing `_meta` entries are preserved; callers are expected to own
/// their own vendor namespace inside the same map.
fn attach_next_tools_meta(result: &mut CallToolResult, next_tools: &dcc_mcp_models::NextTools) {
    let list = if result.is_error {
        &next_tools.on_failure
    } else {
        &next_tools.on_success
    };
    if list.is_empty() {
        return;
    }
    let key = if result.is_error {
        "on_failure"
    } else {
        "on_success"
    };
    let mut nt_map = serde_json::Map::new();
    nt_map.insert(
        key.to_string(),
        Value::Array(list.iter().map(|n| Value::String(n.clone())).collect()),
    );
    let meta = result.meta.get_or_insert_with(serde_json::Map::new);
    meta.insert("dcc.next_tools".to_string(), Value::Object(nt_map));
}

async fn handle_list_roots(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(sid) = session_id else {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "list_roots requires Mcp-Session-Id header",
            ))?,
        ));
    };
    let roots = state.sessions.get_client_roots(sid);
    let payload = json!({
        "supports_roots": state.sessions.supports_roots(sid),
        "count": roots.len(),
        "roots": roots,
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string_pretty(
            &payload,
        )?))?,
    ))
}

// ── Core discovery tool handlers ──────────────────────────────────────────

/// Deprecated — kept as a compatibility shim that forwards to
/// `search_skills` (issue #340).
///
/// Emits a `tracing::warn!` on every call and attaches the deprecation
/// notice to the MCP `_meta` block on the response so agents can surface
/// the guidance without reparsing text content. Scheduled for removal in
/// v0.17.
async fn handle_find_skills(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    tracing::warn!(
        "find_skills is deprecated; use search_skills instead (issue #340). \
         Scheduled for removal in v0.17."
    );

    // Forward to the unified entry point. `find_skills` historically required
    // no arguments, so we map its full parameter surface (query/tags/dcc)
    // onto `search_skills` 1:1 — no caller breaks.
    let mut resp = handle_search_skills(state, req, params).await?;

    // Attach the deprecation marker to `_meta` on the CallToolResult. We
    // deserialize, mutate, and reserialize the result so we can reach into
    // the envelope that `handle_search_skills` just produced.
    if let Some(result_val) = resp.result.as_mut() {
        if let Ok(mut ctr) = serde_json::from_value::<CallToolResult>(result_val.clone()) {
            let meta = ctr.meta.get_or_insert_with(serde_json::Map::new);
            meta.insert(
                "dcc.deprecation".to_string(),
                Value::String(
                    "find_skills is deprecated — use search_skills. Will be removed in v0.17."
                        .to_string(),
                ),
            );
            *result_val = serde_json::to_value(&ctr)?;
        }
    }
    Ok(resp)
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
        // Skill content changed — invalidate the prompt cache and
        // broadcast `notifications/prompts/list_changed` (issues
        // #351, #355).
        state.prompts.invalidate();
        notify_prompts_list_changed_all(state);
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
            // Invalidate prompt cache and fire
            // `notifications/prompts/list_changed` (issues #351, #355).
            state.prompts.invalidate();
            notify_prompts_list_changed_all(state);

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
            name: "list_roots".to_string(),
            description: "Returns the filesystem roots the MCP client advertised for this session (cached from roots/list).\n\n\
                          When to use: Call when another tool needs to resolve a relative path and you have no absolute context yet. Rarely needed — most DCC tools operate on in-memory scene data, not the client's workspace.\n\n\
                          How to use:\n\
                          - Takes no arguments; returns an empty array if the client sent no roots.\n\
                          - Do not call repeatedly; roots change only on client reconnect."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("List Roots".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "find_skills".to_string(),
            description: "Deprecated (#340): forwards to search_skills and stamps _meta with a deprecation notice; removed in v0.17.\n\n\
                          When to use: Only for backward compatibility. New code should call search_skills instead.\n\n\
                          How to use:\n\
                          - Prefer search_skills(query, tags, dcc, scope, limit).\n\
                          - After a match, call load_skill(skill_name=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword matched against skill name and description."
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Tag filter; every listed tag must match."
                    },
                    "dcc": {
                        "type": "string",
                        "description": "DCC type filter (e.g. maya, blender, houdini)."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Find Skills (deprecated)".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "list_skills".to_string(),
            description: "Lists every discovered skill on this server with its current load status (loaded, unloaded, or error).\n\n\
                          When to use: Use to browse what is available or to audit which skills are currently active. For keyword lookup, call search_skills instead — list_skills is a flat dump with no ranking.\n\n\
                          How to use:\n\
                          - Pass status='loaded' to inspect the active tool surface, 'unloaded' to find candidates to load.\n\
                          - Follow up with get_skill_info(skill_name=...) or load_skill(skill_name=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["all", "loaded", "unloaded", "error"],
                        "default": "all",
                        "description": "Load-status filter; 'all' returns every discovered skill."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("List Skills".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "get_skill_info".to_string(),
            description: "Returns detailed metadata for one skill: description, tags, DCC binding, and full input schemas for every tool it declares.\n\n\
                          When to use: Use when you already know the skill name and need to inspect its tools' schemas before committing to load_skill. Pair this with search_skills when deciding between candidates.\n\n\
                          How to use:\n\
                          - Inspecting alone does not make the tools callable; follow up with load_skill(skill_name=...) to activate them.\n\
                          - Returns an error if the skill is unknown."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Exact skill name as reported by list_skills / search_skills."
                    }
                },
                "required": ["skill_name"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Get Skill Info".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "load_skill".to_string(),
            description: "Loads one or more discovered skills and registers their tools, then emits a tools/list_changed notification.\n\n\
                          When to use: Call after search_skills, list_skills, or get_skill_info has identified the skill you need. Idempotent — re-loading an already-loaded skill is a no-op.\n\n\
                          How to use:\n\
                          - Use skill_name for one skill, or skill_names for a batch in a single round-trip.\n\
                          - After success, call tools/list or the specific tool (e.g. maya_geometry__create_sphere) directly."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Single skill to load; mutually exclusive with skill_names."
                    },
                    "skill_names": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Batch of skill names to load in one call."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Load Skill".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "unload_skill".to_string(),
            description: "Unloads a previously loaded skill, unregisters its tools, and emits a tools/list_changed notification.\n\n\
                          When to use: Use to free tool slots and shrink the tools/list token footprint once a workflow no longer needs a skill. Safe to call on an unloaded skill (no-op).\n\n\
                          How to use:\n\
                          - Pending tools/call requests against this skill will fail after unload — drain them first.\n\
                          - To re-enable the skill later, call load_skill(skill_name=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Exact skill name previously passed to load_skill."
                    }
                },
                "required": ["skill_name"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Unload Skill".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "search_skills".to_string(),
            description: "Unified skill discovery (#340, supersedes find_skills). Ranks skills against query across name, description, search-hint, tags, and tool names; filters by tags/dcc/scope.\n\n\
                          When to use: Start here when you need a capability but don't know the skill name. Call with no args to browse by trust scope (Admin>System>User>Repo).\n\n\
                          How to use:\n\
                          - Keep query short (2-4 keywords); combine with tags/dcc/scope.\n\
                          - After a hit, call load_skill(skill_name=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Short keyword phrase (2-4 words). Leave empty to browse by scope."
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by tags (all must match; case-insensitive)."
                    },
                    "dcc": {
                        "type": "string",
                        "description": "DCC filter (e.g. maya, blender, houdini)."
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["repo", "user", "system", "admin"],
                        "description": "Filter by trust scope (Admin > System > User > Repo)."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 20,
                        "description": "Cap the number of results (default 20, max 100)."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Search Skills".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "activate_tool_group".to_string(),
            description: "Activates a tool group inside a loaded skill, making its members callable and emitting a tools/list_changed notification.\n\n\
                          When to use: Call when tools/list surfaces a __group__<name> stub and you need the underlying tools. Progressive exposure keeps the default surface small until you opt in.\n\n\
                          How to use:\n\
                          - The parent skill must be loaded first; check list_skills(status='loaded') if unsure.\n\
                          - After activation, re-run tools/list to see the newly available tools, then call them by name."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "Group name as shown in the __group__<name> stub."
                    }
                },
                "required": ["group"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Activate Tool Group".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "deactivate_tool_group".to_string(),
            description: "Deactivates a tool group, collapsing its members back into a __group__<name> stub and emitting a tools/list_changed notification.\n\n\
                          When to use: Use to shrink the active tool surface once a sub-workflow is done, to stay within the client's token budget. Group tools remain on disk — only their visibility changes.\n\n\
                          How to use:\n\
                          - Idempotent; calling on an already-inactive group is a safe no-op.\n\
                          - To bring the tools back, call activate_tool_group(group=...)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "group": {
                        "type": "string",
                        "description": "Group name previously passed to activate_tool_group."
                    }
                },
                "required": ["group"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Deactivate Tool Group".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "search_tools".to_string(),
            description: "Full-text search over already-registered tools, matching name, description, category, and tags and ranking enabled tools first.\n\n\
                          When to use: Use after skills are loaded to locate a specific tool without dumping the whole tools/list. If nothing matches, fall back to search_skills — the tool may live in an unloaded skill.\n\n\
                          How to use:\n\
                          - Keep the query short; set include_disabled=true only when inspecting inactive groups.\n\
                          - Call the returned tool directly by its name."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keyword matched against tool name, description, category, and tags."
                    },
                    "dcc": {
                        "type": "string",
                        "description": "DCC filter (e.g. maya, blender)."
                    },
                    "include_disabled": {
                        "type": "boolean",
                        "default": false,
                        "description": "Also search tools inside inactive tool groups."
                    }
                },
                "required": ["query"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Search Tools".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        // `jobs.get_status` — built-in job-polling tool (#319).
        //
        // Complements the `$/dcc.jobUpdated` SSE channel (#326) for clients
        // that prefer request/response polling over a long-lived stream.
        // SEP-986 compliant: the dot-separated `jobs.*` namespace is the
        // reserved built-in prefix (see `docs/guide/naming.md`). We panic
        // at first build if the regex or the length cap ever rejects this
        // name — that would be a dcc-mcp-naming regression and we want to
        // catch it loudly.
        {
            const TOOL_NAME: &str = "jobs.get_status";
            if let Err(e) = dcc_mcp_naming::validate_tool_name(TOOL_NAME) {
                panic!("built-in tool name `{TOOL_NAME}` fails SEP-986 validation: {e}");
            }
            McpTool {
                name: TOOL_NAME.to_string(),
                description: "Poll the status of an async tool-call job tracked by JobManager. \
                              Returns a JSON envelope with job_id, parent_job_id, tool, status \
                              (pending|running|completed|failed|cancelled|interrupted), timestamps, \
                              progress, error, and optionally the final ToolResult once the job \
                              is terminal. Complements the `$/dcc.jobUpdated` SSE channel (#326) \
                              for polling-based clients. Returns isError=true with a human-readable \
                              message when the job id is unknown (never a JSON-RPC transport error)."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "job_id": {
                            "type": "string",
                            "description": "UUID of the job to query"
                        },
                        "include_logs": {
                            "type": "boolean",
                            "default": false,
                            "description": "Include captured stdout/stderr if any. \
                                            Currently a no-op — JobManager does not capture logs; \
                                            the flag is accepted for forward compatibility."
                        },
                        "include_result": {
                            "type": "boolean",
                            "default": true,
                            "description": "Include the job's final ToolResult when the job is \
                                            in a terminal state (completed/failed). Ignored for \
                                            pending/running jobs since no result exists yet."
                        }
                    },
                    "required": ["job_id"]
                }),
                output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Get Job Status".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
            }
        },
        // `jobs.cleanup` — built-in TTL pruning tool (#328). Removes
        // terminal job rows (and storage-backed rows when a
        // `job_storage_path` is configured) older than the given
        // window. Never touches pending / running jobs.
        {
            const TOOL_NAME: &str = "jobs.cleanup";
            if let Err(e) = dcc_mcp_naming::validate_tool_name(TOOL_NAME) {
                panic!("built-in tool name `{TOOL_NAME}` fails SEP-986 validation: {e}");
            }
            McpTool {
                name: TOOL_NAME.to_string(),
                description: "Purge terminal (completed/failed/cancelled/interrupted) jobs \
                              older than `older_than_hours` hours from JobManager and any \
                              attached storage backend. Non-terminal (pending/running) jobs \
                              are never removed regardless of age. Returns {removed: <count>} \
                              as structured content. Idempotent — repeated calls with the \
                              same window return 0 once the pruning horizon is reached."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "older_than_hours": {
                            "type": "integer",
                            "minimum": 0,
                            "default": 24,
                            "description": "Prune terminal jobs whose last update is older \
                                            than this many hours. Default: 24."
                        }
                    },
                    "required": []
                }),
                output_schema: None,
                annotations: Some(McpToolAnnotations {
                    title: Some("Cleanup Completed Jobs".to_string()),
                    read_only_hint: Some(false),
                    destructive_hint: Some(true),
                    idempotent_hint: Some(true),
                    open_world_hint: Some(false),
                    deferred_hint: Some(false),
                }),
                meta: None,
            }
        },
    ]
}

/// Build the three opt-in meta-tools for the lazy-actions fast-path (#254).
///
/// All three tool names are bare, lower-snake and ≤ 16 chars — SEP-986
/// compliant and therefore legal to surface unprefixed in `tools/list`.
/// They are only emitted when [`AppState::lazy_actions`] is `true`.
fn build_lazy_action_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "list_actions".to_string(),
            description: "Returns every enabled action as a compact {id, summary, tags} record with no JSON schemas attached.\n\n\
                          When to use: The entry point of the lazy-actions fast-path (lazy_actions=true). Use it to enumerate candidates cheaply when the full tools/list would blow the token budget.\n\n\
                          How to use:\n\
                          - Filter with dcc and/or skill to narrow the list before fetching schemas.\n\
                          - Follow up with describe_action(id=...) for one action, then call_action(id=..., args=...) to invoke."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dcc": {
                        "type": "string",
                        "description": "DCC filter (e.g. maya, blender)."
                    },
                    "skill": {
                        "type": "string",
                        "description": "Skill-name filter to limit results to one skill."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("List Actions".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "describe_action".to_string(),
            description: "Returns the full JSON input schema and metadata for a single action, identical to what tools/list would surface for it.\n\n\
                          When to use: Step 2 of the lazy-actions flow — after list_actions has narrowed the candidate, fetch the schema for exactly one action before calling it.\n\n\
                          How to use:\n\
                          - Pass id exactly as reported by list_actions; unknown ids return an error.\n\
                          - Follow up with call_action(id=..., args=...) using the returned schema."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Action id as reported by list_actions."
                    }
                },
                "required": ["id"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Describe Action".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "call_action".to_string(),
            description: "Generic dispatcher that invokes any action by id with the given arguments, using the same code path as a native tools/call.\n\n\
                          When to use: Step 3 of the lazy-actions flow, or whenever you want to avoid inflating tools/list with every action. Semantically identical to calling the action's native tool name directly.\n\n\
                          How to use:\n\
                          - Make sure args matches the schema from describe_action; invalid args are rejected.\n\
                          - Side effects are those of the underlying action — check its ToolAnnotations first."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Action id (e.g. create_sphere or maya-geometry.create_sphere)."
                    },
                    "args": {
                        "type": "object",
                        "description": "Arguments matching the action's input_schema."
                    }
                },
                "required": ["id"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Call Action".to_string()),
                // `call_action` itself has no side effects beyond those of
                // the underlying action — so we inherit nothing and signal
                // the open-world hint so clients treat it defensively.
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(false),
                open_world_hint: Some(true),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
    ]
}

/// Convert an ActionMeta to an McpTool, respecting annotations from the skill.
///
/// `include_output_schema` controls whether the action's declared
/// [`ActionMeta::output_schema`] is surfaced as the MCP `outputSchema` field
/// (introduced in 2025-06-18). On older sessions this must be `false` so the
/// field is never serialised.
fn action_meta_to_mcp_tool(
    meta: &dcc_mcp_actions::registry::ActionMeta,
    include_output_schema: bool,
    bare_eligible: &std::collections::HashSet<(String, String)>,
) -> McpTool {
    let input_schema = if meta.input_schema.is_null() {
        json!({"type": "object"})
    } else {
        meta.input_schema.clone()
    };

    // Only surface a non-null schema. An explicit `null` from the action is
    // equivalent to "unspecified" and must not leak as `outputSchema: null`
    // (which some clients treat as a hard rejection).
    let output_schema = if include_output_schema && !meta.output_schema.is_null() {
        Some(meta.output_schema.clone())
    } else {
        None
    };

    // #307 — prefer the bare action name when the caller has deemed it
    // unique within this instance. Core tools and actions without a skill
    // still publish under their canonical form.
    let mcp_name = meta
        .skill_name
        .as_deref()
        .map(|sn| {
            let key = (sn.to_string(), meta.name.clone());
            if bare_eligible.contains(&key) {
                crate::gateway::namespace::extract_bare_tool_name(sn, &meta.name).to_string()
            } else {
                skill_tool_name(sn, &meta.name).unwrap_or_else(|| meta.name.clone())
            }
        })
        .unwrap_or_else(|| meta.name.clone());
    // Build the MCP `annotations` object from the skill-author declaration
    // (issue #344). Only hints that were explicitly declared appear in
    // the output — tools without any spec-standard annotations omit the
    // `annotations` field entirely instead of emitting an empty object.
    // `deferred_hint` is intentionally *not* placed inside the spec
    // annotations map — it rides in `_meta["dcc.deferred_hint"]` (set by
    // `build_tool_meta`), which keeps us MCP 2025-03-26 compliant.
    let declared = &meta.annotations;
    let annotations = if declared.is_spec_empty() {
        None
    } else {
        Some(McpToolAnnotations {
            title: declared.title.clone(),
            read_only_hint: declared.read_only_hint,
            destructive_hint: declared.destructive_hint,
            idempotent_hint: declared.idempotent_hint,
            open_world_hint: declared.open_world_hint,
            deferred_hint: None,
        })
    };

    McpTool {
        name: mcp_name,
        description: meta.description.clone(),
        input_schema,
        output_schema,
        annotations,
        meta: build_tool_meta(meta),
    }
}

/// Build the MCP `_meta` map for a tool definition (issues #317, #344).
///
/// Emits dcc-mcp-core-specific hints under a vendor-scoped `dcc.*` key so
/// future additions don't collide with spec-defined fields:
///
/// * `dcc.timeoutHintSecs` — when the skill author declared
///   `timeout_hint_secs` (issue #317).
/// * `dcc.deferred_hint` — when the tool is deferred. This is a
///   dcc-mcp-core extension (not part of MCP 2025-03-26), so it rides in
///   `_meta` instead of the spec `annotations` map (issue #344). The
///   value is `true` when either the skill author declared
///   `deferred_hint: true` in `tools.yaml` **or** the author declared
///   `execution: async` (which implies deferred).
///
/// Returns `None` when there is nothing to emit.
fn build_tool_meta(
    meta: &dcc_mcp_actions::registry::ActionMeta,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let deferred = meta
        .annotations
        .deferred_hint
        .unwrap_or_else(|| meta.execution.is_deferred());

    let has_timeout = meta.timeout_hint_secs.is_some();
    if !has_timeout && !deferred {
        return None;
    }

    let mut dcc_meta = serde_json::Map::new();
    if let Some(t) = meta.timeout_hint_secs {
        dcc_meta.insert("timeoutHintSecs".to_string(), serde_json::json!(t));
    }
    if deferred {
        dcc_meta.insert("deferred_hint".to_string(), serde_json::json!(true));
    }
    let mut out = serde_json::Map::new();
    out.insert("dcc".to_string(), serde_json::Value::Object(dcc_meta));
    Some(out)
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
        output_schema: None,
        // Skill stubs are not callable tools: they exist solely to hint the agent
        // to call `load_skill` first. Full annotation blocks add ~40-60 tokens
        // per stub × 64 skills = measurable `tools/list` bloat with zero routing
        // value for the model. (#235)
        annotations: None,
        meta: None,
    }
}

/// Handle `search_skills` — unified skill discovery tool (issue #340).
///
/// Input:
///   - `query`  (str, optional)     — substring match on name/description/search_hint/tool names
///   - `tags`   (list[str], optional) — every tag must match (AND)
///   - `dcc`    (str, optional)       — filter by DCC binding
///   - `scope`  (str, optional)       — `"repo" | "user" | "system" | "admin"`
///   - `limit`  (int, optional)       — cap results (default 20, max 100)
///
/// When all inputs are empty/None, returns the top `limit` skills sorted by
/// scope precedence (Admin > System > User > Repo) then name. This is the
/// "what skills are available?" discovery entry point for agents.
async fn handle_search_skills(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    const DEFAULT_LIMIT: usize = 20;
    const MAX_LIMIT: usize = 100;

    let args = params.arguments.as_ref();

    let query = args
        .and_then(|a| a.get("query"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let tags_owned: Vec<String> = args
        .and_then(|a| a.get("tags"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let tags: Vec<&str> = tags_owned.iter().map(String::as_str).collect();

    let dcc_filter = args.and_then(|a| a.get("dcc")).and_then(Value::as_str);

    let scope_filter = match args.and_then(|a| a.get("scope")).and_then(Value::as_str) {
        None => None,
        Some(s) => match parse_scope_label(s) {
            Ok(sc) => Some(sc),
            Err(msg) => {
                return Ok(JsonRpcResponse::success(
                    req.id.clone(),
                    serde_json::to_value(CallToolResult::error(msg))?,
                ));
            }
        },
    };

    let limit = args
        .and_then(|a| a.get("limit"))
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);

    let query_opt = if query.is_empty() { None } else { Some(query) };
    let matches =
        state
            .catalog
            .search_skills(query_opt, &tags, dcc_filter, scope_filter, Some(limit));

    if matches.is_empty() {
        let text = if query.is_empty()
            && tags.is_empty()
            && dcc_filter.is_none()
            && scope_filter.is_none()
        {
            "No skills discovered. Drop SKILL.md files into the scan paths and rescan.".to_string()
        } else if query.is_empty() {
            "No skills match the given filters.".to_string()
        } else {
            format!("No skills found matching '{query}'.")
        };
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::text(text))?,
        ));
    }

    // RTK-inspired: ultra-compact JSON format to reduce token consumption.
    // Keep the historical keys (`name`, `tools`, `loaded`, `dcc`) and add
    // `scope` / `description` / `tags` / `search_hint` so the union covers
    // what find_skills used to return.
    let compact_skills: Vec<serde_json::Value> = matches
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "description": s.description,
                "tools": s.tool_count,
                "loaded": s.loaded,
                "dcc": s.dcc,
                "scope": s.scope,
                "tags": s.tags,
                "search_hint": s.search_hint,
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

/// Parse the `scope` argument string into a [`SkillScope`].
fn parse_scope_label(s: &str) -> Result<SkillScope, String> {
    match s.to_ascii_lowercase().as_str() {
        "repo" => Ok(SkillScope::Repo),
        "user" => Ok(SkillScope::User),
        "system" => Ok(SkillScope::System),
        "admin" => Ok(SkillScope::Admin),
        other => Err(format!(
            "Invalid scope {other:?}: expected 'repo' | 'user' | 'system' | 'admin'"
        )),
    }
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
        output_schema: None,
        // Same rationale as `build_skill_stub`: group stubs are not callable
        // tools, so their annotations are pure protocol noise. (#235)
        annotations: None,
        meta: None,
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

// ── Built-in `jobs.get_status` (#319) ─────────────────────────────────────

/// Handle ``jobs.get_status`` — poll a tracked job's lifecycle state.
///
/// Returns the standard status envelope — ``{job_id, parent_job_id, tool,
/// status, created_at, started_at, completed_at, progress, error, result}``
/// — mirroring the field names emitted on the ``$/dcc.jobUpdated`` SSE
/// channel (#326) so clients can mix polling and streaming freely.
///
/// Semantics:
///
/// * Missing / empty ``job_id`` → ``isError=true`` with a human-readable
///   message (still a valid ``CallToolResult``, never a JSON-RPC error).
/// * Unknown ``job_id`` → ``isError=true`` naming the bad id.
/// * ``include_result=false`` or job not terminal → ``result`` is omitted.
/// * ``include_logs=true`` is accepted for forward compatibility —
///   ``JobManager`` does not currently capture per-job stdout/stderr, so
///   the flag is a no-op and a ``tracing::debug!`` breadcrumb is emitted.
async fn handle_jobs_get_status(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();
    let job_id = args
        .and_then(|a| a.get("job_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if job_id.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "Missing required parameter: job_id".to_string(),
            ))?,
        ));
    }
    let include_logs = args
        .and_then(|a| a.get("include_logs"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_result = args
        .and_then(|a| a.get("include_result"))
        .and_then(Value::as_bool)
        .unwrap_or(true);

    if include_logs {
        // #319: accepted for forward-compat. JobManager does not capture
        // stdout/stderr today; document the reality instead of silently
        // pretending to honour the flag.
        tracing::debug!(
            job_id = %job_id,
            "jobs.get_status received include_logs=true — no-op, JobManager does not capture logs"
        );
    }

    let Some(entry) = state.jobs.get(job_id) else {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(format!(
                "No job found with id '{job_id}'"
            )))?,
        ));
    };
    let job = entry.read();

    // Build the envelope. Field order / names mirror `$/dcc.jobUpdated`
    // (see `notifications.rs`) so polling clients see the same shape as
    // streaming subscribers.
    let (started_at, completed_at) = compute_job_timestamps(&job);
    let mut envelope = serde_json::Map::new();
    envelope.insert("job_id".into(), Value::String(job.id.clone()));
    envelope.insert(
        "parent_job_id".into(),
        match &job.parent_job_id {
            Some(p) => Value::String(p.clone()),
            None => Value::Null,
        },
    );
    envelope.insert("tool".into(), Value::String(job.tool_name.clone()));
    envelope.insert(
        "status".into(),
        serde_json::to_value(job.status).unwrap_or(Value::Null),
    );
    envelope.insert(
        "created_at".into(),
        Value::String(job.created_at.to_rfc3339()),
    );
    envelope.insert(
        "started_at".into(),
        started_at
            .map(|t| Value::String(t.to_rfc3339()))
            .unwrap_or(Value::Null),
    );
    envelope.insert(
        "completed_at".into(),
        completed_at
            .map(|t| Value::String(t.to_rfc3339()))
            .unwrap_or(Value::Null),
    );
    envelope.insert(
        "updated_at".into(),
        Value::String(job.updated_at.to_rfc3339()),
    );
    envelope.insert(
        "progress".into(),
        serde_json::to_value(&job.progress).unwrap_or(Value::Null),
    );
    envelope.insert(
        "error".into(),
        match &job.error {
            Some(e) => Value::String(e.clone()),
            None => Value::Null,
        },
    );
    if include_result && job.status.is_terminal() {
        if let Some(ref r) = job.result {
            envelope.insert("result".into(), r.clone());
        }
    }
    drop(job);

    let envelope_value = Value::Object(envelope);
    let text = serde_json::to_string(&envelope_value)?;
    let tool_result = CallToolResult {
        content: vec![crate::protocol::ToolContent::Text { text }],
        structured_content: Some(envelope_value),
        is_error: false,
        meta: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(tool_result)?,
    ))
}

// ── Built-in `jobs.cleanup` (#328) ────────────────────────────────────────

/// Handle ``jobs.cleanup`` — TTL prune terminal jobs from JobManager
/// and any attached storage backend (issue #328).
///
/// Semantics:
/// * `older_than_hours` defaults to 24. Values of 0 prune every
///   terminal row that already exists (useful for tests).
/// * Non-terminal (pending/running) rows are never touched.
/// * Returns a ``{removed: <count>}`` envelope both as text and
///   `structuredContent`.
async fn handle_jobs_cleanup(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();
    let older_than_hours = args
        .and_then(|a| a.get("older_than_hours"))
        .and_then(Value::as_u64)
        .unwrap_or(24);
    let removed = state.jobs.cleanup_older_than_hours(older_than_hours);
    let envelope = serde_json::json!({
        "removed": removed,
        "older_than_hours": older_than_hours,
    });
    let text = serde_json::to_string(&envelope)?;
    let tool_result = CallToolResult {
        content: vec![crate::protocol::ToolContent::Text { text }],
        structured_content: Some(envelope),
        is_error: false,
        meta: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(tool_result)?,
    ))
}

/// Derive ``started_at`` and ``completed_at`` from a [`Job`] snapshot.
///
/// `JobManager` does not store these explicitly — it keeps only
/// `created_at` + `updated_at` + current `status`. For the public
/// envelope (#319 / #326) we reconstruct them:
/// * `started_at` is `updated_at` once the job has left `Pending`.
/// * `completed_at` is `updated_at` once the job is terminal.
fn compute_job_timestamps(
    job: &crate::job::Job,
) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    use crate::job::JobStatus;
    let started_at = match job.status {
        JobStatus::Pending => None,
        _ => Some(job.updated_at),
    };
    let completed_at = if job.status.is_terminal() {
        Some(job.updated_at)
    } else {
        None
    };
    (started_at, completed_at)
}

// ── Lazy-actions fast-path (#254) ─────────────────────────────────────────

/// Handle ``list_actions`` — compact action catalog without JSON schemas.
///
/// Returns one JSON object per enabled action, containing **only** the
/// three fields needed for an agent to decide whether to follow up with
/// ``describe_action`` / ``call_action``:
///
/// ```text
/// {"id": <full tool name>, "summary": <description>, "tags": [...]}
/// ```
///
/// Deliberately omits the input/output schemas — surfacing them here
/// would defeat the whole point of the fast-path (1/10 token target).
async fn handle_list_actions(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();
    let dcc = args.and_then(|a| a.get("dcc")).and_then(Value::as_str);
    let skill_filter = args.and_then(|a| a.get("skill")).and_then(Value::as_str);

    let mut items: Vec<Value> = Vec::new();
    for meta in state.registry.list_actions(dcc) {
        if !meta.enabled {
            continue;
        }
        if let Some(want) = skill_filter
            && meta.skill_name.as_deref() != Some(want)
        {
            continue;
        }
        // Action id is the canonical tool name — matches what `tools/list`
        // would have emitted, so `call_action(id=...)` is interchangeable
        // with a direct `tools/call { name: id }`.
        let id = meta
            .skill_name
            .as_deref()
            .and_then(|sn| skill_tool_name(sn, &meta.name))
            .unwrap_or_else(|| meta.name.clone());
        items.push(json!({
            "id": id,
            "summary": meta.description,
            "tags": meta.tags,
        }));
    }

    let payload = json!({
        "total": items.len(),
        "actions": items,
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string(&payload)?))?,
    ))
}

/// Handle ``describe_action`` — full JSON schema for a single action.
async fn handle_describe_action(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let id = match params
        .arguments
        .as_ref()
        .and_then(|a| a.get("id"))
        .and_then(Value::as_str)
    {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error("Missing required parameter: id"))?,
            ));
        }
    };

    // Accept both the canonical skill-prefixed id (what `list_actions`
    // returns) and the bare registry name, so the agent can round-trip
    // through either `tools/list` or the fast-path.
    let meta = resolve_action_by_id(&state.registry, &id);

    let Some(meta) = meta else {
        let envelope = DccMcpError::new(
            "registry",
            "ACTION_NOT_FOUND",
            format!("Unknown action id: {id}"),
        )
        .with_hint("Call list_actions to see available ids.");
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    };

    // Mirror the exact shape `tools/list` would have produced for this
    // action so agents can reuse a single parser.
    let include_output_schema = session_id
        .and_then(|sid| state.sessions.get_protocol_version(sid))
        .as_deref()
        == Some("2025-06-18");
    // `describe_action` is a single-action view — passing an empty
    // bare-eligible set keeps it on the canonical `<skill>.<action>` form
    // rather than synthesising a bare name that might collide against a
    // peer action the caller didn't ask about.
    let bare_eligible_for_describe = std::collections::HashSet::new();
    let tool = action_meta_to_mcp_tool(&meta, include_output_schema, &bare_eligible_for_describe);
    let payload = serde_json::to_value(tool)?;

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string(&payload)?))?,
    ))
}

/// Handle ``call_action`` — generic dispatcher that delegates to the same
/// tool-call path as a direct `tools/call`.
///
/// Implementation strategy: rewrite the incoming request's ``params``
/// into a standard ``CallToolParams { name: id, arguments: args }`` shape
/// and recurse into [`handle_tools_call`]. Because the recursion target
/// rejects ``list_actions`` / ``describe_action`` / ``call_action`` names
/// (the dispatch branch only matches when `state.lazy_actions` is true
/// **and** the name is one of the three), we guard against infinite
/// recursion by rejecting those three names explicitly.
async fn handle_call_action(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();
    let id = match args.and_then(|a| a.get("id")).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error("Missing required parameter: id"))?,
            ));
        }
    };

    // Guard: refuse to call the fast-path meta-tools through themselves.
    // This also makes their discoverability less surprising — the agent
    // cannot recurse into `call_action(id="call_action")`.
    if matches!(
        id.as_str(),
        "list_actions" | "describe_action" | "call_action"
    ) {
        let envelope = DccMcpError::new(
            "registry",
            "RECURSIVE_META_CALL",
            format!("`call_action` refuses to dispatch meta-tool `{id}`."),
        )
        .with_hint("Call the meta-tool directly via tools/call instead.");
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    }

    let inner_args = args.and_then(|a| a.get("args")).cloned();

    // Build a synthetic request that looks identical to a direct
    // `tools/call` on the target action. Preserving the original
    // JSON-RPC id/meta keeps progress/cancellation tokens working.
    let inner_params = CallToolParams {
        name: id,
        arguments: inner_args,
        meta: params.meta.clone(),
    };
    let inner_req = JsonRpcRequest {
        jsonrpc: req.jsonrpc.clone(),
        id: req.id.clone(),
        method: "tools/call".to_string(),
        params: Some(serde_json::to_value(&inner_params)?),
    };

    // `Box::pin` is required because this async fn would otherwise form a
    // recursion cycle with `handle_tools_call` (which routes back into us
    // on the `call_action` branch). The meta-tool guard above guarantees
    // the recursion terminates in one step — we only ever call through
    // to a real action.
    // Recurse through the `_inner` variant — the outer wrapper has
    // already started the Prometheus timer for this request; letting
    // the recursion hit the wrapper again would double-count.
    Box::pin(handle_tools_call_inner(state, &inner_req, session_id)).await
}

/// Look up an action by the id surfaced in `list_actions` (canonical
/// `<skill>.<tool>` form or bare registry name), returning a cloned
/// [`ActionMeta`] for downstream inspection.
fn resolve_action_by_id(
    registry: &dcc_mcp_actions::registry::ActionRegistry,
    id: &str,
) -> Option<dcc_mcp_actions::registry::ActionMeta> {
    // Fast path: direct registry hit (happens for bare action names).
    if let Some(m) = registry.get_action(id, None) {
        return Some(m);
    }
    // Canonical `<skill>.<tool>` form — decode and match by skill.
    if let Some((skill_part, bare_tool)) = decode_skill_tool_name(id) {
        return registry
            .list_actions_by_skill(skill_part)
            .into_iter()
            .find(|m| extract_bare_tool_name(skill_part, &m.name) == bare_tool);
    }
    None
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

/// Emit an MCP `notifications/message` event when the message level passes the
/// session threshold. Every message is still retained for `details.log_tail`.
fn notify_message(sessions: &SessionManager, session_id: &str, entry: SessionLogMessage) {
    let emit_level = entry.level;
    let request_id = entry.request_id.clone();
    let logger = entry.logger.clone();
    let data = entry.data.clone();
    let _ = sessions.push_log_message(session_id, entry);

    let threshold = sessions.get_log_level(session_id);
    if !threshold.allows(emit_level) {
        return;
    }

    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/message",
        "params": {
            "level": emit_level.as_str(),
            "logger": logger.clone(),
            "data": data,
        },
    });
    let event = format_sse_event(&notification, None);
    sessions.push_event(session_id, event);
    tracing::debug!(
        session_id,
        level = emit_level.as_str(),
        logger,
        request_id = request_id.unwrap_or_default(),
        "Sent notifications/message"
    );
}

fn request_id_to_string(id: Option<&Value>) -> Option<String> {
    let id = id?;
    let s = match id {
        Value::String(v) => v.clone(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    if s.is_empty() { None } else { Some(s) }
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

async fn refresh_roots_cache_for_session(
    sessions: &SessionManager,
    session_id: &str,
) -> Vec<crate::protocol::ClientRoot> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": format!("roots-refresh-{session_id}"),
        "method": "roots/list",
        "params": {}
    });
    let event = format_sse_event(&request, None);
    sessions.push_event(session_id, event);

    // Current low-risk phase: opportunistically keep existing cache.
    // Full client response correlation will be added in follow-up.
    let _ = tokio::time::timeout(ROOTS_REFRESH_TIMEOUT, async {}).await;
    sessions.get_client_roots(session_id)
}

#[cfg(test)]
mod issue_317_tests {
    //! Issues #317 and #344 — `execution` / `timeout_hint_secs` / annotation plumbing.
    use super::*;
    use dcc_mcp_actions::registry::ActionMeta;
    use dcc_mcp_models::{ExecutionMode, ToolAnnotations};

    fn empty_eligible() -> std::collections::HashSet<(String, String)> {
        std::collections::HashSet::new()
    }

    #[test]
    fn sync_action_without_annotations_omits_both_fields() {
        // Issue #344 — tools with no declared annotations omit the spec
        // `annotations` field entirely. `deferred_hint` is a dcc-mcp-core
        // extension that rides in `_meta` (never in the spec `annotations`
        // map) and for a sync tool it is simply absent.
        let meta = ActionMeta {
            name: "quick".into(),
            description: "Fast".into(),
            execution: ExecutionMode::Sync,
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible());
        assert!(
            tool.annotations.is_none(),
            "tools without declared annotations must omit the field"
        );
        assert!(tool.meta.is_none(), "sync, no timeout → no _meta");
    }

    #[test]
    fn async_action_surfaces_deferred_hint_in_meta_only() {
        // deferred_hint MUST land in _meta["dcc.deferred_hint"] and NEVER
        // inside the spec `annotations` map (issue #344).
        let meta = ActionMeta {
            name: "render".into(),
            description: "Render".into(),
            execution: ExecutionMode::Async,
            timeout_hint_secs: Some(600),
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible());
        let v = serde_json::to_value(&tool).unwrap();

        assert_eq!(
            v.pointer("/_meta/dcc/deferred_hint")
                .and_then(|x| x.as_bool()),
            Some(true),
            "deferred_hint must surface in _meta",
        );
        assert_eq!(
            v.pointer("/_meta/dcc/timeoutHintSecs")
                .and_then(|x| x.as_u64()),
            Some(600),
        );
        assert!(
            v.pointer("/annotations/deferredHint").is_none(),
            "deferredHint must never appear inside spec annotations",
        );
    }

    #[test]
    fn timeout_hint_emitted_even_when_sync() {
        let meta = ActionMeta {
            name: "measured".into(),
            description: "Sync with timeout hint".into(),
            execution: ExecutionMode::Sync,
            timeout_hint_secs: Some(30),
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible());
        let m = tool.meta.as_ref().unwrap();
        assert_eq!(
            m.get("dcc")
                .and_then(|v| v.get("timeoutHintSecs"))
                .and_then(|v| v.as_u64()),
            Some(30),
        );
        // No deferred_hint in _meta for sync with no explicit async flag.
        assert!(m.get("dcc").and_then(|v| v.get("deferred_hint")).is_none(),);
    }

    #[test]
    fn declared_annotations_surface_as_camelcase_with_spec_keys_only() {
        // Issue #344 — skill-author-declared annotations surface on
        // `tools/list` with spec-compliant camelCase keys. `deferred_hint`
        // from the declaration is routed into `_meta` and MUST NOT
        // contaminate the spec `annotations` map.
        let meta = ActionMeta {
            name: "delete_keyframes".into(),
            description: "danger".into(),
            execution: ExecutionMode::Sync,
            annotations: ToolAnnotations {
                title: Some("Delete Keyframes".into()),
                read_only_hint: Some(false),
                destructive_hint: Some(true),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(true),
            },
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible());
        let v = serde_json::to_value(&tool).unwrap();

        assert_eq!(
            v.pointer("/annotations/destructiveHint")
                .and_then(|x| x.as_bool()),
            Some(true)
        );
        assert_eq!(
            v.pointer("/annotations/readOnlyHint")
                .and_then(|x| x.as_bool()),
            Some(false)
        );
        assert_eq!(
            v.pointer("/annotations/idempotentHint")
                .and_then(|x| x.as_bool()),
            Some(true)
        );
        assert_eq!(
            v.pointer("/annotations/openWorldHint")
                .and_then(|x| x.as_bool()),
            Some(false)
        );
        assert_eq!(
            v.pointer("/annotations/title").and_then(|x| x.as_str()),
            Some("Delete Keyframes")
        );
        assert!(
            v.pointer("/annotations/deferredHint").is_none(),
            "deferredHint must live in _meta, not spec annotations"
        );
        assert_eq!(
            v.pointer("/_meta/dcc/deferred_hint")
                .and_then(|x| x.as_bool()),
            Some(true),
        );
    }

    #[test]
    fn partial_annotations_only_emit_declared_keys() {
        // Undeclared hints are omitted entirely — not defaulted to false.
        let meta = ActionMeta {
            name: "get_keyframes".into(),
            description: "read only".into(),
            annotations: ToolAnnotations {
                read_only_hint: Some(true),
                idempotent_hint: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible());
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(
            v.pointer("/annotations/readOnlyHint")
                .and_then(|x| x.as_bool()),
            Some(true)
        );
        assert_eq!(
            v.pointer("/annotations/idempotentHint")
                .and_then(|x| x.as_bool()),
            Some(true)
        );
        assert!(v.pointer("/annotations/destructiveHint").is_none());
        assert!(v.pointer("/annotations/openWorldHint").is_none());
    }
}
