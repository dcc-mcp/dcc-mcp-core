//! Standard WebSocket JSON-RPC 2.0 bridge protocol for non-Python DCCs.
//!
//! This module defines the message types used between:
//! - The **bridge server** (dcc-mcp-core / dcc-mcp-server binary)
//! - The **DCC plugin** (UXP plugin, C++ extension, C# script, etc.)
//!
//! ## Protocol overview
//!
//! The DCC plugin acts as a WebSocket **client** (e.g. Photoshop UXP cannot host
//! a WS server). The bridge server acts as a WebSocket **server**.
//!
//! ```text
//! MCP Client (Claude/Cursor)
//!     ↕  HTTP :8765  (MCP Streamable HTTP)
//! dcc-mcp-server  ← this crate / standalone binary
//!     ↕  WebSocket :9001  (this module's protocol)
//! DCC Plugin (any language: JS, C++, C#, GDScript)
//! ```
//!
//! ## Message sequence
//!
//! 1. DCC plugin connects and sends [`BridgeMessage::Hello`].
//! 2. Bridge server acknowledges with [`BridgeMessage::HelloAck`].
//! 3. Bridge server sends [`BridgeMessage::Request`] when an MCP tool is called.
//! 4. DCC plugin replies with [`BridgeMessage::Response`] (success or error).
//! 5. Either side may send [`BridgeMessage::Event`] asynchronously.
//! 6. Either side sends [`BridgeMessage::Disconnect`] to end the session.
//!
//! ## Standard error codes
//!
//! | Code   | Meaning                   |
//! |--------|---------------------------|
//! | -32700 | Parse error               |
//! | -32601 | Method not found          |
//! | -32602 | Invalid params            |
//! | -32603 | Internal error            |
//! | -32001 | No active document        |
//! | -32000 | Generic DCC error         |

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Top-level envelope ──────────────────────────────────────────────────────

/// A single message exchanged on the WebSocket connection.
///
/// All messages are JSON-encoded. Each variant maps to a `"type"` field in the
/// serialised form so that JavaScript (and other dynamic-typing) DCC plugins can
/// switch on a single string without needing a full JSON-RPC parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeMessage {
    /// Sent by the DCC plugin immediately after the WebSocket connection opens.
    Hello(BridgeHello),
    /// Sent by the bridge server in response to a valid [`BridgeMessage::Hello`].
    HelloAck(BridgeHelloAck),
    /// A JSON-RPC 2.0 request sent **from the bridge server to the DCC plugin**.
    ///
    /// The DCC plugin must reply with a [`BridgeMessage::Response`] carrying the
    /// same `id`.
    Request(BridgeRequest),
    /// A JSON-RPC 2.0 response sent **from the DCC plugin to the bridge server**.
    Response(BridgeResponse),
    /// An asynchronous notification (no reply expected) sent by either side.
    Event(BridgeEvent),
    /// Sent by either side to signal a clean close.
    Disconnect(BridgeDisconnect),
    /// Sent by the bridge server when it cannot parse an incoming message.
    ParseError(BridgeParseError),
}

// ── Hello / HelloAck ────────────────────────────────────────────────────────

/// Connection handshake sent by the DCC plugin on connect.
///
/// ```json
/// {"type": "hello", "client": "photoshop-uxp", "version": "0.1.0"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeHello {
    /// DCC plugin identifier, e.g. `"photoshop-uxp"`, `"zbrush-zscript"`.
    pub client: String,
    /// Plugin version string, e.g. `"0.1.0"`.
    pub version: String,
    /// Optional DCC application version the plugin is running in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dcc_version: Option<String>,
    /// Optional additional capabilities / metadata the plugin wants to advertise.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub capabilities: serde_json::Map<String, Value>,
}

/// Server acknowledgement for a [`BridgeHello`].
///
/// ```json
/// {"type": "hello_ack", "server": "dcc-mcp-server", "version": "0.12.18", "session_id": "..."}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeHelloAck {
    /// Bridge server name.
    pub server: String,
    /// Bridge server version.
    pub version: String,
    /// Opaque session identifier assigned by the server.
    pub session_id: String,
}

// ── Request / Response ──────────────────────────────────────────────────────

/// JSON-RPC 2.0 request from the bridge server to the DCC plugin.
///
/// ```json
/// {
///   "type": "request",
///   "jsonrpc": "2.0",
///   "id": 1,
///   "method": "ps.getDocumentInfo",
///   "params": {}
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeRequest {
    /// Always `"2.0"`.
    #[serde(default = "jsonrpc_version")]
    pub jsonrpc: String,
    /// Request identifier — must be echoed in the matching response.
    pub id: RequestId,
    /// Method name (DCC-specific), e.g. `"ps.getDocumentInfo"`.
    pub method: String,
    /// Method parameters (optional; omit or pass `null` for no params).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 response from the DCC plugin to the bridge server.
///
/// Exactly one of `result` or `error` must be present.
///
/// ```json
/// {"type": "response", "jsonrpc": "2.0", "id": 1, "result": {...}}
/// {"type": "response", "jsonrpc": "2.0", "id": 1, "error": {"code": -32603, "message": "..."}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeResponse {
    /// Always `"2.0"`.
    #[serde(default = "jsonrpc_version")]
    pub jsonrpc: String,
    /// Must match the `id` from the originating [`BridgeRequest`].
    pub id: RequestId,
    /// Successful result value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error object (present on failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl BridgeResponse {
    /// Create a successful response.
    pub fn ok(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: jsonrpc_version(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response.
    pub fn err(id: RequestId, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: jsonrpc_version(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// Return `true` if the response carries a result (no error).
    pub fn is_ok(&self) -> bool {
        self.result.is_some() && self.error.is_none()
    }
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    /// Numeric error code (see module-level doc for standard codes).
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional error data (stack trace, context, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Standard JSON-RPC 2.0 and DCC-specific error codes.
pub mod error_codes {
    /// JSON could not be parsed.
    pub const PARSE_ERROR: i32 = -32700;
    /// Method does not exist in the DCC plugin.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid method parameters.
    pub const INVALID_PARAMS: i32 = -32602;
    /// Unspecified internal error.
    pub const INTERNAL_ERROR: i32 = -32603;
    /// No active document is open in the DCC application.
    pub const NO_ACTIVE_DOCUMENT: i32 = -32001;
    /// Generic DCC-side error (inspect `message` / `data` for details).
    pub const DCC_ERROR: i32 = -32000;
}

// ── Event ───────────────────────────────────────────────────────────────────

/// Asynchronous notification — no reply expected.
///
/// Either side can send events; the bridge server will forward DCC-originated
/// events as MCP `tools/call` results where appropriate.
///
/// ```json
/// {"type": "event", "event": "document.changed", "data": {"name": "Untitled-2.psd"}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeEvent {
    /// Event name (DCC-specific), e.g. `"document.changed"`, `"layer.added"`.
    pub event: String,
    /// Optional event payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ── Disconnect / ParseError ─────────────────────────────────────────────────

/// Clean close notification.
///
/// ```json
/// {"type": "disconnect", "reason": "shutdown"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeDisconnect {
    /// Human-readable reason for disconnecting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Sent by the server when it cannot parse an incoming message.
///
/// ```json
/// {"type": "parse_error", "message": "expected JSON object"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeParseError {
    /// Description of the parse failure.
    pub message: String,
}

// ── RequestId ───────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 request identifier — may be a number or a string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// Numeric identifier (most common).
    Number(u64),
    /// String identifier.
    String(String),
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::String(s) => write!(f, "{s}"),
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn jsonrpc_version() -> String {
    "2.0".to_string()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_hello_roundtrip() {
        let msg = BridgeMessage::Hello(BridgeHello {
            client: "photoshop-uxp".to_string(),
            version: "0.1.0".to_string(),
            dcc_version: Some("25.0".to_string()),
            capabilities: serde_json::Map::new(),
        });
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"hello\""));
        let decoded: BridgeMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, BridgeMessage::Hello(_)));
    }

    #[test]
    fn test_request_roundtrip() {
        let msg = BridgeMessage::Request(BridgeRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::Number(42),
            method: "ps.getDocumentInfo".to_string(),
            params: Some(json!({})),
        });
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"request\""));
        assert!(json.contains("\"id\":42"));
    }

    #[test]
    fn test_response_ok() {
        let r = BridgeResponse::ok(RequestId::Number(1), json!({"name": "doc.psd"}));
        assert!(r.is_ok());
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_response_error() {
        let r = BridgeResponse::err(
            RequestId::Number(2),
            error_codes::NO_ACTIVE_DOCUMENT,
            "No active document",
        );
        assert!(!r.is_ok());
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"error\""));
        assert!(json.contains("-32001"));
    }

    #[test]
    fn test_string_request_id() {
        let r = BridgeResponse::ok(RequestId::String("req-abc".to_string()), json!(null));
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"req-abc\""));
    }
}
