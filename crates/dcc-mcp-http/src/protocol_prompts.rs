//! `prompts/list` + `prompts/get` message types (issues #351, #355).

use serde::{Deserialize, Serialize};

/// Single prompt template entry returned by `prompts/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<McpPromptArgument>,
}

/// Declared argument for a prompt (surfaced through `prompts/list`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
}

/// Result payload for `prompts/list`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListPromptsResult {
    pub prompts: Vec<McpPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Request params for `prompts/get`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptParams {
    pub name: String,
    #[serde(default)]
    pub arguments: std::collections::HashMap<String, String>,
}

/// Single message inside a rendered prompt (MCP `PromptMessage`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptMessage {
    pub role: String,
    pub content: McpPromptContent,
}

/// Prompt message content. Only text is emitted by this library —
/// binary content is forwarded as-is on the wire for forward compat.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpPromptContent {
    #[serde(rename = "text")]
    Text { text: String },
}

impl McpPromptContent {
    pub fn text(t: impl Into<String>) -> Self {
        Self::Text { text: t.into() }
    }
}

/// Result payload for `prompts/get`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GetPromptResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<McpPromptMessage>,
}
