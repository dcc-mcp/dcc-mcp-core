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
mod modern;
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
pub use modern::{
    CACHEABLE_RESULT_METHODS, CacheScope, CompleteResultType, SERVER_INFO_META_KEY,
    complete_modern_result,
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

/// Default MCP protocol version for the legacy `initialize` lifecycle.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// The MCP 2026-07-28 protocol version string.
pub const MCP_PROTOCOL_VERSION_2026: &str = "2026-07-28";

/// Alias for [`MCP_PROTOCOL_VERSION_2026`] — explicit date-qualified name.
pub const MCP_PROTOCOL_VERSION_2026_07_28: &str = MCP_PROTOCOL_VERSION_2026;

/// All protocol versions this server can speak, newest first.
///
/// This includes both lifecycles. The HTTP protocol header selects the
/// stateless lifecycle; `initialize` negotiates only legacy versions.
#[cfg(feature = "mcp-2026-07-28")]
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2026-07-28", "2025-06-18", "2025-03-26"];

#[cfg(not(feature = "mcp-2026-07-28"))]
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];

/// Legacy (session-based) protocol versions (2025-x and earlier).
///
/// Used by [`select_protocol_mode`] for routing and by
/// [`negotiate_protocol_version`] for `initialize` negotiation.
pub const LEGACY_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];

/// Negotiate the protocol version for the legacy `initialize` lifecycle.
///
/// Supported legacy versions are echoed; missing or unsupported versions
/// fall back to `2025-06-18`. Even when stateless support is compiled in,
/// requesting `2026-07-28` here cannot switch lifecycles: that protocol uses
/// an explicit HTTP header and `server/discover`, not `initialize`.
pub fn negotiate_protocol_version(client_requested: Option<&str>) -> &'static str {
    if let Some(requested) = client_requested {
        for &v in LEGACY_PROTOCOL_VERSIONS {
            if v == requested {
                return v;
            }
        }
    }
    // Keep the negotiated version consistent with the selected lifecycle.
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
    #[cfg(feature = "mcp-2026-07-28")]
    match mcp_protocol_version_header {
        Some(v) if v == MCP_PROTOCOL_VERSION_2026 => ProtocolMode::Stateless,
        Some(v) if LEGACY_PROTOCOL_VERSIONS.contains(&v) => ProtocolMode::Session,
        None if has_session_id => ProtocolMode::Session,
        _ => ProtocolMode::default(),
    }

    #[cfg(not(feature = "mcp-2026-07-28"))]
    {
        let _ = mcp_protocol_version_header;
        let _ = has_session_id;
        ProtocolMode::Session
    }
}

/// Request-level hints used by HTTP routers before JSON-RPC parsing.
///
/// The protocol version is the authoritative routing signal.  `Accept`,
/// `Mcp-Method`, and `Mcp-Name` are intentionally retained as optional hints
/// so gateways can make the same decision without re-parsing request bodies.
/// They are not used to upgrade an unversioned legacy request to stateless
/// mode; doing so would silently change the lifecycle contract for old MCP
/// clients.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProtocolRequestHints<'a> {
    pub protocol_version: Option<&'a str>,
    pub has_session_id: bool,
    pub accept: Option<&'a str>,
    pub method: Option<&'a str>,
    pub name: Option<&'a str>,
}

/// Select the HTTP protocol route from transport headers.
///
/// This is the shared classifier for `/mcp` implementations.  A request is
/// routed statelessly only when the client explicitly advertises
/// `2026-07-28`; the remaining headers are metadata available to middleware.
/// The helper deliberately does not require `Accept`, `Mcp-Method`, or
/// `Mcp-Name`, because discovery and intermediary clients may omit one of
/// those hints while still negotiating the version explicitly.
#[must_use]
pub fn select_protocol_mode_from_headers(hints: ProtocolRequestHints<'_>) -> ProtocolMode {
    select_protocol_mode(hints.protocol_version, hints.has_session_id)
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
    fn initialize_negotiation_does_not_select_the_stateless_lifecycle() {
        assert_eq!(negotiate_protocol_version(Some("2026-07-28")), "2025-06-18");
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

    #[cfg(feature = "mcp-2026-07-28")]
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

    #[cfg(feature = "mcp-2026-07-28")]
    #[test]
    fn header_classifier_uses_explicit_version_over_optional_transport_hints() {
        let hints = ProtocolRequestHints {
            protocol_version: Some(MCP_PROTOCOL_VERSION_2026),
            has_session_id: true,
            accept: Some("application/json, text/event-stream"),
            method: Some("tools/call"),
            name: Some("blender_scene__new_scene"),
        };
        assert_eq!(
            select_protocol_mode_from_headers(hints),
            ProtocolMode::Stateless
        );
    }

    #[test]
    fn header_classifier_keeps_unversioned_requests_on_legacy_route() {
        let hints = ProtocolRequestHints {
            accept: Some("application/json"),
            method: Some("tools/list"),
            ..Default::default()
        };
        assert_eq!(
            select_protocol_mode_from_headers(hints),
            ProtocolMode::Session
        );
    }
}
