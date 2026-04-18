//! MCP JSON-RPC 2.0 protocol types (2025-03-26 Streamable HTTP spec).
//!
//! Reference: https://modelcontextprotocol.io/specification/2025-03-26/basic/transports

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP protocol version this server implements (default / latest).
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// All protocol versions this server can speak, newest first.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];

/// Negotiate the protocol version to use for a session.
///
/// If the client requests a version we support, we use it; otherwise we fall
/// back to our latest supported version (`SUPPORTED_PROTOCOL_VERSIONS[0]`).
pub fn negotiate_protocol_version(client_requested: Option<&str>) -> &'static str {
    if let Some(requested) = client_requested {
        for &v in SUPPORTED_PROTOCOL_VERSIONS {
            if v == requested {
                return v;
            }
        }
    }
    // Client asked for an unknown version (or didn't specify one) — use our latest.
    SUPPORTED_PROTOCOL_VERSIONS[0]
}

/// The `Mcp-Session-Id` HTTP header name.
pub const MCP_SESSION_HEADER: &str = "Mcp-Session-Id";

/// Vendored capability key for delta tools notifications.
pub const DELTA_TOOLS_UPDATE_CAP: &str = "dcc_mcp_core/deltaToolsUpdate";

/// Method name for vendored delta tools update notifications.
pub const DELTA_TOOLS_METHOD: &str = "notifications/tools/delta";

/// Number of tools returned per `tools/list` page.
pub const TOOLS_LIST_PAGE_SIZE: usize = 32;

// ── JSON-RPC envelope ──────────────────────────────────────────────────────

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

// Standard JSON-RPC error codes
pub mod error_codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
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
}

// ── MCP lifecycle messages ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
    /// Vendor-extension capabilities echoed back to the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesCapability {
    pub subscribe: bool,
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PromptsCapability {
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

// ── Tools ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    pub tools: Vec<McpTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<McpToolAnnotations>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpToolAnnotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deferred_hint: Option<bool>,
}

/// `_meta` field carried in a `tools/call` request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CallToolMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_token: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Option<Value>,
    #[serde(rename = "_meta", default)]
    pub meta: Option<CallToolMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image", rename_all = "camelCase")]
    Image { data: String, mime_type: String },
    #[serde(rename = "resource")]
    Resource { resource: Value },
    /// MCP 2025-06-18 `resource_link` content type.
    ///
    /// Used to hand DCC-produced artifact files (playblasts, exports,
    /// screenshots) back to the agent without copying their bytes into the
    /// JSON-RPC response.
    #[serde(rename = "resource_link", rename_all = "camelCase")]
    ResourceLink {
        uri: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

impl CallToolResult {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: false,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: msg.into() }],
            is_error: true,
        }
    }
}

// ── SSE ────────────────────────────────────────────────────────────────────

/// Format a JSON-RPC message as an SSE event string.
pub fn format_sse_event(data: &impl Serialize, event_id: Option<&str>) -> String {
    let json = serde_json::to_string(data).unwrap_or_default();
    if let Some(id) = event_id {
        format!("id: {id}\ndata: {json}\n\n")
    } else {
        format!("data: {json}\n\n")
    }
}

// ── Cursor pagination helpers ─────────────────────────────────────────────

/// Encode a page offset as an opaque cursor string.
pub fn encode_cursor(offset: usize) -> String {
    format!("{offset}")
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Decode a cursor produced by [`encode_cursor`]. Returns `None` if malformed.
pub fn decode_cursor(cursor: &str) -> Option<usize> {
    if cursor.len() % 2 != 0 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..cursor.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&cursor[i..i + 2], 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()?.parse().ok()
}
