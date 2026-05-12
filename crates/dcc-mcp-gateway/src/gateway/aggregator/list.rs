use super::*;

/// Build the unified `tools/list` result — gateway meta-tools + skill-management tools only.
///
/// The gateway MCP surface is intentionally **minimal + static**: `tools/list`
/// returns only the discover+dispatch primitives (the "few basic MCP tools"
/// listed in [`GATEWAY_LOCAL_TOOLS`]). Per-action backend tools are never
/// fanned out here — agents discover them dynamically through `search_tools`
/// / `describe_tool` and invoke them through `call_tool` or `call_tools` (which
/// route to the per-DCC REST `POST /v1/call`). This keeps the agent-facing context
/// bounded regardless of how many DCC instances are registered.
///
/// Tool order:
/// 1. Gateway discovery / pooling meta-tools (`list_dcc_instances`,
///    `get_dcc_instance`, `connect_to_dcc`, `acquire_dcc_instance`,
///    `release_dcc_instance`).
/// 2. Skill-management + dispatch tools (one canonical set for the whole
///    gateway).
///
/// Pagination uses the same cursor scheme as the per-DCC server:
/// `cursor` is an opaque hex-encoded offset into the flat tool list.
///
/// [`GATEWAY_LOCAL_TOOLS`]: super::super::namespace::GATEWAY_LOCAL_TOOLS
pub async fn aggregate_tools_list(gs: &GatewayState, cursor: Option<&str>) -> Value {
    // Touch `gs` so clippy stops warning about the unused argument; future
    // expansions (auth-aware filtering etc.) will consume it again.
    let _ = gs;

    let mut tools: Vec<Value> = Vec::new();

    // Tier 1 + 2: local gateway tools (meta + skill management + dynamic
    // discovery/dispatch wrappers). This is the entire gateway MCP surface.
    if let Value::Array(local) = gateway_tool_defs() {
        tools.extend(local);
    }
    tools.extend(skill_management_tool_defs());

    // ── Pagination ───────────────────────────────────────────────────────
    let offset = cursor.and_then(decode_cursor).unwrap_or(0);
    let total = tools.len();
    let page_end = (offset + TOOLS_LIST_PAGE_SIZE).min(total);
    let page: Vec<Value> = if offset < total {
        tools.drain(offset..page_end).collect()
    } else {
        Vec::new()
    };

    let mut result = json!({"tools": page});
    if page_end < total {
        result["nextCursor"] = json!(encode_cursor(page_end));
    }
    result
}
