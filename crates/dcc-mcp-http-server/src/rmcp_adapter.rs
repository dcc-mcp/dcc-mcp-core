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

use rmcp::ErrorData as McpError;
use rmcp::model::{
    CallToolResult as RmcpCallToolResult, ContentBlock, ErrorCode,
    GetPromptResult as RmcpGetPromptResult, GetSkillResult as RmcpGetSkillResult,
    ListResourcesResult as RmcpListResourcesResult, ListSkillsResult as RmcpListSkillsResult,
    MetaObject, Prompt as RmcpPrompt, PromptArgument as RmcpPromptArgument,
    PromptMessage as RmcpPromptMessage, ReadResourceResult as RmcpReadResourceResult,
    Resource as RmcpResource, ResourceContents, Role, Skill as RmcpSkill,
    SkillFrontmatter as RmcpSkillFrontmatter, SkillResource as RmcpSkillResource,
    SkillResources as RmcpSkillResources, Tool as RmcpTool, ToolAnnotations as RmcpToolAnnotations,
};
use serde_json::Value;

use dcc_mcp_jsonrpc::{
    CallToolResult as DccCallToolResult, GetPromptResult as DccGetPromptResult,
    GetSkillResult as DccGetSkillResult, ListSkillsResult as DccListSkillsResult, McpPrompt,
    McpPromptContent, McpResource, McpTool, McpToolAnnotations,
    ReadResourceDirectoryResult as DccReadResourceDirectoryResult,
    ReadResourceResult as DccReadResourceResult, Skill as DccSkill,
    SkillResources as DccSkillResources, ToolContent,
    error_codes::{BACKEND_NOT_READY, CAPABILITY_MISSING},
};

// ── Skills extension results ────────────────────────────────────────────────

/// Convert an internal Skills extension entry to rmcp's validated wire type.
pub fn skill_to_rmcp(skill: &DccSkill) -> Result<RmcpSkill, String> {
    let name = skill
        .frontmatter
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("skill {} is missing frontmatter.name", skill.uri))?;
    let description = skill
        .frontmatter
        .get("description")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("skill {} is missing frontmatter.description", skill.uri))?;

    let mut frontmatter = RmcpSkillFrontmatter::new(name, description);
    for (key, value) in &skill.frontmatter {
        if key != "name" && key != "description" {
            frontmatter = frontmatter.with_field(key.clone(), value.clone());
        }
    }

    let resources = match &skill.resources {
        DccSkillResources::Entries(entries) => RmcpSkillResources::Files(
            entries
                .iter()
                .map(|entry| RmcpSkillResource::new(&entry.uri, &entry.digest, entry.size))
                .collect(),
        ),
        DccSkillResources::Dynamic => RmcpSkillResources::Dynamic,
    };

    Ok(RmcpSkill::new(&skill.uri, frontmatter, resources))
}

/// Convert an internal `skills/list` result to rmcp's result type.
pub fn list_skills_result_to_rmcp(
    result: &DccListSkillsResult,
) -> Result<RmcpListSkillsResult, String> {
    let skills = result
        .skills
        .iter()
        .map(skill_to_rmcp)
        .collect::<Result<Vec<_>, _>>()?;
    let mut converted = RmcpListSkillsResult::with_all_items(skills);
    converted.next_cursor = result.next_cursor.clone();
    Ok(converted)
}

/// Convert an internal `skills/get` result to rmcp's result type.
pub fn get_skill_result_to_rmcp(result: &DccGetSkillResult) -> Result<RmcpGetSkillResult, String> {
    let skill = result
        .skill
        .as_ref()
        .ok_or_else(|| "skills/get returned no skill".to_string())?;
    Ok(RmcpGetSkillResult::new(skill_to_rmcp(skill)?))
}

/// Convert an internal directory listing to rmcp's resource-list result type.
pub fn read_resource_directory_result_to_rmcp(
    result: &DccReadResourceDirectoryResult,
) -> RmcpListResourcesResult {
    let resources = result.resources.iter().map(resource_to_rmcp).collect();
    let mut converted = RmcpListResourcesResult::with_all_items(resources);
    converted.next_cursor = result.next_cursor.clone();
    converted
}

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

    let meta = tool.meta.as_ref().map(|m| MetaObject(m.clone()));

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
    let content: Vec<ContentBlock> = result.content.iter().map(tool_content_to_rmcp).collect();

    let mut out = RmcpCallToolResult::default();
    out.content = content;
    out.structured_content = result.structured_content.clone();
    // Always set is_error so rmcp serialization includes isError=false on success
    // (clients that use result["isError"] otherwise get KeyError).
    out.is_error = Some(result.is_error);
    out.meta = result.meta.as_ref().map(|m| MetaObject(m.clone()));
    out
}

/// Map protocol-level tool errors (capability gate, readiness) to JSON-RPC errors.
#[must_use]
pub fn protocol_error_from_call_result(result: &DccCallToolResult) -> Option<McpError> {
    if !result.is_error {
        return None;
    }
    let sc = result.structured_content.as_ref()?;
    let code = sc.get("code")?.as_i64()?;
    let message = sc
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let data = sc.get("data").cloned();
    match code {
        CAPABILITY_MISSING | BACKEND_NOT_READY => {
            Some(McpError::new(ErrorCode(code as i32), message, data))
        }
        _ => None,
    }
}

/// Convert a single [`ToolContent`] variant to rmcp's [`ContentBlock`].
fn tool_content_to_rmcp(tc: &ToolContent) -> ContentBlock {
    match tc {
        ToolContent::Text { text } => ContentBlock::text(text.clone()),
        ToolContent::Image { data, mime_type } => {
            ContentBlock::image(data.clone(), mime_type.clone())
        }
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
            ContentBlock::resource(ResourceContents::TextResourceContents {
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
            let mut resource = RmcpResource::new(uri.clone(), name.clone().unwrap_or_default());
            if let Some(description) = description {
                resource = resource.with_description(description.clone());
            }
            if let Some(mime_type) = mime_type {
                resource = resource.with_mime_type(mime_type.clone());
            }
            ContentBlock::resource_link(resource)
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

/// Convert rmcp [`ContentBlock`] back to our [`ToolContent`].
fn rmcp_content_to_tool_content(content: &ContentBlock) -> ToolContent {
    match content {
        ContentBlock::Text(t) => ToolContent::Text {
            text: t.text.clone(),
        },
        ContentBlock::Image(i) => ToolContent::Image {
            data: i.data.clone(),
            mime_type: i.mime_type.clone(),
        },
        ContentBlock::Resource(r) => {
            let resource = serde_json::to_value(&r.resource).unwrap_or_default();
            ToolContent::Resource { resource }
        }
        ContentBlock::Audio(a) => {
            // Audio has no direct equivalent in our type; represent as text
            ToolContent::Text {
                text: format!("[audio: {}, {} bytes]", a.mime_type, a.data.len()),
            }
        }
        ContentBlock::ResourceLink(link) => ToolContent::ResourceLink {
            uri: link.uri.clone(),
            name: Some(link.name.clone()),
            mime_type: link.mime_type.clone(),
            description: link.description.clone(),
        },
        _ => ToolContent::Text {
            text: "[unsupported MCP content block]".to_string(),
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
    fn with_meta_opt(self, meta: Option<MetaObject>) -> Self;
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

    fn with_meta_opt(mut self, meta: Option<MetaObject>) -> Self {
        if let Some(m) = meta {
            self.meta = Some(m);
        }
        self
    }
}

// ── McpResource → rmcp::Resource ────────────────────────────────────────────

/// Convert our internal [`McpResource`] to rmcp's [`Resource`](RmcpResource).
#[must_use]
pub fn resource_to_rmcp(r: &McpResource) -> RmcpResource {
    let mut resource = RmcpResource::new(&r.uri, &r.name);
    if let Some(d) = &r.description {
        resource = resource.with_description(d.clone());
    }
    if let Some(m) = &r.mime_type {
        resource = resource.with_mime_type(m.clone());
    }
    resource
}

// ── ReadResourceResult → rmcp::ReadResourceResult ───────────────────────────

/// Convert our [`ReadResourceResult`](DccReadResourceResult) to rmcp's
/// [`ReadResourceResult`](RmcpReadResourceResult).
#[must_use]
pub fn read_result_to_rmcp(r: &DccReadResourceResult) -> RmcpReadResourceResult {
    let contents: Vec<ResourceContents> = r
        .contents
        .iter()
        .map(|c| {
            if let Some(text) = &c.text {
                let rc = ResourceContents::text(text.clone(), &c.uri);
                if let Some(mime) = &c.mime_type {
                    rc.with_mime_type(mime.clone())
                } else {
                    rc
                }
            } else if let Some(blob) = &c.blob {
                let rc = ResourceContents::blob(blob.clone(), &c.uri);
                if let Some(mime) = &c.mime_type {
                    rc.with_mime_type(mime.clone())
                } else {
                    rc
                }
            } else {
                ResourceContents::text("", &c.uri)
            }
        })
        .collect();
    RmcpReadResourceResult::new(contents)
}

// ── McpPrompt → rmcp::Prompt ────────────────────────────────────────────────

/// Convert our internal [`McpPrompt`] to rmcp's [`Prompt`](RmcpPrompt).
#[must_use]
pub fn prompt_to_rmcp(p: &McpPrompt) -> RmcpPrompt {
    let arguments: Option<Vec<RmcpPromptArgument>> = if p.arguments.is_empty() {
        None
    } else {
        Some(
            p.arguments
                .iter()
                .map(|a| {
                    let mut arg = RmcpPromptArgument::new(&a.name);
                    if let Some(desc) = &a.description {
                        arg = arg.with_description(desc.clone());
                    }
                    if a.required {
                        arg = arg.with_required(true);
                    }
                    arg
                })
                .collect(),
        )
    };
    let mut prompt = RmcpPrompt::new(&p.name, p.description.as_deref(), arguments);
    if let Some(Value::Object(obj)) = &p.meta {
        prompt.meta = Some(MetaObject(obj.clone()));
    }
    prompt
}

// ── GetPromptResult → rmcp::GetPromptResult ─────────────────────────────────

/// Convert our [`GetPromptResult`](DccGetPromptResult) to rmcp's
/// [`GetPromptResult`](RmcpGetPromptResult).
#[must_use]
pub fn get_prompt_result_to_rmcp(r: &DccGetPromptResult) -> RmcpGetPromptResult {
    let messages: Vec<RmcpPromptMessage> = r
        .messages
        .iter()
        .map(|m| {
            let role = match m.role.as_str() {
                "assistant" => Role::Assistant,
                _ => Role::User,
            };
            let content = match &m.content {
                McpPromptContent::Text { text } => ContentBlock::text(text.clone()),
            };
            RmcpPromptMessage::new(role, content)
        })
        .collect();
    let mut result = RmcpGetPromptResult::new(messages);
    result.description = r.description.clone();
    result
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

    #[test]
    fn test_resource_conversion() {
        let resource = McpResource {
            uri: "scene://current".to_string(),
            name: "Current Scene".to_string(),
            description: Some("Current DCC scene state".to_string()),
            mime_type: Some("application/json".to_string()),
        };

        let rmcp = resource_to_rmcp(&resource);
        assert_eq!(rmcp.uri, "scene://current");
        assert_eq!(rmcp.name, "Current Scene");
        assert_eq!(rmcp.description.as_deref(), Some("Current DCC scene state"));
        assert_eq!(rmcp.mime_type.as_deref(), Some("application/json"));
        assert!(rmcp.annotations.is_none());
    }

    #[test]
    fn test_prompt_conversion() {
        use dcc_mcp_jsonrpc::McpPromptArgument;

        let prompt = McpPrompt {
            name: "bake_animation".to_string(),
            description: Some("Bake and export animation".to_string()),
            arguments: vec![
                McpPromptArgument {
                    name: "frame_range".to_string(),
                    description: Some("Frame range to bake".to_string()),
                    required: true,
                },
                McpPromptArgument {
                    name: "format".to_string(),
                    description: None,
                    required: false,
                },
            ],
            meta: Some(serde_json::json!({
                "dcc.prompt_source": {
                    "skill": "maya-prompts-demo",
                    "source": "explicit"
                }
            })),
        };

        let rmcp = prompt_to_rmcp(&prompt);
        assert_eq!(rmcp.name, "bake_animation");
        assert_eq!(
            rmcp.description.as_deref(),
            Some("Bake and export animation")
        );
        let args = rmcp.arguments.unwrap();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].name, "frame_range");
        assert_eq!(args[0].required, Some(true));
        assert_eq!(args[1].name, "format");
        assert_eq!(args[1].required, None); // false → not set
        assert_eq!(
            rmcp.meta
                .and_then(|m| m.0.get("dcc.prompt_source").cloned())
                .and_then(|v| v.get("source").cloned()),
            Some(Value::String("explicit".to_string()))
        );
    }

    #[test]
    fn test_get_prompt_result_conversion() {
        use dcc_mcp_jsonrpc::{McpPromptContent, McpPromptMessage};

        let result = DccGetPromptResult {
            description: Some("Rendered prompt".to_string()),
            messages: vec![McpPromptMessage {
                role: "user".to_string(),
                content: McpPromptContent::text("Please bake frames 1-100"),
            }],
        };

        let rmcp = get_prompt_result_to_rmcp(&result);
        assert_eq!(rmcp.description.as_deref(), Some("Rendered prompt"));
        assert_eq!(rmcp.messages.len(), 1);
        assert_eq!(rmcp.messages[0].role, Role::User);
    }

    #[test]
    fn test_read_result_text_conversion() {
        use dcc_mcp_jsonrpc::ResourceContents as DccResourceContents;

        let result = DccReadResourceResult {
            contents: vec![DccResourceContents {
                uri: "scene://current".to_string(),
                mime_type: Some("application/json".to_string()),
                text: Some(r#"{"objects": 42}"#.to_string()),
                blob: None,
            }],
        };

        let rmcp = read_result_to_rmcp(&result);
        assert_eq!(rmcp.contents.len(), 1);
        match &rmcp.contents[0] {
            ResourceContents::TextResourceContents {
                uri,
                mime_type,
                text,
                ..
            } => {
                assert_eq!(uri, "scene://current");
                assert_eq!(mime_type.as_deref(), Some("application/json"));
                assert_eq!(text, r#"{"objects": 42}"#);
            }
            _ => panic!("expected TextResourceContents"),
        }
    }
}
