use super::*;

/// Dispatch a gateway `tools/call` to the right local handler.
///
/// Returns `(text_body, is_error)` so the caller can wrap into an MCP
/// `CallToolResult`.
///
/// The gateway MCP surface is intentionally minimal: only meta-tools,
/// skill-management tools, and the dynamic `search_tools` /
/// `describe_tool` / `call_tool` / `call_tools` wrappers are accepted here. Per-action
/// backend tools are not published in `tools/list`, so any other name is
/// rejected with an error that points the caller at the canonical
/// discovery path.
pub async fn route_tools_call(
    gs: &GatewayState,
    tool: &str,
    args: &Value,
    meta: Option<&Value>,
    _request_id: Option<String>,
    _client_session_id: Option<&str>,
    trace_context: Option<&crate::gateway::admin::trace::TraceContext>,
) -> (String, bool) {
    // ── Consolidated gateway surface (6 tools) ───────────────────────
    match tool {
        "lease" => return to_text_result(tool_lease(gs, args).await),
        "search" => return to_text_result(tool_search(gs, args).await),
        "describe" => return to_text_result(tool_describe(gs, args).await),
        "call" => return tool_call(gs, args, meta, trace_context).await,
        "load_skill" => return tool_load_skill(gs, args).await,
        "unload_skill" => return skill_mgmt_dispatch(gs, "unload_skill", args).await,
        _ => {}
    }

    // ── Legacy aliases (hidden from tools/list; keeps old clients working) ─
    match tool {
        "acquire_dcc_instance" => return to_text_result(tool_acquire_instance(gs, args).await),
        "release_dcc_instance" => return to_text_result(tool_release_instance(gs, args).await),
        "search_tools" => return to_text_result(tool_search_tools(gs, args).await),
        "describe_tool" => return to_text_result(tool_describe_tool(gs, args).await),
        "call_tool" => return tool_call_tool(gs, args, meta, trace_context).await,
        "call_tools" => return tool_call_tools(gs, args, meta, trace_context).await,
        _ => {}
    }

    if matches!(
        tool,
        "list_skills"
            | "search_skills"
            | "get_skill_info"
            | "activate_tool_group"
            | "deactivate_tool_group"
    ) {
        return skill_mgmt_dispatch(gs, tool, args).await;
    }

    // ── Unknown tool ────────────────────────────────────────────────────
    // The gateway MCP surface does not publish per-action backend tools.
    // Any name we did not match above is not a valid gateway entry point;
    // redirect the caller to the dynamic-capability wrappers that own
    // that namespace.
    let hint = format!(
        "Unknown gateway tool '{tool}'. The gateway MCP surface exposes six tools: \
         `search` (tools and/or skills), `describe` (tool schema or skill detail), \
         `call` (single slug or ordered `calls` batch), `load_skill` / `unload_skill`, \
         and `lease` (acquire/release instance pooling). Use `search` → `describe` → \
         `call`; put backend parameters inside `call.arguments` (e.g. export_fbx uses `path`)."
    );
    (hint, true)
}
