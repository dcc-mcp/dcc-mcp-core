//! Assemble and paginate the MCP `tools/list` surface for rmcp.

use std::collections::{BTreeMap, HashSet};

use dcc_mcp_gateway_core::naming::{BareNameInput, resolve_bare_names};
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
    if state.lazy_actions {
        tools.extend(build_lazy_action_tools());
    }

    let actions = state.registry.list_actions(None);

    let bare_eligible: HashSet<(String, String)> = if state.bare_tool_names {
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

    let mut inactive_groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
                .entry(meta.group.clone())
                .or_default()
                .push(meta.name.clone());
        }
    }

    if !state.exclude_group_stubs_from_tools_list {
        for (group, names) in &inactive_groups {
            tools.push(build_group_stub(group, names));
        }
    }

    if !state.exclude_skill_stubs_from_tools_list {
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
