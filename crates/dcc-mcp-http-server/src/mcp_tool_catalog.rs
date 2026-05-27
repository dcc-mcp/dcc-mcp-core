//! MCP tool descriptors: registry actions, lazy-action meta-tools, and progressive stubs.

use std::collections::HashSet;

use serde_json::json;

use dcc_mcp_actions::registry::{ToolMeta, ToolRegistry};
use dcc_mcp_gateway_core::naming::{
    decode_skill_tool_name, extract_bare_tool_name, skill_tool_name,
};
use dcc_mcp_jsonrpc::{McpTool, McpToolAnnotations};
use dcc_mcp_models::SkillScope;
use dcc_mcp_skills::SkillSummary;

#[must_use]
pub fn build_lazy_action_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "list_actions".to_string(),
            description: "Returns every enabled action as a compact {id, summary, tags} record with no JSON schemas attached.\n\n\
                          When to use: The entry point of the lazy-actions fast-path (lazy_actions=true). Use it to enumerate candidates cheaply when the full tools/list would blow the token budget.\n\n\
                          How to use:\n\
                          - Filter with dcc and/or skill to narrow the list before fetching schemas.\n\
                          - Follow up with describe_action(id=...) for one action, then call_action(id=..., args=...) to invoke."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dcc": {
                        "type": "string",
                        "description": "DCC filter (e.g. maya, blender)."
                    },
                    "skill": {
                        "type": "string",
                        "description": "Skill-name filter to limit results to one skill."
                    }
                }
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("List Actions".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "describe_action".to_string(),
            description: "Returns the full JSON input schema and metadata for a single action, identical to what tools/list would surface for it.\n\n\
                          When to use: Step 2 of the lazy-actions flow — after list_actions has narrowed the candidate, fetch the schema for exactly one action before calling it.\n\n\
                          How to use:\n\
                          - Pass id exactly as reported by list_actions; unknown ids return an error.\n\
                          - Follow up with call_action(id=..., args=...) using the returned schema."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Action id as reported by list_actions."
                    }
                },
                "required": ["id"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Describe Action".to_string()),
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
        McpTool {
            name: "call_action".to_string(),
            description: "Generic dispatcher that invokes any action by id with the given arguments, using the same code path as a native tools/call.\n\n\
                          When to use: Step 3 of the lazy-actions flow, or whenever you want to avoid inflating tools/list with every action. Semantically identical to calling the action's native tool name directly.\n\n\
                          How to use:\n\
                          - Make sure args matches the schema from describe_action; invalid args are rejected.\n\
                          - Side effects are those of the underlying action — check its ToolAnnotations first."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                            "description": "Action id (e.g. create_sphere or maya_geometry__create_sphere)."
                    },
                    "args": {
                        "type": "object",
                        "description": "Arguments matching the action's input_schema."
                    }
                },
                "required": ["id"]
            }),
            output_schema: None,
            annotations: Some(McpToolAnnotations {
                title: Some("Call Action".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                idempotent_hint: Some(false),
                open_world_hint: Some(true),
                deferred_hint: Some(false),
            }),
            meta: None,
        },
    ]
}

#[must_use]
pub fn action_meta_to_mcp_tool(
    meta: &ToolMeta,
    include_output_schema: bool,
    bare_eligible: &HashSet<(String, String)>,
    declared_capabilities: &[String],
) -> McpTool {
    let schema_is_incomplete = meta.input_schema.is_null();
    let input_schema = if schema_is_incomplete {
        json!({"type": "object"})
    } else {
        meta.input_schema.clone()
    };

    let output_schema = if include_output_schema && !meta.output_schema.is_null() {
        Some(meta.output_schema.clone())
    } else {
        None
    };

    let mcp_name = meta
        .skill_name
        .as_deref()
        .map(|sn| {
            let key = (sn.to_string(), meta.name.clone());
            if bare_eligible.contains(&key) {
                extract_bare_tool_name(sn, &meta.name).to_string()
            } else {
                skill_tool_name(sn, &meta.name).unwrap_or_else(|| meta.name.clone())
            }
        })
        .unwrap_or_else(|| meta.name.clone());

    let declared = &meta.annotations;
    let annotations = if declared.is_spec_empty() {
        None
    } else {
        Some(McpToolAnnotations {
            title: declared.title.clone(),
            read_only_hint: declared.read_only_hint,
            destructive_hint: declared.destructive_hint,
            idempotent_hint: declared.idempotent_hint,
            open_world_hint: declared.open_world_hint,
            deferred_hint: None,
        })
    };

    McpTool {
        name: mcp_name,
        description: meta.description.clone(),
        input_schema,
        output_schema,
        annotations,
        meta: build_tool_meta(meta, declared_capabilities, schema_is_incomplete),
    }
}

#[must_use]
pub fn build_tool_meta(
    meta: &ToolMeta,
    declared_capabilities: &[String],
    schema_is_incomplete: bool,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let deferred = meta
        .annotations
        .deferred_hint
        .unwrap_or_else(|| meta.execution.is_deferred());

    let has_timeout = meta.timeout_hint_secs.is_some();
    let missing = missing_capabilities(&meta.required_capabilities, declared_capabilities);
    let has_required_caps = !meta.required_capabilities.is_empty();
    let has_search_aliases = !meta.search_aliases.is_empty();
    if !has_timeout
        && !deferred
        && !has_required_caps
        && !schema_is_incomplete
        && !has_search_aliases
    {
        return None;
    }

    let mut dcc_meta = serde_json::Map::new();
    if let Some(t) = meta.timeout_hint_secs {
        dcc_meta.insert("timeoutHintSecs".to_string(), serde_json::json!(t));
    }
    if deferred {
        dcc_meta.insert("deferred_hint".to_string(), serde_json::json!(true));
    }
    if has_required_caps {
        dcc_meta.insert(
            "required_capabilities".to_string(),
            serde_json::json!(meta.required_capabilities),
        );
        if !missing.is_empty() {
            dcc_meta.insert(
                "missing_capabilities".to_string(),
                serde_json::json!(missing),
            );
        }
    }
    if has_search_aliases {
        dcc_meta.insert(
            "searchAliases".to_string(),
            serde_json::json!(meta.search_aliases),
        );
    }
    if schema_is_incomplete {
        dcc_meta.insert("incompleteSchema".to_string(), serde_json::json!(true));
        dcc_meta.insert(
            "schemaHint".to_string(),
            serde_json::json!(
                "Tool author did not declare an input schema; arguments are unvalidated. \
                 Inspect the source script or skill docs before calling."
            ),
        );
    }
    let mut out = serde_json::Map::new();
    out.insert("dcc".to_string(), serde_json::Value::Object(dcc_meta));
    Some(out)
}

pub(crate) fn missing_capabilities(required: &[String], declared: &[String]) -> Vec<String> {
    if required.is_empty() {
        return Vec::new();
    }
    let set: HashSet<&str> = declared.iter().map(String::as_str).collect();
    required
        .iter()
        .filter(|c| !c.is_empty() && !set.contains(c.as_str()))
        .cloned()
        .collect()
}

/// Whether *name* is a progressive-loading stub surfaced in ``tools/list``.
#[must_use]
pub fn is_progressive_tool_stub(name: &str) -> bool {
    name.starts_with("__skill__")
        || name.starts_with("__group__")
        || name.contains("__skill__")
        || name.contains("__group__")
}

#[must_use]
pub fn build_skill_stub(summary: &SkillSummary) -> McpTool {
    let has_explicit_hint =
        !summary.search_hint.is_empty() && summary.search_hint != summary.description;

    let description = if has_explicit_hint {
        format!(
            "[{}] {} tools • keywords: {} • Call load_skill(\"{}\")",
            summary.dcc, summary.tool_count, summary.search_hint, summary.name
        )
    } else {
        const PREVIEW_LIMIT: usize = 5;
        let preview = if summary.tool_names.is_empty() {
            String::new()
        } else if summary.tool_names.len() <= PREVIEW_LIMIT {
            format!(" ({})", summary.tool_names.join(", "))
        } else {
            format!(
                " ({}, …+{} more)",
                summary.tool_names[..PREVIEW_LIMIT].join(", "),
                summary.tool_names.len() - PREVIEW_LIMIT
            )
        };

        format!(
            "[{}] {} tools{} • Call load_skill(\"{}\")",
            summary.dcc, summary.tool_count, preview, summary.name
        )
    };

    McpTool {
        name: format!("__skill__{}", summary.name),
        description,
        input_schema: json!({"type": "object", "properties": {}}),
        output_schema: None,
        annotations: None,
        meta: None,
    }
}

pub fn parse_scope_label(s: &str) -> Result<SkillScope, String> {
    match s.to_ascii_lowercase().as_str() {
        "repo" => Ok(SkillScope::Repo),
        "user" => Ok(SkillScope::User),
        "team" => Ok(SkillScope::Team),
        "system" => Ok(SkillScope::System),
        "admin" => Ok(SkillScope::Admin),
        other => Err(format!(
            "Invalid scope {other:?}: expected 'repo' | 'user' | 'team' | 'system' | 'admin'"
        )),
    }
}

/// Look up an action by an id compatible with `list_actions` / bare names.
#[must_use]
pub fn resolve_action_by_id(registry: &ToolRegistry, id: &str) -> Option<ToolMeta> {
    if let Some(m) = registry.get_action(id, None) {
        return Some(m);
    }
    if let Some((skill_part, bare_tool)) = decode_skill_tool_name(id) {
        return registry
            .list_actions_by_skill(skill_part)
            .into_iter()
            .find(|m| extract_bare_tool_name(skill_part, &m.name) == bare_tool);
    }
    None
}

#[must_use]
pub fn build_group_stub(group: &str, tool_names: &[String]) -> McpTool {
    const PREVIEW_LIMIT: usize = 5;
    let preview = if tool_names.len() <= PREVIEW_LIMIT {
        format!(" [{}]", tool_names.join(", "))
    } else {
        format!(
            " [{}, … +{} more]",
            tool_names[..PREVIEW_LIMIT].join(", "),
            tool_names.len() - PREVIEW_LIMIT
        )
    };
    let description = format!(
        "Inactive group '{group}' • {} tools{preview} • Call activate_tool_group(\"{group}\")",
        tool_names.len(),
    );
    McpTool {
        name: format!("__group__{group}"),
        description,
        input_schema: json!({"type": "object", "properties": {}}),
        output_schema: None,
        annotations: None,
        meta: None,
    }
}
