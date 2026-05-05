use super::*;

/// Dispatch a gateway `tools/call` to the right local handler.
///
/// Returns `(text_body, is_error)` so the caller can wrap into an MCP
/// `CallToolResult`.
///
/// The gateway MCP surface is intentionally minimal: only meta-tools,
/// skill-management tools, and the dynamic `search_tools` /
/// `describe_tool` / `call_tool` wrappers are accepted here. Per-action
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
        "list_dcc_instances" => return to_text_result(tool_list_instances(gs, args).await),
        "get_dcc_instance" => return to_text_result(tool_get_instance(gs, args).await),
        "connect_to_dcc" => return to_text_result(tool_connect_to_dcc(gs, args).await),
        "acquire_dcc_instance" => return to_text_result(tool_acquire_instance(gs, args).await),
        "release_dcc_instance" => return to_text_result(tool_release_instance(gs, args).await),
        "diagnostics__process_status" => {
            return to_text_result(tool_diagnostics_process_status(gs, args).await);
        }
        "diagnostics__audit_log" => {
            return to_text_result(tool_diagnostics_audit_log(gs, args).await);
        }
        "diagnostics__tool_metrics" => {
            return to_text_result(tool_diagnostics_tool_metrics(gs, args).await);
        }
        // ── #774 public catalog tools ────────────────────────────────
        "dcc_catalog__search" => return to_text_result(tool_catalog_search(args)),
        "dcc_catalog__describe" => return to_text_result(tool_catalog_describe(args)),
        // ── #655 dynamic-capability MCP wrappers ────────────────────
        "search_tools" => return to_text_result(tool_search_tools(gs, args).await),
        "describe_tool" => return to_text_result(tool_describe_tool(gs, args).await),
        "call_tool" => return tool_call_tool(gs, args, meta).await,
        _ => {}
    }

    // ── Skill-management tools ──────────────────────────────────────────
    if matches!(
        tool,
        "list_skills" | "search_skills" | "get_skill_info" | "load_skill" | "unload_skill"
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
         schema, and `call_tool` to invoke one by slug. For direct HTTP access, \
         each per-DCC server exposes the same tools via `POST /v1/call`."
    );
    (hint, true)
}
