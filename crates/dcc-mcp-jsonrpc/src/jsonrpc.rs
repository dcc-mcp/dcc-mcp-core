//! JSON-RPC 2.0 envelope + standard error codes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A single JSON-RPC message (request, response, or notification).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

/// A batch of JSON-RPC messages.
pub type JsonRpcBatch = Vec<JsonRpcMessage>;

/// Standard JSON-RPC error codes.
pub mod error_codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;

    /// Issue #354 — the target tool declared capabilities that the hosting
    /// DCC adapter did not advertise at startup. The error `data` payload
    /// includes the `tool`, `required_capabilities`, `declared_capabilities`
    /// and `missing_capabilities` fields so clients can react programmatically.
    pub const CAPABILITY_MISSING: i64 = -32001;

    /// Issue #354 — the client invoked a `workspace://` URI but did not
    /// advertise any MCP `roots` on this session. The error `data` carries
    /// the original path.
    pub const NO_WORKSPACE_ROOTS: i64 = -32602;

    /// Issue #714 — the hosting DCC backend has not finished initialising
    /// yet (dispatcher not wired or DCC host still booting), so a
    /// `tools/call` that would otherwise be queued on the
    /// `DeferredExecutor` / `QueueDispatcher` is refused synchronously.
    ///
    /// The error `data` payload carries the runtime readiness
    /// [`ReadinessReport`](../../dcc_mcp_skill_rest/readiness/struct.ReadinessReport.html)
    /// (`process`, `dcc`, `skill_catalog`, `dispatcher`,
    /// `host_execution_bridge`, `main_thread_executor`) plus the requested
    /// `tool` name so clients can surface context in their back-off messaging.
    pub const BACKEND_NOT_READY: i64 = -32002;

    /// Issue #1009 — gateway `initialize` did not complete within the server-side
    /// deadline (typically because the embedded runtime is starved by a busy DCC
    /// host). Clients should back off and retry with fewer concurrent sessions.
    pub const GATEWAY_BUSY: i64 = -32003;

    // ── MCP 2026-07-28 specific error codes ────────────────────────────────

    /// Final SEP-2243 header absence, malformed encoding, or body disagreement.
    pub const HEADER_MISMATCH: i64 = -32020;
    /// Required client capability was not declared for this request.
    pub const MISSING_REQUIRED_CLIENT_CAPABILITY: i64 = -32021;
    /// Unsupported protocol version; data contains `supported` and `requested`.
    pub const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;
    /// Compatibility name: final missing envelope fields use invalid params.
    #[deprecated(note = "Use INVALID_PARAMS for missing final-revision envelope fields")]
    pub const VERSION_REQUIRED: i64 = INVALID_PARAMS;
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// Like [`Self::error`] but carries a structured `data` payload per
    /// JSON-RPC 2.0 §5.1. Used by issue #354 for `capability_missing` and
    /// `no workspace roots` so clients can machine-read the surrounding
    /// context (missing capabilities, advertised roots, …).
    pub fn error_with_data(
        id: Option<Value>,
        code: i64,
        message: impl Into<String>,
        data: Option<Value>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data,
            }),
        }
    }

    pub fn method_not_found(id: Option<Value>, method: &str) -> Self {
        Self::error(
            id,
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {method}"),
        )
    }

    /// Build the standard JSON-RPC parse-error response.
    pub fn parse_error() -> Self {
        Self::error(None, error_codes::PARSE_ERROR, "Parse error")
    }

    /// Build the standard JSON-RPC invalid-request response.
    pub fn invalid_request() -> Self {
        Self::error(None, error_codes::INVALID_REQUEST, "Invalid Request")
    }

    /// Build an invalid-params response with a human-readable detail.
    pub fn invalid_params(id: Option<Value>, detail: &str) -> Self {
        Self::error(
            id,
            error_codes::INVALID_PARAMS,
            format!("Invalid params: {detail}"),
        )
    }

    pub fn internal_error(id: Option<Value>, msg: impl Into<String>) -> Self {
        Self::error(id, error_codes::INTERNAL_ERROR, msg)
    }

    /// MCP 2026-07-28 — respond with `UNSUPPORTED_PROTOCOL_VERSION` (-32022).
    ///
    /// `requested` is the version the client asked for; `supported` is the
    /// slice of versions the server accepts (passed through as `data`).
    pub fn unsupported_protocol_version(
        id: Option<Value>,
        requested: &str,
        supported: &[&str],
    ) -> Self {
        use serde_json::json;
        Self::error_with_data(
            id,
            error_codes::UNSUPPORTED_PROTOCOL_VERSION,
            "Unsupported protocol version",
            Some(json!({
                "requested": requested,
                "supported": supported,
            })),
        )
    }

    pub fn header_mismatch(id: Option<Value>, header: &str, body: &str) -> Self {
        Self::error_with_data(
            id,
            error_codes::HEADER_MISMATCH,
            "Request headers and body disagree",
            Some(serde_json::json!({"mismatch": {"header": header, "body": body}})),
        )
    }

    pub fn missing_required_client_capability(id: Option<Value>, required: Value) -> Self {
        Self::error_with_data(
            id,
            error_codes::MISSING_REQUIRED_CLIENT_CAPABILITY,
            "Missing required client capability",
            Some(serde_json::json!({"requiredCapabilities": required})),
        )
    }
}

impl JsonRpcNotification {
    /// Build a notification with an explicit params payload.
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params: Some(params),
        }
    }
}
