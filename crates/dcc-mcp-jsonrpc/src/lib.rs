//! MCP JSON-RPC 2.0 protocol types (2025-03-26 Streamable HTTP spec).
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
mod sse;
mod tools;

pub use jsonrpc::{
    JsonRpcBatch, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, error_codes,
};
pub use lifecycle::{
    ClientCapabilities, ClientInfo, ClientRoot, DiscoverResult, ElicitationCapability,
    ElicitationCreateParams, ElicitationCreateResult, InitializeParams, InitializeResult,
    LoggingCapability, LoggingSetLevelParams, PromptsCapability, ResourcesCapability,
    RootsListResult, ServerCapabilities, ServerInfo, StatelessServerCapabilities, TasksCapability,
    ToolsCapability,
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
    McpToolAnnotations, ToolContent, coerce_tool_arguments_object,
};

// ── Protocol-version negotiation + session/header/method constants ─────────

/// MCP protocol version this server implements (default / latest).
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// MCP 2026-07-28 protocol version constant (ADR-010 Phase 1, SEP-2575).
///
/// This version introduces stateless sessions: no `initialize` handshake,
/// no `Mcp-Session-Id`, every request self-contained via `_meta`.
pub const MCP_PROTOCOL_VERSION_2026_07_28: &str = "2026-07-28";

/// All protocol versions this server can speak, newest first.
///
/// `2026-07-28` is listed when the `mcp-2026-07-28` feature flag is enabled
/// (ADR-010 Phase 1).  At Phase 2 it will become the default; at Phase 3
/// only `2026-07-28` will remain.
#[cfg(feature = "mcp-2026-07-28")]
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2026-07-28", "2025-06-18", "2025-03-26"];

/// All protocol versions this server can speak (feature flag disabled).
#[cfg(not(feature = "mcp-2026-07-28"))]
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
