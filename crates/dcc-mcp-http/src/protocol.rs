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

/// MCP method name for per-session logging threshold updates.
pub const LOGGING_SET_LEVEL_METHOD: &str = "logging/setLevel";

/// Method name for server-initiated user elicitation.
pub const ELICITATION_CREATE_METHOD: &str = "elicitation/create";

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

/// A single client-advertised filesystem root (`roots/list`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientRoot {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Result payload for `roots/list`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RootsListResult {
    pub roots: Vec<ClientRoot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
    /// Server supports client-driven log threshold control via
    /// `logging/setLevel` and emits `notifications/message`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingCapability>,

    /// Client-side elicitation support (MCP 2025-06-18).
    ///
    /// The server includes this field only on 2025-06-18 sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapability>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoggingCapability {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSetLevelParams {
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ElicitationCapability {}

/// Request params for `elicitation/create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationCreateParams {
    pub message: String,
    pub requested_schema: Value,
}

/// Result payload returned by client for `elicitation/create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationCreateResult {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    /// JSON Schema describing the tool's structured result (MCP 2025-06-18).
    ///
    /// Serialised as ``outputSchema`` when present. Must be omitted on
    /// 2025-03-26 sessions because the field did not exist in that version
    /// of the spec — a compliant client might treat it as an unknown field
    /// and warn/log. See [`crate::handler`] for the version-gated emitter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<McpToolAnnotations>,
    /// MCP `_meta` — free-form server-scoped metadata (issue #317).
    ///
    /// dcc-mcp-core surfaces implementation-specific hints (e.g.
    /// `dcc.timeoutHintSecs`) here rather than in `annotations`, which is
    /// reserved for spec-defined tool hints.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Map<String, Value>>,
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
    /// dcc-mcp-core specific metadata (issue #318).
    ///
    /// Opt-in async dispatch (`dcc.async = true`) and workflow nesting
    /// (`dcc.parentJobId`). Nested under a single `dcc` key to avoid
    /// polluting the top-level `_meta` namespace which is spec-reserved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dcc: Option<CallToolMetaDcc>,
}

/// dcc-mcp-core specific `_meta.dcc` block on a `tools/call` request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CallToolMetaDcc {
    /// Opt into async job dispatch (#318). When `true`, the server returns
    /// immediately with a `{job_id, status: "pending"}` envelope and runs
    /// the tool on a Tokio task.
    #[serde(default, rename = "async", skip_serializing_if = "std::ops::Not::not")]
    pub r#async: bool,
    /// Parent job id for workflow nesting (#318).
    ///
    /// When set, the dispatched async job's `cancel_token` is a child of the
    /// parent's token — cancelling the parent cancels every descendant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_job_id: Option<String>,
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
    /// Machine-readable payload (MCP 2025-06-18 ``structuredContent``).
    ///
    /// When set, the agent can skip re-parsing ``content[0].text`` as JSON.
    /// Populated by the handler when the dispatch returns a JSON object or
    /// array **and** the session negotiated protocol version 2025-06-18.
    /// Left ``None`` (omitted from the wire) on 2025-03-26 sessions so
    /// older clients never see a field they do not recognise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
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
            structured_content: None,
            is_error: false,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: msg.into() }],
            structured_content: None,
            is_error: true,
        }
    }
}

// ── Resources (MCP 2025-03-26) ────────────────────────────────────────────

/// Single entry returned by `resources/list`.
///
/// Per MCP 2025-03-26, a resource is identified by an opaque URI and
/// carries display metadata. Actual payload is fetched on demand via
/// `resources/read`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Result payload for `resources/list`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListResourcesResult {
    pub resources: Vec<McpResource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Request params for `resources/read`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceParams {
    pub uri: String,
}

/// A single blob returned inside a `ReadResourceResult`.
///
/// Exactly one of `text` or `blob` (base64-encoded bytes) is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContents {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// Result payload for `resources/read`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContents>,
}

/// Request params for `resources/subscribe` / `resources/unsubscribe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeResourceParams {
    pub uri: String,
}

/// Issue #350 — MCP error code for resources that are recognized by
/// URI scheme but whose backing store is not enabled (e.g. `artefact://`
/// before issue #349 wires up the artefact store).
pub const RESOURCE_NOT_ENABLED_ERROR: i64 = -32002;

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
