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
) -> (String, bool) {
    // ── Local meta-tools ────────────────────────────────────────────────
    match tool {
        "acquire_dcc_instance" => return to_text_result(tool_acquire_instance(gs, args).await),
        "release_dcc_instance" => return to_text_result(tool_release_instance(gs, args).await),
        // ── #655 dynamic-capability MCP wrappers ────────────────────
        "search_tools" => return to_text_result(tool_search_tools(gs, args).await),
        "describe_tool" => return to_text_result(tool_describe_tool(gs, args).await),
        "call_tool" => return tool_call_tool(gs, args, meta).await,
        "call_tools" => return tool_call_tools(gs, args, meta).await,
        _ => {}
    }

    // ── Skill-management tools ──────────────────────────────────────────
    if matches!(
        tool,
        "list_skills"
            | "search_skills"
            | "get_skill_info"
            | "load_skill"
            | "unload_skill"
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
        "Unknown gateway tool '{tool}'. The gateway MCP surface is intentionally \
         minimal — it only exposes discovery + dispatch primitives. Use \
         `search_tools` to find backend capabilities, `describe_tool` to get a \
         schema, and `call_tool` (or `call_tools` for ordered batches) to invoke \
         by slug. For direct HTTP access, each per-DCC server exposes `POST /v1/call`; \
         the gateway also exposes `POST /v1/call_batch`."
    );
    (hint, true)
}
