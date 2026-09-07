//! Assemble and paginate the MCP `tools/list` surface for rmcp.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};

use dcc_mcp_gateway_core::capability_naming::{BareNameInput, resolve_bare_names};
use dcc_mcp_jsonrpc::{McpTool, TOOLS_LIST_PAGE_SIZE, decode_cursor, encode_cursor};
use dcc_mcp_naming::validate_tool_name;

use crate::handlers::build_core_tools;
use crate::mcp_tool_catalog::{
    SchemaProjection, action_meta_to_mcp_tool, build_group_stub, build_lazy_action_tools,
    build_skill_stub,
};
use crate::server_state::ServerState;

/// Build the full tool list: core tools, registry actions, stubs, and session dynamic tools.
#[must_use]
pub fn assemble_full_tool_list(
    state: &ServerState,
    include_output_schema: bool,
    session_id: Option<&str>,
) -> Vec<McpTool> {
    let mut tools: Vec<McpTool> = Vec::with_capacity(64);
    tools.extend_from_slice(build_core_tools());
    if state.features.lazy_actions {
        tools.extend(build_lazy_action_tools());
    }

    let actions = state.registry.list_actions(None);

    let bare_eligible: HashSet<(String, String)> = if state.features.bare_tool_names {
        let inputs: Vec<BareNameInput<'_>> = actions
            .iter()
            .filter(|m| m.enabled)
            .filter_map(|m| {
                m.skill_name.as_deref().map(|sn| BareNameInput {
                    skill_name: sn,
                    action_name: m.name.as_str(),
                })
            })
            .collect();
        resolve_bare_names(&inputs)
    } else {
        HashSet::new()
    };

    let mut inactive_groups: BTreeMap<(Option<String>, String), Vec<String>> = BTreeMap::new();
    for meta in &actions {
        if meta.enabled {
            tools.push(action_meta_to_mcp_tool(
                meta,
                include_output_schema,
                &bare_eligible,
                state.declared_capabilities.as_ref(),
                SchemaProjection::ToolsListCompatible,
            ));
        } else if !meta.group.is_empty() {
            inactive_groups
                .entry((meta.skill_name.clone(), meta.group.clone()))
                .or_default()
                .push(meta.name.clone());
        }
    }

    if !state.features.exclude_group_stubs_from_tools_list {
        for ((skill_name, group), names) in &inactive_groups {
            let mut stub = build_group_stub(group, names);
            if let Some(skill_name) = skill_name {
                stub.name = group_stub_name(Some(skill_name), group);
                stub.description = stub
                    .description
                    .replacen(
                        &format!("Inactive group '{group}'"),
                        &format!("Inactive group '{group}' in skill '{skill_name}'"),
                        1,
                    )
                    .replacen(
                        &format!("activate_tool_group(\"{group}\")"),
                        &format!(
                            "activate_tool_group(group_name=\"{group}\", skill_name=\"{skill_name}\")"
                        ),
                        1,
                    );
            }
            tools.push(stub);
        }
    }

    if !state.features.exclude_skill_stubs_from_tools_list {
        let unloaded = state.catalog.list_skills(Some("unloaded"));
        for summary in &unloaded {
            tools.push(build_skill_stub(summary));
        }
    }

    if let Some(sid) = session_id {
        tools.extend(state.sessions.dynamic_tools_for_list(sid));
    }

    tools.retain(tool_name_is_client_safe);
    tools
}

/// Modern annotations are validated against the actual dispatch winner's
/// source schema, never against the lossy legacy compatibility projection.
#[cfg(feature = "mcp-2026-07-28")]
pub(crate) fn assemble_modern_tool_list(state: &ServerState) -> Vec<McpTool> {
    project_modern_tools(state, assemble_full_tool_list(state, true, None))
}

#[cfg(feature = "mcp-2026-07-28")]
fn project_modern_tools(state: &ServerState, mut tools: Vec<McpTool>) -> Vec<McpTool> {
    use crate::mcp_tool_catalog::simplify_mcp_input_schema;
    use crate::rmcp_tool_call_dispatch::{stateless_wire_tool, warn_invalid_parameter_schema};

    let mut names = HashSet::new();
    tools.retain_mut(|tool| {
        if !names.insert(tool.name.clone()) {
            return false;
        }
        let Some((mut winner, projection)) = stateless_wire_tool(state, &tool.name) else {
            return true;
        };
        match dcc_mcp_jsonrpc::scan_mcp_param_headers(&winner.input_schema) {
            Ok(declarations) => {
                if declarations.is_empty() && projection == SchemaProjection::ToolsListCompatible {
                    winner.input_schema = simplify_mcp_input_schema(&winner.input_schema);
                }
                *tool = winner;
                true
            }
            Err(issue) => {
                warn_invalid_parameter_schema(&tool.name, issue.reason);
                false
            }
        }
    });
    tools
}

const GROUP_SKILL_SEPARATOR: &str = "__for_skill__";

pub(crate) fn group_stub_name(skill_name: Option<&str>, group: &str) -> String {
    let name = match skill_name {
        Some(skill_name) => format!("__group__{group}{GROUP_SKILL_SEPARATOR}{skill_name}"),
        None => format!("__group__{group}"),
    };
    if validate_tool_name(&name).is_ok() {
        return name;
    }

    // ponytail: long stubs are error-only; use a stable bounded key instead
    // of adding a runtime lookup map solely to recover their display names.
    let mut hasher = DefaultHasher::new();
    skill_name.hash(&mut hasher);
    group.hash(&mut hasher);
    format!("__group__scoped_{:016x}", hasher.finish())
}

pub(crate) fn parse_group_stub_name(name: &str) -> Option<(Option<&str>, &str)> {
    let value = name.strip_prefix("__group__")?;
    match value.rsplit_once(GROUP_SKILL_SEPARATOR) {
        Some((group, skill_name)) if !group.is_empty() && !skill_name.is_empty() => {
            Some((Some(skill_name), group))
        }
        _ if !value.is_empty() => Some((None, value)),
        _ => None,
    }
}

fn tool_name_is_client_safe(tool: &McpTool) -> bool {
    match validate_tool_name(&tool.name) {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(
                tool_name = %tool.name,
                error = %err,
                "dropping invalid MCP tool name from tools/list"
            );
            false
        }
    }
}

/// Paginate a tool list using MCP cursor tokens.
#[must_use]
pub fn slice_tools_page(
    mut tools: Vec<McpTool>,
    cursor_str: Option<&str>,
) -> (Vec<McpTool>, Option<String>) {
    let total = tools.len();
    let cursor: usize = cursor_str.and_then(decode_cursor).unwrap_or(0);
    let page_end = cursor.saturating_add(TOOLS_LIST_PAGE_SIZE).min(total);
    let page: Vec<McpTool> = if cursor < total {
        tools.drain(cursor..page_end).collect()
    } else {
        Vec::new()
    };
    let next_cursor = if page_end < total {
        Some(encode_cursor(page_end))
    } else {
        None
    };
    (page, next_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "mcp-2026-07-28")]
    #[test]
    fn colliding_alias_retains_actual_winner_safety_and_output_contract_in_either_order() {
        use dcc_mcp_actions::{ToolDispatcher, ToolMeta, ToolRegistry};
        use dcc_mcp_models::SkillToolAnnotations;
        use dcc_mcp_skills::SkillCatalog;
        use serde_json::json;
        use std::sync::Arc;

        let winner = ToolMeta {
            name: "echo".into(),
            description: "actual destructive winner".into(),
            input_schema: json!({"type":"object", "properties": {
                "tenant":{"type":"string", "x-mcp-header":"Tenant"}
            }}),
            output_schema: json!({"type":"object", "required":["actual"]}),
            annotations: SkillToolAnnotations {
                read_only_hint: Some(false),
                destructive_hint: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let alias = ToolMeta {
            name: "fixture_tools__echo".into(),
            skill_name: Some("fixture-tools".into()),
            description: "shadowed read-only alias".into(),
            input_schema: json!({"type":"object"}),
            output_schema: json!({"type":"string"}),
            annotations: SkillToolAnnotations {
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        let registry = Arc::new(ToolRegistry::new());
        registry.register_action(winner.clone());
        registry.register_action(alias.clone());
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            registry.clone(),
            dispatcher.clone(),
        ));
        let state = ServerState::builder(registry, dispatcher, catalog)
            .with_bare_tool_names(true)
            .build();
        let bare = HashSet::from([("fixture-tools".into(), "fixture_tools__echo".into())]);
        let expected = action_meta_to_mcp_tool(&winner, true, &bare, &[], SchemaProjection::Full);
        let loser = action_meta_to_mcp_tool(
            &alias,
            true,
            &bare,
            &[],
            SchemaProjection::ToolsListCompatible,
        );
        assert_eq!(loser.name, expected.name);
        // Explicit orders avoid relying on randomized DashMap iteration.
        for rows in [
            vec![loser.clone(), expected.clone()],
            vec![expected.clone(), loser],
        ] {
            let projected = project_modern_tools(&state, rows);
            assert_eq!(serde_json::to_value(projected).unwrap(), json!([expected]));
        }
    }

    fn pagination_tools() -> Vec<McpTool> {
        (0..TOOLS_LIST_PAGE_SIZE * 2 + 1)
            .map(|index| McpTool {
                name: format!("tool_{index}"),
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn slice_tools_page_out_of_range_cursors_end_pagination() {
        for offset in [
            usize::MAX,
            usize::MAX - TOOLS_LIST_PAGE_SIZE + 1,
            pagination_tools().len(),
        ] {
            let cursor = encode_cursor(offset);
            let (page, next_cursor) = slice_tools_page(pagination_tools(), Some(&cursor));
            assert!(page.is_empty(), "offset: {offset}");
            assert_eq!(next_cursor, None, "offset: {offset}");
        }
    }

    #[test]
    fn slice_tools_page_preserves_page_order_and_completion() {
        let mut cursor = None;
        let mut names = Vec::new();
        for expected_size in [TOOLS_LIST_PAGE_SIZE, TOOLS_LIST_PAGE_SIZE, 1] {
            let (page, next_cursor) = slice_tools_page(pagination_tools(), cursor.as_deref());
            assert_eq!(page.len(), expected_size);
            names.extend(page.into_iter().map(|tool| tool.name));
            cursor = next_cursor;
        }
        assert_eq!(cursor, None);
        assert_eq!(
            names,
            pagination_tools()
                .into_iter()
                .map(|tool| tool.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn slice_tools_page_malformed_cursors_keep_first_page_fallback() {
        for cursor in ["0é0".to_owned(), "30".repeat(4096), "gg".to_owned()] {
            let (page, next_cursor) = slice_tools_page(pagination_tools(), Some(&cursor));
            assert_eq!(page.len(), TOOLS_LIST_PAGE_SIZE);
            assert_eq!(page[0].name, "tool_0");
            assert_eq!(next_cursor, Some(encode_cursor(TOOLS_LIST_PAGE_SIZE)));
        }
    }

    #[test]
    fn scoped_group_stub_names_stay_client_safe() {
        let skill_name = "a".repeat(40);
        let name = group_stub_name(Some(&skill_name), "inspection");

        assert!(validate_tool_name(&name).is_ok(), "{name}");
        assert_eq!(name, group_stub_name(Some(&skill_name), "inspection"));
        assert_eq!(
            parse_group_stub_name("__group__inspection__for_skill__houdini-scene"),
            Some((Some("houdini-scene"), "inspection"))
        );
    }
}
