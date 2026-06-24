//! MCP JSON-RPC 2.0 protocol types (2025-03-26 and 2025-06-18 Streamable HTTP
//! spec, and 2026-07-28 stateless spec).
//!
//! References:
//! - 2025-03-26: <https://modelcontextprotocol.io/specification/2025-03-26/basic/transports>
//! - 2025-06-18: <https://modelcontextprotocol.io/specification/2025-06-18/basic/transports>
//! - 2026-07-28: ADR-010 / SEP-2575 (stateless, `server/discover`)
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
//! | `lifecycle.rs`            | `initialize` / `ServerCapabilities` / `ClientRoot` / `RootsListResult` / `LoggingSetLevelParams` / `ElicitationCreate*` (2025-x only) |
//! | `discover.rs`             | `ServerDiscoverResult` / `DiscoverCapabilities` / `TasksCapability` / `StatelessRequestMeta` (2026-07-28) |
//! | `tools.rs`                | `ListToolsResult` / `McpTool` / `McpToolAnnotations` / `CallTool*` / `ToolContent` |
//! | `resources.rs`            | `McpResource` / `ListResourcesResult` / `ReadResource*` / `ResourceContents` / `SubscribeResourceParams` + `RESOURCE_NOT_ENABLED_ERROR` |
//! | `prompts.rs`              | `McpPrompt` / `McpPromptArgument` / `ListPromptsResult` / `GetPrompt*` / `McpPromptMessage` / `McpPromptContent` |
//! | `sse.rs`                  | `format_sse_event` + `encode_cursor` / `decode_cursor` pagination helpers |
//! | `notification_builder.rs` | `NotificationBuilder` / `JsonRpcRequestBuilder` — fluent envelope construction (#484) |

mod discover;
mod jsonrpc;
mod lifecycle;
mod notification_builder;
mod prompts;
mod resources;
mod sse;
mod tools;

pub use discover::{
    Discover2026PromptsCapability, Discover2026ResourcesCapability, Discover2026ToolsCapability,
    DiscoverCapabilities, DiscoverServerInfo, ServerDiscoverResult, StatelessClientInfo,
    StatelessRequestMeta, TasksCapability,
};
pub use jsonrpc::{
    JsonRpcBatch, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, error_codes,
};
pub use lifecycle::{
    ClientCapabilities, ClientInfo, ClientRoot, DiscoverResult, ElicitationCapability,
    ElicitationCreateParams, ElicitationCreateResult, InitializeParams, InitializeResult,
    LoggingCapability, LoggingSetLevelParams, PromptsCapability, ResourcesCapability,
    RootsListResult, ServerCapabilities, ServerInfo, StatelessServerCapabilities, ToolsCapability,
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
///
/// Phase 1 (0.19.0): still `2025-06-18`; will become `2026-07-28` in Phase 2
/// (0.21.0) per ADR-010.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// The MCP 2026-07-28 protocol version string.
pub const MCP_PROTOCOL_VERSION_2026: &str = "2026-07-28";

/// Alias for [`MCP_PROTOCOL_VERSION_2026`] — explicit date-qualified name.
pub const MCP_PROTOCOL_VERSION_2026_07_28: &str = MCP_PROTOCOL_VERSION_2026;

/// All protocol versions this server can speak, newest first.
///
/// `2026-07-28` is listed first so that `negotiate_protocol_version` returns it
/// when a client explicitly requests it, even though `MCP_PROTOCOL_VERSION` is
/// still `2025-06-18` in Phase 1.  The ordering here is used only for fallback
/// when the client requests an *unknown* version — which remains `2025-06-18`
/// until Phase 2.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2026-07-28", "2025-06-18", "2025-03-26"];

/// Legacy (session-based) protocol versions (2025-x and earlier).
///
/// Used by [`select_protocol_mode`] to decide whether to route to the
/// session-based or stateless handler.
pub const LEGACY_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];

/// Negotiate the protocol version to use for a session.
///
/// If the client requests a version we support, we use it; otherwise we fall
/// back to the Phase 1 default (`2025-06-18`), keeping old clients working.
///
/// In Phase 2 this will change to fall back to `2026-07-28`.
pub fn negotiate_protocol_version(client_requested: Option<&str>) -> &'static str {
    if let Some(requested) = client_requested {
        for &v in SUPPORTED_PROTOCOL_VERSIONS {
            if v == requested {
                return v;
            }
        }
    }
    // Client asked for an unknown version (or didn't specify one) — fall back to
    // the Phase 1 default so existing session-based clients are not broken.
    MCP_PROTOCOL_VERSION
}

/// Protocol routing mode derived from request headers (ADR-010).
///
/// The gateway and HTTP server use this to decide which handler path to invoke:
/// the existing session-based path or the new stateless path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolMode {
    /// 2025-x session-based path (`initialize` + `Mcp-Session-Id`).
    Session,
    /// 2026-07-28 stateless path (`server/discover`, no session).
    Stateless,
}

impl Default for ProtocolMode {
    /// Phase 1 default: session mode (old behaviour preserved).
    ///
    /// Will become `Stateless` in Phase 2 (0.21.0) per ADR-010.
    fn default() -> Self {
        ProtocolMode::Session
    }
}

/// Determine the protocol mode to use for an incoming request.
///
/// Decision logic (ADR-010 §協議分流):
///
/// 1. `MCP-Protocol-Version: 2026-07-28`   → `Stateless`
/// 2. `MCP-Protocol-Version: <legacy>`     → `Session`
/// 3. No `MCP-Protocol-Version` + `Mcp-Session-Id` present → `Session`
/// 4. No headers / unknown version         → Phase 1 default (`Session`)
///
/// `mcp_protocol_version_header` – value of the `MCP-Protocol-Version` header,
/// if present.  `has_session_id` – true if the request carries a
/// `Mcp-Session-Id` header.
pub fn select_protocol_mode(
    mcp_protocol_version_header: Option<&str>,
    has_session_id: bool,
) -> ProtocolMode {
    match mcp_protocol_version_header {
        Some(v) if v == MCP_PROTOCOL_VERSION_2026 => ProtocolMode::Stateless,
        Some(v) if LEGACY_PROTOCOL_VERSIONS.contains(&v) => ProtocolMode::Session,
        None if has_session_id => ProtocolMode::Session,
        _ => ProtocolMode::default(),
    }
}

/// The `Mcp-Session-Id` HTTP header name.
pub const MCP_SESSION_HEADER: &str = "Mcp-Session-Id";

/// The `MCP-Protocol-Version` HTTP header name (2026-07-28, SEP-2243).
///
/// Clients that support `2026-07-28` MUST send this header on every request.
/// The server echoes the negotiated version in the response header.
pub const MCP_PROTOCOL_VERSION_HEADER: &str = "MCP-Protocol-Version";

/// The `Mcp-Method` HTTP header name (2026-07-28, SEP-2243).
///
/// Carries the JSON-RPC method name at the transport level so that HTTP
/// middlewares (rate limiters, routers) can inspect it without parsing the body.
pub const MCP_METHOD_HEADER: &str = "Mcp-Method";

/// The `Mcp-Name` HTTP header name (2026-07-28, SEP-2243).
///
/// Carries the tool / resource / prompt name at the transport level.
pub const MCP_NAME_HEADER: &str = "Mcp-Name";

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

// ── MCP 2026-07-28 method name constants ───────────────────────────────────

/// `server/discover` — replaces `initialize` in the 2026-07-28 stateless path.
///
/// Returns [`ServerDiscoverResult`] as the `result` field.
pub const SERVER_DISCOVER_METHOD: &str = "server/discover";

#[cfg(test)]
mod tests {
    use super::*;

    // ── negotiate_protocol_version ──────────────────────────────────────────

    #[test]
    fn negotiate_returns_2026_when_client_requests_it() {
        assert_eq!(negotiate_protocol_version(Some("2026-07-28")), "2026-07-28");
    }

    #[test]
    fn negotiate_returns_2025_06_18_when_client_requests_it() {
        assert_eq!(negotiate_protocol_version(Some("2025-06-18")), "2025-06-18");
    }

    #[test]
    fn negotiate_returns_2025_03_26_when_client_requests_it() {
        assert_eq!(negotiate_protocol_version(Some("2025-03-26")), "2025-03-26");
    }

    #[test]
    fn negotiate_falls_back_to_default_for_unknown_version() {
        // Unknown version → Phase 1 default (2025-06-18, not 2026-07-28).
        let result = negotiate_protocol_version(Some("2024-01-01"));
        assert_eq!(result, MCP_PROTOCOL_VERSION);
    }

    #[test]
    fn negotiate_falls_back_to_default_when_none() {
        let result = negotiate_protocol_version(None);
        assert_eq!(result, MCP_PROTOCOL_VERSION);
    }

    // ── select_protocol_mode ────────────────────────────────────────────────

    #[test]
    fn select_protocol_mode_returns_stateless_for_2026() {
        assert_eq!(
            select_protocol_mode(Some("2026-07-28"), false),
            ProtocolMode::Stateless
        );
    }

    #[test]
    fn select_protocol_mode_returns_session_for_legacy_header() {
        assert_eq!(
            select_protocol_mode(Some("2025-06-18"), false),
            ProtocolMode::Session
        );
        assert_eq!(
            select_protocol_mode(Some("2025-03-26"), false),
            ProtocolMode::Session
        );
    }

    #[test]
    fn select_protocol_mode_returns_session_when_session_id_present_and_no_header() {
        assert_eq!(select_protocol_mode(None, true), ProtocolMode::Session);
    }

    #[test]
    fn select_protocol_mode_returns_default_when_no_hints() {
        // Phase 1 default is Session.
        assert_eq!(select_protocol_mode(None, false), ProtocolMode::default());
        assert_eq!(ProtocolMode::default(), ProtocolMode::Session);
    }

    #[test]
    fn select_protocol_mode_unknown_version_falls_back_to_default() {
        assert_eq!(
            select_protocol_mode(Some("3000-01-01"), false),
            ProtocolMode::default()
        );
    }
}
