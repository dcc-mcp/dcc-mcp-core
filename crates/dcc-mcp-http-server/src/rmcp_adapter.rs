//! Type conversion adapters between `dcc-mcp-jsonrpc` wire types and `rmcp` model types.
//!
//! This module provides zero-copy-where-possible conversions so the existing
//! `ToolRegistry` / `ToolDispatcher` / `SkillCatalog` internals remain
//! unchanged while the transport layer speaks rmcp's type language.
//!
//! # Gating
//!
//! This entire module is compiled only when the `rmcp-transport` feature is
//! enabled.

use std::sync::Arc;

use rmcp::model::{
    CallToolResult as RmcpCallToolResult, Content, Meta, RawContent, RawResource, ResourceContents,
    Tool as RmcpTool, ToolAnnotations as RmcpToolAnnotations,
};
use serde_json::Value;

use dcc_mcp_jsonrpc::{
    CallToolResult as DccCallToolResult, McpTool, McpToolAnnotations, ToolContent,
};

// ── McpTool → rmcp::Tool ─────────────────────────────────────────────────────

/// Convert our internal [`McpTool`] to rmcp's [`Tool`](RmcpTool).
pub fn tool_to_rmcp(tool: &McpTool) -> RmcpTool {
    let input_schema: Arc<rmcp::model::JsonObject> = match &tool.input_schema {
        Value::Object(map) => Arc::new(map.clone()),
        _ => Arc::new(
            serde_json::json!({"type": "object", "properties": {}})
                .as_object()
                .unwrap()
                .clone(),
        ),
    };

    let output_schema: Option<Arc<rmcp::model::JsonObject>> =
        tool.output_schema.as_ref().and_then(|v| match v {
            Value::Object(map) => Some(Arc::new(map.clone())),
            _ => None,
        });

    let annotations = tool.annotations.as_ref().map(annotations_to_rmcp);

    let meta = tool.meta.as_ref().map(|m| Meta(m.clone()));

    RmcpTool::new(tool.name.clone(), tool.description.clone(), input_schema)
        .with_raw_output_schema_opt(output_schema)
        .with_annotations_opt(annotations)
        .with_meta_opt(meta)
}

/// Convert our [`McpToolAnnotations`] to rmcp's [`ToolAnnotations`](RmcpToolAnnotations).
fn annotations_to_rmcp(ann: &McpToolAnnotations) -> RmcpToolAnnotations {
    let mut result = RmcpToolAnnotations::new();
    if let Some(title) = &ann.title {
        result = RmcpToolAnnotations::with_title(title.clone());
    }
    if let Some(v) = ann.read_only_hint {
        result = result.read_only(v);
    }
    if let Some(v) = ann.destructive_hint {
        result = result.destructive(v);
    }
    if let Some(v) = ann.idempotent_hint {
        result = result.idempotent(v);
    }
    if let Some(v) = ann.open_world_hint {
        result = result.open_world(v);
    }
    result
}

// ── CallToolResult → rmcp::CallToolResult ────────────────────────────────────

/// Convert our [`CallToolResult`](DccCallToolResult) to rmcp's [`CallToolResult`](RmcpCallToolResult).
pub fn call_result_to_rmcp(result: &DccCallToolResult) -> RmcpCallToolResult {
    let content: Vec<Content> = result.content.iter().map(tool_content_to_rmcp).collect();

    let mut out = RmcpCallToolResult::default();
    out.content = content;
    out.structured_content = result.structured_content.clone();
    out.is_error = if result.is_error { Some(true) } else { None };
    out.meta = result.meta.as_ref().map(|m| Meta(m.clone()));
    out
}

/// Convert a single [`ToolContent`] variant to rmcp's [`Content`].
fn tool_content_to_rmcp(tc: &ToolContent) -> Content {
    match tc {
        ToolContent::Text { text } => Content::text(text.clone()),
        ToolContent::Image { data, mime_type } => Content::image(data.clone(), mime_type.clone()),
        ToolContent::Resource { resource } => {
            // Best-effort: try to interpret as TextResourceContents
            let uri = resource
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown://resource")
                .to_string();
            let text = resource
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mime_type = resource
                .get("mimeType")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Content::resource(ResourceContents::TextResourceContents {
                uri,
                mime_type,
                text,
                meta: None,
            })
        }
        ToolContent::ResourceLink {
            uri,
            name,
            mime_type,
            description,
        } => {
            // Use resource_link content type
            Content::resource_link(RawResource {
                uri: uri.clone(),
                name: name.clone().unwrap_or_default(),
                title: None,
                description: description.clone(),
                mime_type: mime_type.clone(),
                size: None,
                icons: None,
                meta: None,
            })
        }
    }
}

// ── rmcp::CallToolResult → our CallToolResult (reverse) ──────────────────────

/// Convert rmcp's [`CallToolResult`](RmcpCallToolResult) back to our
/// [`CallToolResult`](DccCallToolResult) for proxy/compatibility scenarios.
pub fn rmcp_result_to_dcc(result: &RmcpCallToolResult) -> DccCallToolResult {
    let content: Vec<ToolContent> = result
        .content
        .iter()
        .map(rmcp_content_to_tool_content)
        .collect();

    DccCallToolResult {
        content,
        structured_content: result.structured_content.clone(),
        is_error: result.is_error.unwrap_or(false),
        meta: result.meta.as_ref().map(|m| m.0.clone()),
    }
}

/// Convert rmcp [`Content`] back to our [`ToolContent`].
fn rmcp_content_to_tool_content(content: &Content) -> ToolContent {
    match &content.raw {
        RawContent::Text(t) => ToolContent::Text {
            text: t.text.clone(),
        },
        RawContent::Image(i) => ToolContent::Image {
            data: i.data.clone(),
            mime_type: i.mime_type.clone(),
        },
        RawContent::Resource(r) => {
            let resource = serde_json::to_value(&r.resource).unwrap_or_default();
            ToolContent::Resource { resource }
        }
        RawContent::Audio(a) => {
            // Audio has no direct equivalent in our type; represent as text
            ToolContent::Text {
                text: format!("[audio: {}, {} bytes]", a.mime_type, a.data.len()),
            }
        }
        RawContent::ResourceLink(link) => ToolContent::ResourceLink {
            uri: link.uri.clone(),
            name: Some(link.name.clone()),
            mime_type: link.mime_type.clone(),
            description: link.description.clone(),
        },
    }
}

// ── Helper trait extensions for optional builder patterns ─────────────────────

trait ToolBuilderExt {
    fn with_raw_output_schema_opt(
        self,
        output_schema: Option<Arc<rmcp::model::JsonObject>>,
    ) -> Self;
    fn with_annotations_opt(self, annotations: Option<RmcpToolAnnotations>) -> Self;
    fn with_meta_opt(self, meta: Option<Meta>) -> Self;
}

impl ToolBuilderExt for RmcpTool {
    fn with_raw_output_schema_opt(
        mut self,
        output_schema: Option<Arc<rmcp::model::JsonObject>>,
    ) -> Self {
        if let Some(schema) = output_schema {
            self.output_schema = Some(schema);
        }
        self
    }

    fn with_annotations_opt(mut self, annotations: Option<RmcpToolAnnotations>) -> Self {
        if let Some(ann) = annotations {
            self.annotations = Some(ann);
        }
        self
    }

    fn with_meta_opt(mut self, meta: Option<Meta>) -> Self {
        if let Some(m) = meta {
            self.meta = Some(m);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_conversion_name_description() {
        let tool = McpTool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
            output_schema: None,
            annotations: None,
            meta: None,
        };

        let rmcp_tool = tool_to_rmcp(&tool);
        assert_eq!(rmcp_tool.name.as_ref(), "test_tool");
        assert_eq!(rmcp_tool.description.as_deref(), Some("A test tool"));
    }

    #[test]
    fn test_call_result_text_roundtrip() {
        let result = DccCallToolResult::text("hello world");
        let rmcp_result = call_result_to_rmcp(&result);
        let back = rmcp_result_to_dcc(&rmcp_result);

        assert_eq!(back.content.len(), 1);
        assert!(!back.is_error);
        match &back.content[0] {
            ToolContent::Text { text } => assert_eq!(text, "hello world"),
            _ => panic!("expected Text content"),
        }
    }

    #[test]
    fn test_call_result_error() {
        let result = DccCallToolResult::error("something failed");
        let rmcp_result = call_result_to_rmcp(&result);

        assert_eq!(rmcp_result.is_error, Some(true));
        assert_eq!(rmcp_result.content.len(), 1);
    }
}
