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
    let page_end = (cursor + TOOLS_LIST_PAGE_SIZE).min(total);
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
