//! MCP JSON-RPC 2.0 protocol types spanning legacy Streamable HTTP revisions
//! and the modern 2026-07-28 discovery lifecycle.
//!
//! Reference: <https://modelcontextprotocol.io/specification/2025-03-26/basic/transports>
//!
//! Extracted from `dcc-mcp-http` so that downstream crates (clients,
//! CLIs, alternative transports) can depend on the wire types without
//! pulling in axum/tokio/reqwest.
//!
//! ## Maintainer layout
//!
//! Every type is split by MCP primitive (lifecycle / tools / resources
//! / prompts) so that downstream readers can jump straight to the file
//! that matches the JSON-RPC method they are inspecting:
//!
//! | File | Contents |
//! |------|----------|
//! | `jsonrpc.rs`              | `JsonRpcRequest` / `JsonRpcResponse` / `JsonRpcError` / `JsonRpcNotification` / `JsonRpcMessage` / `JsonRpcBatch` + `error_codes` module |
//! | `lifecycle.rs`            | `initialize` / `ServerCapabilities` / `ClientRoot` / `RootsListResult` / `LoggingSetLevelParams` / `ElicitationCreate*` |
//! | `tools.rs`                | `ListToolsResult` / `McpTool` / `McpToolAnnotations` / `CallTool*` / `ToolContent` |
//! | `resources.rs`            | `McpResource` / `ListResourcesResult` / `ReadResource*` / `ResourceContents` / `SubscribeResourceParams` + `RESOURCE_NOT_ENABLED_ERROR` |
//! | `prompts.rs`              | `McpPrompt` / `McpPromptArgument` / `ListPromptsResult` / `GetPrompt*` / `McpPromptMessage` / `McpPromptContent` |
//! | `sse.rs`                  | `format_sse_event` + `encode_cursor` / `decode_cursor` pagination helpers |
//! | `notification_builder.rs` | `NotificationBuilder` / `JsonRpcRequestBuilder` — fluent envelope construction (#484) |

mod jsonrpc;
mod lifecycle;
mod notification_builder;
mod prompts;
mod resources;
mod skills;
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
pub use skills::{
    GetSkillParams, GetSkillResult, ListSkillsResult, RESOURCE_DIRECTORY_READ_METHOD,
    ReadResourceDirectoryParams, ReadResourceDirectoryResult, SKILLS_GET_METHOD,
    SKILLS_LIST_METHOD, Skill, SkillResource, SkillResources,
};
pub use sse::{decode_cursor, encode_cursor, format_sse_event};
pub use tools::{
    CallToolMeta, CallToolMetaDcc, CallToolParams, CallToolResult, ListToolsResult, McpTool,
    McpToolAnnotations, ToolContent, coerce_tool_arguments_object,
};

// ── Protocol-version negotiation + session/header/method constants ─────────

/// MCP protocol version this server implements for the modern lifecycle.
pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";

/// Latest protocol revision that still uses the initialize/session lifecycle.
pub const MCP_LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";

/// All protocol versions this server can speak, newest first.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26"];

/// Protocol revisions accepted by the initialize/session lifecycle.
pub const SUPPORTED_LEGACY_PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

/// Negotiate the protocol version to use for a session.
///
/// If the client requests a version we support, we use it; otherwise we fall
/// back to the latest supported legacy version.
pub fn negotiate_protocol_version(client_requested: Option<&str>) -> &'static str {
    if let Some(requested) = client_requested {
        for &v in SUPPORTED_LEGACY_PROTOCOL_VERSIONS {
            if v == requested {
                return v;
            }
        }
    }
    // Initialize is the legacy lifecycle. A modern version is selected through
    // server/discover and per-request metadata, never through initialize.
    MCP_LEGACY_PROTOCOL_VERSION
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
