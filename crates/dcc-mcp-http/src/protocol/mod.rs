//! MCP JSON-RPC 2.0 protocol types (2025-03-26 Streamable HTTP spec).
//!
//! Reference: <https://modelcontextprotocol.io/specification/2025-03-26/basic/transports>
//!
//! ## Maintainer layout
//!
//! This module is a **thin facade** keeping only protocol-level
//! constants + version negotiation. Every type is split by MCP
//! primitive (lifecycle / tools / resources / prompts) so that
//! downstream readers can jump straight to the file that matches the
//! JSON-RPC method they are inspecting:
//!
//! | File | Contents |
//! |------|----------|
//! | `protocol_jsonrpc.rs`   | `JsonRpcRequest` / `JsonRpcResponse` / `JsonRpcError` / `JsonRpcNotification` / `JsonRpcMessage` / `JsonRpcBatch` + `error_codes` module |
//! | `protocol_lifecycle.rs` | `initialize` / `ServerCapabilities` / `ClientRoot` / `RootsListResult` / `LoggingSetLevelParams` / `ElicitationCreate*` |
//! | `protocol_tools.rs`     | `ListToolsResult` / `McpTool` / `McpToolAnnotations` / `CallTool*` / `ToolContent` |
//! | `protocol_resources.rs` | `McpResource` / `ListResourcesResult` / `ReadResource*` / `ResourceContents` / `SubscribeResourceParams` + `RESOURCE_NOT_ENABLED_ERROR` |
//! | `protocol_prompts.rs`   | `McpPrompt` / `McpPromptArgument` / `ListPromptsResult` / `GetPrompt*` / `McpPromptMessage` / `McpPromptContent` |
//! | `protocol_sse.rs`       | `format_sse_event` + `encode_cursor` / `decode_cursor` pagination helpers |
//! | `notification_builder.rs` | `NotificationBuilder` / `JsonRpcRequestBuilder` — fluent envelope construction (#484) |

mod jsonrpc;
mod lifecycle;
mod notification_builder;
mod prompts;
mod resources;
mod sse;
mod tools;

pub use jsonrpc::{
    JsonRpcBatch, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, error_codes,
};
pub use lifecycle::{
    ClientCapabilities, ClientInfo, ClientRoot, ElicitationCapability, ElicitationCreateParams,
    ElicitationCreateResult, InitializeParams, InitializeResult, LoggingCapability,
    LoggingSetLevelParams, PromptsCapability, ResourcesCapability, RootsListResult,
    ServerCapabilities, ServerInfo, ToolsCapability,
};
pub use notification_builder::{JsonRpcRequestBuilder, NotificationBuilder};
pub use prompts::{
    GetPromptParams, GetPromptResult, ListPromptsResult, McpPrompt, McpPromptArgument,
    McpPromptContent, McpPromptMessage,
};
pub use resources::{
    ListResourcesResult, McpResource, RESOURCE_NOT_ENABLED_ERROR, ReadResourceParams,
    ReadResourceResult, ResourceContents, SubscribeResourceParams,
};
pub use sse::{decode_cursor, encode_cursor, format_sse_event};
pub use tools::{
    CallToolMeta, CallToolMetaDcc, CallToolParams, CallToolResult, ListToolsResult, McpTool,
    McpToolAnnotations, ToolContent,
};

// ── Protocol-version negotiation + session/header/method constants ─────────

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
