use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use dcc_mcp_host_rpc::HostRpcError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::trace::trace_context_from_headers;
use super::{MCP_PROTOCOL_VERSION, SIDECAR_SERVER_NAME, SidecarMcpState};

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

pub(super) async fn handle_health() -> Response {
    (StatusCode::OK, axum::Json(json!({"ok": true}))).into_response()
}

pub(super) async fn handle_healthz() -> Response {
    (StatusCode::OK, "ok").into_response()
}

pub(super) async fn handle_v1_healthz() -> Response {
    (StatusCode::OK, axum::Json(json!({"ok": true}))).into_response()
}

pub(super) async fn handle_v1_readyz(State(state): State<SidecarMcpState>) -> Response {
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

pub(super) async fn handle_mcp_post(
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

    // Notifications have no id - we accept and discard.
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
