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

    /// MCP 2026-07-28 — the client sent an `MCP-Protocol-Version` header
    /// whose value is not in the server's supported versions list.
    ///
    /// The `data` payload MUST include a `supported_versions` array so the
    /// client can retry with the correct version (ADR-010 §版本降级流程):
    ///
    /// ```json
    /// {
    ///   "code": -32004,
    ///   "message": "Unsupported protocol version",
    ///   "data": {
    ///     "requested": "2026-07-28",
    ///     "supported_versions": ["2026-07-28", "2025-06-18", "2025-03-26"]
    ///   }
    /// }
    /// ```
    pub const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32004;

    /// MCP 2026-07-28 — the server received a stateless request that omitted
    /// a required `_meta` field (e.g. `protocolVersion`).
    ///
    /// Only emitted on the `2026-07-28` code path; legacy session requests
    /// use `INVALID_PARAMS` instead.
    pub const VERSION_REQUIRED: i64 = -32005;
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

    pub fn internal_error(id: Option<Value>, msg: impl Into<String>) -> Self {
        Self::error(id, error_codes::INTERNAL_ERROR, msg)
    }

    /// MCP 2026-07-28 — respond with `UNSUPPORTED_PROTOCOL_VERSION` (-32004).
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
                "supported_versions": supported,
            })),
        )
    }
}
