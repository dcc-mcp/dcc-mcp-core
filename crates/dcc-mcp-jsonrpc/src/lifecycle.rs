//! MCP lifecycle messages: `initialize`, capabilities, `roots/list`,
//! `logging/setLevel`, `elicitation/create`.

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
