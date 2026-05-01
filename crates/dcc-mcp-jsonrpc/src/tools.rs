//! `tools/list` + `tools/call` message types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// MCP `_meta` slot on a `CallToolResult` (issue #342).
    ///
    /// Namespaced under vendor keys (e.g. `dcc.next_tools`); never used
    /// to carry spec-defined top-level fields. Populated lazily by the
    /// handler; older clients ignore it.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Map<String, Value>>,
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
            meta: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: msg.into() }],
            structured_content: None,
            is_error: true,
            meta: None,
        }
    }
}
