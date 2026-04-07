//! Wire types for the MCP Streamable HTTP protocol.
//!
//! Reference: <https://modelcontextprotocol.io/specification/2025-11-25/basic/transports#streamable-http>

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ── Server config ─────────────────────────────────────────────────────────────

/// Configuration for [`McpHttpServer`](crate::McpHttpServer).
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Bind address (default: `"0.0.0.0"`).
    pub host: String,
    /// Port to listen on (default: `8765`).
    pub port: u16,
    /// Server name advertised in the MCP `initialize` response.
    pub server_name: String,
    /// Server version string.
    pub server_version: String,
    /// CORS allowed origins (`*` by default).
    pub cors_allow_origin: String,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 8765,
            server_name: "dcc-mcp".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
            cors_allow_origin: "*".into(),
        }
    }
}

// ── MCP JSON-RPC wire types ───────────────────────────────────────────────────

/// JSON-RPC 2.0 request envelope (subset used by MCP).
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 success response.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub result: Value,
}

impl JsonRpcResponse {
    pub fn ok(id: Option<Value>, result: Value) -> Self {
        Self { jsonrpc: "2.0".into(), id, result }
    }
}

/// JSON-RPC 2.0 error response.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub error: JsonRpcErrorBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcErrorBody {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn method_not_found(id: Option<Value>, method: &str) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            error: JsonRpcErrorBody {
                code: -32601,
                message: format!("Method not found: {method}"),
                data: None,
            },
        }
    }
    pub fn internal(id: Option<Value>, msg: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            error: JsonRpcErrorBody { code: -32603, message: msg.into(), data: None },
        }
    }
}

// ── Public convenience types ──────────────────────────────────────────────────

/// Request body for `tools/call`.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallRequest {
    pub name: String,
    #[serde(default)]
    pub arguments: HashMap<String, Value>,
}

/// Response body for `tools/call`.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallResponse {
    pub content: Vec<ToolContent>,
    #[serde(rename = "isError")]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl ToolCallResponse {
    pub fn success(text: String) -> Self {
        Self {
            content: vec![ToolContent { content_type: "text".into(), text }],
            is_error: false,
        }
    }
    pub fn error(msg: String) -> Self {
        Self {
            content: vec![ToolContent { content_type: "text".into(), text: msg }],
            is_error: true,
        }
    }
}

/// Response body for `tools/list`.
#[derive(Debug, Clone, Serialize)]
pub struct ToolListResponse {
    pub tools: Vec<ToolDescription>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDescription {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}
