//! Source-schema resolution in the same precedence order as stateless dispatch.

use std::collections::HashSet;

use dcc_mcp_jsonrpc::McpTool;

use crate::handlers::build_core_tools;
use crate::mcp_tool_catalog::{SchemaProjection, action_meta_to_mcp_tool, build_lazy_action_tools};
use crate::server_state::ServerState;

use super::helpers::{handle_stub_tool, resolve_action_name};

/// A stateless call has no session dynamic tools. Core/lazy tools and stubs
/// still take precedence over registry exact names and encoded/bare aliases.
pub(crate) fn stateless_wire_tool(
    state: &ServerState,
    name: &str,
) -> Option<(McpTool, SchemaProjection)> {
    if let Some(tool) = build_core_tools().iter().find(|tool| tool.name == name) {
        return Some((tool.clone(), SchemaProjection::Full));
    }
    if state.features.lazy_actions
        && let Some(tool) = build_lazy_action_tools()
            .into_iter()
            .find(|tool| tool.name == name)
    {
        return Some((tool, SchemaProjection::Full));
    }
    if handle_stub_tool(state, name).is_some() {
        return None;
    }
    let resolved = resolve_action_name(state, name);
    let meta = state.registry.get_action(&resolved, None)?;
    if !meta.enabled {
        return None;
    }
    let mut tool = action_meta_to_mcp_tool(
        &meta,
        true,
        &HashSet::new(),
        state.declared_capabilities.as_ref(),
        SchemaProjection::Full,
    );
    // Wire aliases change the exposed name, never the winner's safety,
    // output-schema, description, or vendor-metadata contract.
    tool.name = name.to_string();
    Some((tool, SchemaProjection::ToolsListCompatible))
}

pub(crate) fn warn_invalid_parameter_schema(name: &str, reason: &'static str) {
    let safe_name = if dcc_mcp_naming::validate_tool_name(name).is_ok() {
        name
    } else {
        "<invalid-tool-name>"
    };
    tracing::warn!(
        tool_name = safe_name,
        reason,
        "invalid modern parameter header definition"
    );
}
