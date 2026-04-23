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

pub(crate) mod handlers;
pub(crate) use handlers::*;

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
    /// DCC capabilities advertised by the hosting adapter (issue #354).
    ///
    /// Per-tool `required_capabilities` are checked against this set at
    /// `tools/call` time. Tools with missing capabilities surface
    /// `_meta.dcc.missing_capabilities` in `tools/list` and fail the call
    /// with JSON-RPC error `-32001 capability_missing`.
    pub declared_capabilities: Arc<Vec<String>>,
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

pub(crate) fn handle_response_message(state: &AppState, resp: &JsonRpcResponse) {
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

pub(crate) async fn dispatch_request(
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

pub(crate) async fn handle_initialize(
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


pub(crate) async fn handle_tools_list(
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
                state.declared_capabilities.as_ref(),
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
