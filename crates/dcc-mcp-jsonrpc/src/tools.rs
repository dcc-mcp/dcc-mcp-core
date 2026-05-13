//! `tools/list` + `tools/call` message types.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Re-export of [`dcc_mcp_protocols::ToolAnnotations`] under the historical
/// `McpToolAnnotations` name used throughout the wire layer.
///
/// The two types had identical field sets (`title`, four spec hints, plus the
/// dcc-mcp-core-specific `deferred_hint` extension) and identical camelCase
/// serialisation, so they are now a single canonical type. Kept under the
/// `McpToolAnnotations` alias here so existing `use dcc_mcp_jsonrpc::McpToolAnnotations`
/// paths in `dcc-mcp-gateway` / `dcc-mcp-http` continue to compile unchanged.
///
/// Resolves the duplication tracked in #812 part 1.
pub use dcc_mcp_protocols::ToolAnnotations as McpToolAnnotations;

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

fn json_value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Normalise MCP `tools/call` / gateway `call_tool` `arguments` payloads to a JSON **object**.
///
/// Some clients double-serialise `arguments` as a JSON string. Serde accepts
/// that as [`Value::String`], which then breaks backends that expect an object
/// at the outer `arguments` key.
///
/// - `None` / [`Value::Null`] / empty or whitespace-only string → `{}`
/// - [`Value::Object`] → returned unchanged
/// - [`Value::String`] → parsed as JSON; decoded value must be an object
/// - any other top-level kind → [`Err`]
pub fn coerce_tool_arguments_object(arguments: Option<Value>) -> Result<Value, String> {
    match arguments {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::Object(map)) => Ok(Value::Object(map)),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(json!({}));
            }
            let parsed: Value = serde_json::from_str(trimmed).map_err(|e| {
                format!("arguments must be a JSON object; string value is not valid JSON ({e})")
            })?;
            if let Value::Object(_) = parsed {
                Ok(parsed)
            } else {
                Err(format!(
                    "arguments must be a JSON object; decoded string is {} (expected object)",
                    json_value_kind(&parsed)
                ))
            }
        }
        Some(other) => Err(format!(
            "arguments must be a JSON object (got {})",
            json_value_kind(&other)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerce_tool_arguments_object_accepts_object_and_empty() {
        assert_eq!(coerce_tool_arguments_object(None).unwrap(), json!({}));
        assert_eq!(
            coerce_tool_arguments_object(Some(Value::Null)).unwrap(),
            json!({})
        );
        let obj = json!({"code": "pass"});
        assert_eq!(
            coerce_tool_arguments_object(Some(obj.clone())).unwrap(),
            obj
        );
    }

    #[test]
    fn coerce_tool_arguments_object_parses_json_object_string() {
        let s = r#"{"code":"print(1)"}"#.to_string();
        let out = coerce_tool_arguments_object(Some(Value::String(s))).unwrap();
        assert_eq!(out, json!({"code": "print(1)"}));
    }

    #[test]
    fn coerce_tool_arguments_object_rejects_non_object_string() {
        let err = coerce_tool_arguments_object(Some(Value::String("[1]".into()))).unwrap_err();
        assert!(err.contains("array"), "err={err}");
        let err2 = coerce_tool_arguments_object(Some(Value::String("42".into()))).unwrap_err();
        assert!(err2.contains("number"), "err2={err2}");
    }

    #[test]
    fn coerce_tool_arguments_object_rejects_array_at_root() {
        let err = coerce_tool_arguments_object(Some(json!([1, 2]))).unwrap_err();
        assert!(err.contains("array"), "err={err}");
    }

    /// Issue #812 part 1: `McpToolAnnotations` is now a re-export of
    /// `dcc_mcp_protocols::ToolAnnotations`. The wire form must remain the
    /// camelCase shape historically emitted by `tools/list`, with `None`
    /// fields fully omitted (not serialised as `null`).
    #[test]
    fn mcp_tool_annotations_wire_form_is_camelcase_with_skipped_nones() {
        let ann = McpToolAnnotations {
            title: None,
            read_only_hint: Some(true),
            destructive_hint: None,
            idempotent_hint: Some(false),
            open_world_hint: None,
            deferred_hint: Some(true),
        };
        let json = serde_json::to_string(&ann).unwrap();
        // Set fields use spec camelCase keys.
        assert!(json.contains("\"readOnlyHint\":true"), "json: {json}");
        assert!(json.contains("\"idempotentHint\":false"), "json: {json}");
        assert!(json.contains("\"deferredHint\":true"), "json: {json}");
        // Unset fields are fully omitted, not serialised as `null`.
        assert!(!json.contains("title"), "json: {json}");
        assert!(!json.contains("destructiveHint"), "json: {json}");
        assert!(!json.contains("openWorldHint"), "json: {json}");
        assert!(!json.contains("null"), "json: {json}");
    }

    /// Issue #812 part 1: round-trip via `McpTool` (the `tools/list` row
    /// type) keeps the annotations payload byte-stable across the alias
    /// boundary.
    #[test]
    fn mcp_tool_annotations_roundtrip_through_mcp_tool() {
        let tool = McpTool {
            name: "delete_scene".to_string(),
            description: "Delete the active scene".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Delete Scene".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(true),
                idempotent_hint: Some(true),
                open_world_hint: None,
                deferred_hint: Some(false),
            }),
            meta: None,
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: McpTool = serde_json::from_str(&json).unwrap();
        let parsed_ann = parsed.annotations.expect("annotations preserved");
        let original_ann = tool.annotations.unwrap();
        assert_eq!(parsed_ann.title, original_ann.title);
        assert_eq!(parsed_ann.read_only_hint, original_ann.read_only_hint);
        assert_eq!(parsed_ann.destructive_hint, original_ann.destructive_hint);
        assert_eq!(parsed_ann.idempotent_hint, original_ann.idempotent_hint);
        assert_eq!(parsed_ann.open_world_hint, original_ann.open_world_hint);
        assert_eq!(parsed_ann.deferred_hint, original_ann.deferred_hint);
    }
}
