//! Minimal MCP Streamable-HTTP listener inside the sidecar process.
//!
//! The gateway routes ``tools/call`` requests to a sidecar's MCP URL
//! instead of the per-DCC in-process URL whenever the sidecar is the
//! reachable endpoint for a given DCC instance (RFC #998 Phase 2).
//! This module is the listener that fronts the dispatch path.
//!
//! ## Scope (intentionally minimal)
//!
//! This is **not** a full MCP server. It implements just enough of
//! the Streamable-HTTP protocol that the gateway's `call_tool`
//! routing decision can land:
//!
//! | Method            | Behaviour |
//! |-------------------|-----------|
//! | `initialize`      | Returns a capability envelope advertising `tools: { listChanged: false }` only. |
//! | `tools/call`      | Dispatches via the in-process [`HostRpcClient`]; returns the result envelope verbatim. |
//! | `ping`            | Echo `{}` — needed by some hosts' health probes. |
//! | `notifications/*` | Accepted and discarded (per JSON-RPC, no response when `id` is absent). |
//! | everything else   | `-32601` "method not found". |
//!
//! Discovery (`tools/list`, `resources/read`) is intentionally NOT
//! served here. The gateway is the authoritative discovery surface;
//! the sidecar only handles the dispatch.
//!
//! ## Why a separate file
//!
//! `sidecar.rs` is the binary's lifecycle composition root (CLI,
//! FileRegistry, PPID-watch, HostRpcClient connect). Splitting the
//! HTTP listener out keeps each surface comprehensible — the test
//! contract for one is "the process lifecycle is correct" and for
//! the other is "the wire protocol is correct".

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use dcc_mcp_host_rpc::{HostRpcClient, HostRpcError};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, watch};

/// The MCP protocol version this listener speaks back to clients.
/// Pinned as a constant so test assertions cannot drift away from
/// what the gateway expects.
pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

/// `server_name` advertised in the `initialize` response. Stable
/// string so the gateway / admin UI can identify a sidecar-served
/// endpoint at a glance.
pub const SIDECAR_SERVER_NAME: &str = "dcc-mcp-sidecar";

/// Shared HTTP-handler state.
///
/// Held in an `Arc` so axum can clone it freely between requests;
/// the inner `Mutex` serialises access to the [`HostRpcClient`]
/// because most per-DCC transports (Maya `commandPort`,
/// Houdini `hrpyc`, …) are inherently single-flight.
#[derive(Clone)]
pub struct SidecarMcpState {
    pub(crate) host_rpc: Arc<Mutex<Box<dyn HostRpcClient>>>,
    pub(crate) server_version: String,
}

impl SidecarMcpState {
    /// Wrap a [`HostRpcClient`] for the HTTP handler.
    ///
    /// The common path passes a connected client. Startup-failure diagnostics
    /// may pass an unavailable client so `/v1/readyz` and `tools/call` can
    /// report structured failure details through the same listener.
    pub fn new(host_rpc: Box<dyn HostRpcClient>, server_version: impl Into<String>) -> Self {
        Self {
            host_rpc: Arc::new(Mutex::new(host_rpc)),
            server_version: server_version.into(),
        }
    }

    /// Tear down the inner client; useful for test fixtures that
    /// want to assert the close path explicitly. In production the
    /// listener's `shutdown` flow drives the close indirectly when
    /// it drops the last `Arc<Mutex<...>>` reference.
    #[allow(dead_code)]
    pub async fn close(&self) {
        let guard = self.host_rpc.lock().await;
        guard.close().await;
    }

    /// Replace the inner client after a previously-unavailable sidecar
    /// reconnects to the live DCC.
    pub(crate) async fn replace_host_rpc(&self, host_rpc: Box<dyn HostRpcClient>) {
        let mut guard = self.host_rpc.lock().await;
        guard.close().await;
        *guard = host_rpc;
    }
}

/// Handle returned by [`spawn_listener`].
///
/// Owns the resolved bind address (so the caller can stamp it into
/// the FileRegistry row) and a `watch` channel that, when set to
/// `()`, signals the axum server to shut down gracefully.
pub struct SidecarMcpListenerHandle {
    pub bind_addr: SocketAddr,
    pub mcp_url: String,
    pub join: tokio::task::JoinHandle<()>,
    pub shutdown_tx: watch::Sender<()>,
}

impl SidecarMcpListenerHandle {
    /// Trigger graceful shutdown and wait for the listener task to
    /// finish (with a hard timeout so a stuck axum cannot block the
    /// sidecar's main exit path forever).
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        if (tokio::time::timeout(Duration::from_secs(5), self.join).await).is_err() {
            tracing::warn!(
                bind_addr = %self.bind_addr,
                "sidecar MCP listener did not exit within 5s; abandoning"
            );
        }
    }
}

/// Bind an MCP HTTP listener on `host:port` (`port = 0` ⇒ OS-assigned)
/// and start serving in the background.
///
/// Returns once the listener is **proven accepting** — the
/// `TcpListener::bind` succeeded and the local address has been
/// resolved. Errors at this stage are returned synchronously so the
/// sidecar's run loop can decide whether to fall back (e.g. retry
/// on a different port) or abort.
pub async fn spawn_listener(
    state: SidecarMcpState,
    host: &str,
    port: u16,
) -> anyhow::Result<SidecarMcpListenerHandle> {
    let listener = TcpListener::bind((host, port))
        .await
        .map_err(|e| anyhow::anyhow!("sidecar MCP bind {host}:{port}: {e}"))?;
    let bind_addr = listener.local_addr()?;
    let mcp_url = format!("http://{}/mcp", bind_addr);

    let router = Router::new()
        .route("/mcp", post(handle_mcp_post))
        .route("/health", get(handle_health))
        .route("/healthz", get(handle_healthz))
        .route("/v1/healthz", get(handle_v1_healthz))
        .route("/v1/readyz", get(handle_v1_readyz))
        .with_state(state);

    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let mut shutdown_rx_for_task = shutdown_rx.clone();
    // Mark the seeded value as already-read so the first `.changed()`
    // only fires when the caller actually invokes ``shutdown_tx.send``.
    shutdown_rx_for_task.borrow_and_update();

    let join = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx_for_task.changed().await;
            })
            .await
        {
            tracing::error!(error = %e, "sidecar MCP listener exited with error");
        }
    });

    Ok(SidecarMcpListenerHandle {
        bind_addr,
        mcp_url,
        join,
        shutdown_tx,
    })
}

// ── request shapes ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ToolsCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

// ── handlers ────────────────────────────────────────────────────────

async fn handle_health() -> Response {
    (StatusCode::OK, axum::Json(json!({"ok": true}))).into_response()
}

async fn handle_healthz() -> Response {
    (StatusCode::OK, "ok").into_response()
}

async fn handle_v1_healthz() -> Response {
    (StatusCode::OK, axum::Json(json!({"ok": true}))).into_response()
}

async fn handle_v1_readyz(State(state): State<SidecarMcpState>) -> Response {
    let dispatcher_ready = match state.host_rpc.try_lock() {
        Ok(guard) => guard.is_alive(),
        // A locked dispatcher is busy serving a call, not unavailable. Keep
        // readiness probes non-blocking so long DCC calls do not look like a
        // dead sidecar.
        Err(_) => true,
    };
    let status = if dispatcher_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        axum::Json(json!({
            "process": true,
            "dispatcher": dispatcher_ready,
            "dcc": dispatcher_ready,
        })),
    )
        .into_response()
}

async fn handle_mcp_post(
    State(state): State<SidecarMcpState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("sc-{}", uuid::Uuid::new_v4().simple()));

    let value: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return parse_error(&session_id, format!("parse error: {e}")),
    };

    let req: JsonRpcRequest = match serde_json::from_value(value) {
        Ok(r) => r,
        Err(e) => return parse_error(&session_id, format!("not a JSON-RPC request: {e}")),
    };

    // Notifications have no id — we accept and discard.
    if req.id.is_none() {
        return (StatusCode::ACCEPTED).into_response();
    }

    let id = req.id.clone().unwrap_or(Value::Null);
    let body = dispatch(&state, &headers, &req, id).await;
    let mut response = axum::Json(body).into_response();
    attach_session(&mut response, &session_id);
    response
}

async fn dispatch(
    state: &SidecarMcpState,
    headers: &HeaderMap,
    req: &JsonRpcRequest,
    id: Value,
) -> Value {
    match req.method.as_str() {
        "initialize" => initialize_response(id, &state.server_version),
        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        "tools/call" => handle_tools_call(state, headers, id, req).await,
        other => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("method not found: {other:?}"),
                "data": {
                    "supported": ["initialize", "ping", "tools/call"],
                    "note": "sidecar serves dispatch only; use the gateway for discovery"
                }
            }
        }),
    }
}

fn initialize_response(id: Value, server_version: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": {"listChanged": false}
            },
            "serverInfo": {
                "name": SIDECAR_SERVER_NAME,
                "version": server_version
            },
            "instructions": "dcc-mcp-server sidecar — dispatches tools/call to a single DCC instance via its native RPC channel. Discovery happens at the gateway."
        }
    })
}

async fn handle_tools_call(
    state: &SidecarMcpState,
    headers: &HeaderMap,
    id: Value,
    req: &JsonRpcRequest,
) -> Value {
    let params: ToolsCallParams = match req
        .params
        .clone()
        .map(serde_json::from_value::<ToolsCallParams>)
        .transpose()
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32602, "message": "tools/call requires params"}
            });
        }
        Err(e) => {
            return json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32602, "message": format!("invalid params: {e}")}
            });
        }
    };

    // Use the JSON-RPC `id` (stringified) as the request_id the
    // HostRpcClient propagates to the DCC. The DCC echoes it back
    // in the result envelope so async correlation works end-to-end.
    let request_id = match &id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    let result = {
        let guard = state.host_rpc.lock().await;
        guard
            .call_with_trace_context(
                &params.name,
                params.arguments,
                &request_id,
                trace_context_from_headers(headers, &request_id),
            )
            .await
    };

    match result {
        Ok(payload) => json!({
            "jsonrpc": "2.0", "id": id,
            "result": payload
        }),
        Err(err) => host_rpc_error_to_jsonrpc(id, err),
    }
}

fn trace_context_from_headers(headers: &HeaderMap, request_id: &str) -> Option<Value> {
    let traceparent = header_str(headers, "traceparent");
    let trace_id = traceparent
        .as_deref()
        .and_then(parse_traceparent_trace_id)
        .or_else(|| header_str(headers, "x-trace-id"));
    let trace_id = trace_id?;
    let parent_span_id = traceparent
        .as_deref()
        .and_then(parse_traceparent_parent_span_id);
    let trace_flags = traceparent.as_deref().and_then(parse_traceparent_flags);
    Some(json!({
        "trace_id": trace_id,
        "request_id": request_id,
        "parent_span_id": parent_span_id,
        "parent_request_id": header_str(headers, "x-dcc-mcp-parent-request-id"),
        "trace_flags": trace_flags,
        "trace_state": header_str(headers, "tracestate"),
    }))
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_traceparent_trace_id(value: &str) -> Option<String> {
    let trace_id = value.trim().split('-').nth(1)?;
    (trace_id.len() == 32).then(|| trace_id.to_ascii_lowercase())
}

fn parse_traceparent_parent_span_id(value: &str) -> Option<String> {
    let span_id = value.trim().split('-').nth(2)?;
    (span_id.len() == 16).then(|| span_id.to_ascii_lowercase())
}

fn parse_traceparent_flags(value: &str) -> Option<String> {
    let flags = value.trim().split('-').nth(3)?;
    (flags.len() == 2).then(|| flags.to_ascii_lowercase())
}

fn host_rpc_error_to_jsonrpc(id: Value, err: HostRpcError) -> Value {
    let (code, message, data) = match err {
        HostRpcError::HostDied {
            last_call_slug,
            last_call_args,
        } => (
            -32000,
            "host-died".to_string(),
            json!({
                "kind": "host-died",
                "last_call_slug": last_call_slug,
                "last_call_args": last_call_args,
                "guidance": "the DCC process disconnected mid-call; the gateway will evict this backend"
            }),
        ),
        HostRpcError::TransportError { message } => (
            -32000,
            "transport-error".to_string(),
            json!({"kind": "transport-error", "message": message}),
        ),
        HostRpcError::Timeout {} => (-32000, "timeout".to_string(), json!({"kind": "timeout"})),
        HostRpcError::Cancelled {} => (
            -32000,
            "cancelled".to_string(),
            json!({"kind": "cancelled"}),
        ),
        HostRpcError::BackendError { envelope } => (
            -32000,
            "backend-error".to_string(),
            json!({"kind": "backend-error", "envelope": envelope}),
        ),
    };

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message, "data": data}
    })
}

fn parse_error(session_id: &str, message: String) -> Response {
    let body = json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {"code": -32700, "message": message}
    });
    let mut response = axum::Json(body).into_response();
    attach_session(&mut response, session_id);
    response
}

fn attach_session(response: &mut Response, session_id: &str) {
    if let Ok(value) = HeaderValue::from_str(session_id) {
        response.headers_mut().insert("Mcp-Session-Id", value);
    }
}

// ── tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_host_rpc::{StubHostRpcClient, UnavailableHostRpcClient};
    use std::time::Duration;

    async fn connected_stub_client() -> Box<dyn HostRpcClient> {
        let mut client = StubHostRpcClient::new();
        client
            .connect("stub://localhost", Duration::from_millis(10))
            .await
            .expect("connect stub client");
        Box::new(client)
    }

    async fn fresh_listener() -> SidecarMcpListenerHandle {
        let client = connected_stub_client().await;
        let state = SidecarMcpState::new(client, "test-0.0.0");
        spawn_listener(state, "127.0.0.1", 0)
            .await
            .expect("spawn_listener")
    }

    async fn post_mcp(url: &str, body: Value) -> reqwest::Response {
        reqwest::Client::new()
            .post(url)
            .json(&body)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .expect("POST /mcp")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn healthz_responds_ok() {
        let handle = fresh_listener().await;
        let response = reqwest::Client::new()
            .get(format!("http://{}/healthz", handle.bind_addr))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("GET /healthz");
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.unwrap(), "ok");
        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn health_aliases_support_gateway_probes() {
        let handle = fresh_listener().await;
        let client = reqwest::Client::new();

        let health: Value = client
            .get(format!("http://{}/health", handle.bind_addr))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("GET /health")
            .json()
            .await
            .expect("parse /health");
        assert_eq!(health["ok"], true);

        let healthz: Value = client
            .get(format!("http://{}/v1/healthz", handle.bind_addr))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("GET /v1/healthz")
            .json()
            .await
            .expect("parse /v1/healthz");
        assert_eq!(healthz["ok"], true);

        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn readyz_reports_fully_ready() {
        let handle = fresh_listener().await;
        let response = reqwest::Client::new()
            .get(format!("http://{}/v1/readyz", handle.bind_addr))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("GET /v1/readyz");
        assert_eq!(response.status().as_u16(), 200);
        let body: Value = response.json().await.expect("parse /v1/readyz");

        assert_eq!(body["process"], true);
        assert_eq!(body["dispatcher"], true);
        assert_eq!(body["dcc"], true);
        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn readyz_reports_dispatcher_unavailable() {
        let client: Box<dyn HostRpcClient> = Box::new(UnavailableHostRpcClient::new(
            "host-rpc connect to `qtserver://127.0.0.1:1` failed",
        ));
        let state = SidecarMcpState::new(client, "test-0.0.0");
        let handle = spawn_listener(state, "127.0.0.1", 0).await.expect("spawn");

        let response = reqwest::Client::new()
            .get(format!("http://{}/v1/readyz", handle.bind_addr))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("GET /v1/readyz");
        assert_eq!(response.status().as_u16(), 503);
        let body: Value = response.json().await.expect("parse /v1/readyz");

        assert_eq!(body["process"], true);
        assert_eq!(body["dispatcher"], false);
        assert_eq!(body["dcc"], false);
        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn initialize_returns_negotiated_protocol_version() {
        let handle = fresh_listener().await;
        let body: Value = post_mcp(
            &handle.mcp_url,
            json!({
                "jsonrpc": "2.0", "id": 1,
                "method": "initialize",
                "params": {"protocolVersion": "2025-03-26"}
            }),
        )
        .await
        .json()
        .await
        .expect("parse JSON");

        assert_eq!(body["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(body["result"]["serverInfo"]["name"], SIDECAR_SERVER_NAME);
        assert_eq!(body["result"]["serverInfo"]["version"], "test-0.0.0");
        // tools.listChanged: false — we intentionally don't promise
        // discovery here; gateway is the discovery surface.
        assert_eq!(
            body["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ping_echoes_empty_result() {
        let handle = fresh_listener().await;
        let body: Value = post_mcp(
            &handle.mcp_url,
            json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
        )
        .await
        .json()
        .await
        .expect("parse JSON");

        assert_eq!(body["id"], 2);
        assert!(body["result"].is_object());
        assert_eq!(body["result"].as_object().unwrap().len(), 0);
        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tools_call_routes_through_host_rpc_client() {
        // StubHostRpcClient::call always returns transport("stub client")
        // and records the action slug — so we can assert the
        // listener forwarded the right slug to the client.
        let stub = Arc::new(StubHostRpcClient::new());
        let stub_for_state: Box<dyn HostRpcClient> = Box::new(StubHostRpcClient::new());
        // We can't share Arc<StubHostRpcClient> directly because the
        // listener owns its own copy via Box<dyn>. So we make a
        // separate stub and verify via the response error instead —
        // the wire path is what we want to pin.
        drop(stub);

        let state = SidecarMcpState::new(stub_for_state, "test");
        let handle = spawn_listener(state, "127.0.0.1", 0).await.expect("spawn");

        let body: Value = post_mcp(
            &handle.mcp_url,
            json!({
                "jsonrpc": "2.0", "id": "req-1",
                "method": "tools/call",
                "params": {
                    "name": "maya_primitives__create_sphere",
                    "arguments": {"radius": 1.0}
                }
            }),
        )
        .await
        .json()
        .await
        .expect("parse JSON");

        // Stub client returns TransportError("stub client") — that
        // travels through our error envelope into the JSON-RPC error
        // structure. Pin the wire shape.
        assert_eq!(body["id"], "req-1");
        assert!(
            body["error"].is_object(),
            "tools/call against stub must produce JSON-RPC error envelope: {body}"
        );
        assert_eq!(body["error"]["code"], -32000);
        assert_eq!(body["error"]["message"], "transport-error");
        assert_eq!(body["error"]["data"]["kind"], "transport-error");
        assert!(
            body["error"]["data"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("stub client"),
            "data.message should propagate the transport error: {body}"
        );

        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_method_returns_method_not_found() {
        let handle = fresh_listener().await;
        let body: Value = post_mcp(
            &handle.mcp_url,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        )
        .await
        .json()
        .await
        .expect("parse JSON");

        assert_eq!(body["error"]["code"], -32601);
        // Hint to the agent that this is not a generic MCP server.
        assert!(body["error"]["data"]["note"].is_string());
        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn notification_is_accepted_without_response_body() {
        let handle = fresh_listener().await;
        let response = post_mcp(
            &handle.mcp_url,
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }),
        )
        .await;
        // Notifications: 202 Accepted, empty body (RFC 8259 §4 / MCP
        // Streamable HTTP). Distinguishes from request responses
        // which always carry id + result|error.
        assert_eq!(response.status(), 202);
        let body = response.text().await.unwrap();
        assert!(
            body.is_empty(),
            "notification body should be empty, got: {body:?}"
        );
        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn parse_error_carries_jsonrpc_minus_32700() {
        let handle = fresh_listener().await;
        let response = reqwest::Client::new()
            .post(&handle.mcp_url)
            .body("{ not even json")
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("POST malformed");
        // Servers SHOULD return 200 with a JSON-RPC error envelope
        // for parse errors (per JSON-RPC 2.0 §5.1). Some hosts treat
        // 4xx as transport failure so the envelope path is more
        // diagnosable.
        let body: Value = response.json().await.expect("parse JSON");
        assert_eq!(body["error"]["code"], -32700);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_id_is_echoed_back_in_header() {
        let handle = fresh_listener().await;
        let response = reqwest::Client::new()
            .post(&handle.mcp_url)
            .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
            .header("Mcp-Session-Id", "pinned-session-42")
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect("POST");
        let echoed = response
            .headers()
            .get("Mcp-Session-Id")
            .expect("server must echo Mcp-Session-Id")
            .to_str()
            .unwrap();
        assert_eq!(echoed, "pinned-session-42");
        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_initialize_requests_all_succeed() {
        // Pins the contract from #1009: multiple concurrent MCP
        // clients attaching to the same listener must each get a
        // negotiated `initialize` response without serialization-
        // induced timeouts. The HostRpcClient mutex only matters for
        // `tools/call`; `initialize` should be lock-free.
        let handle = fresh_listener().await;
        let mut handles = Vec::new();
        for client_idx in 0..8 {
            let url = handle.mcp_url.clone();
            handles.push(tokio::spawn(async move {
                post_mcp(
                    &url,
                    json!({
                        "jsonrpc": "2.0",
                        "id": client_idx,
                        "method": "initialize",
                        "params": {"protocolVersion": "2025-03-26"}
                    }),
                )
                .await
                .json::<Value>()
                .await
                .expect("parse")
            }));
        }
        for h in handles {
            let body = tokio::time::timeout(Duration::from_secs(2), h)
                .await
                .expect("each initialize must complete within 2s")
                .expect("task did not panic");
            assert_eq!(body["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        }
        handle.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_stops_listener_quickly() {
        let handle = fresh_listener().await;
        let addr = handle.bind_addr;
        let start = std::time::Instant::now();
        handle.shutdown().await;
        // Graceful shutdown should be well under the 5s hard cap.
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "shutdown took {:?} — should be sub-second on the happy path",
            start.elapsed()
        );
        // After shutdown the listener should refuse new connections.
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            reqwest::Client::new()
                .get(format!("http://{}/healthz", addr))
                .send(),
        )
        .await;
        match result {
            Ok(Err(_)) => {} // connection refused / closed — expected
            Err(_) => {}     // request timed out — also acceptable
            Ok(Ok(_)) => panic!("listener should not accept after shutdown"),
        }
    }
}
