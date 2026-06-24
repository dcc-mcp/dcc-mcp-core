//! MCP lifecycle messages: `initialize`, capabilities, `roots/list`,
//! `logging/setLevel`, `elicitation/create`.
//!
//! Also includes `server/discover` types for MCP 2026-07-28 (ADR-010 Phase 1).

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

// ── MCP 2026-07-28: server/discover (ADR-010 Phase 1) ───────────────────────

/// Tasks capability (MCP 2026-07-28, SEP-2663).
///
/// Tasks become a first-class capability in 2026-07-28, replacing the
/// experimental `tasks` field in `ServerCapabilities`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TasksCapability {}

/// `ServerCapabilities` for MCP 2026-07-28 stateless sessions.
///
/// Differs from the session-based [`ServerCapabilities`] in that:
/// - `tasks` is a first-class field (not experimental)
/// - `elicitation` is absent (handled via `InputRequiredResult` multi-turn)
/// - `logging` is omitted (deprecated in 2026-07-28)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatelessServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
    /// Tasks are a first-class capability in 2026-07-28 (SEP-2663).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<TasksCapability>,
    /// Vendor-extension capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<serde_json::Value>,
}

/// Result payload for `server/discover` (MCP 2026-07-28, SEP-2575).
///
/// Replaces `initialize` in the 2026-07-28 stateless protocol.  Every
/// request is self-contained (no session), so the client calls
/// `server/discover` once (or caches the result) to learn server capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverResult {
    /// The protocol version this server is speaking (`"2026-07-28"`).
    pub protocol_version: String,
    /// Server name and version.
    pub server_info: ServerInfo,
    /// Server capabilities under the 2026-07-28 model.
    pub capabilities: StatelessServerCapabilities,
    /// Optional human-readable workflow hint for AI agents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}
