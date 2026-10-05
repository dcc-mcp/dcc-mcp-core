use super::*;

/// Dispatch a gateway `tools/call` to the right local handler.
///
/// Returns `(text_body, is_error)` so the caller can wrap into an MCP
/// `CallToolResult`.
///
/// The advertised gateway MCP surface is intentionally minimal: `tools/list`
/// only exposes `search`, `describe`, `load_skill`, and `call`. This router
/// keeps legacy wrapper names and lease tools callable as hidden compatibility
/// routes, while steering new clients toward the canonical four-tool workflow.
pub async fn route_tools_call(
    gs: &GatewayState,
    tool: &str,
    args: &Value,
    meta: Option<&Value>,
    client_session_id: Option<&str>,
    trace_context: Option<&crate::gateway::admin::trace::TraceContext>,
    agent_context: Option<&crate::gateway::admin::trace::AgentContext>,
) -> (String, bool) {
    // ── Advertised gateway surface (read-only) ───────────────────────
    match tool {
        "search" => {
            return to_text_result(
                tool_search(
                    gs,
                    args,
                    meta,
                    trace_context,
                    client_session_id,
                    agent_context,
                )
                .await,
            );
        }
        "describe" => return to_text_result(tool_describe(gs, args, meta, trace_context).await),
        _ => {}
    }

    // ── Hidden compatibility routes ──────────────────────────────────
    match tool {
        "call" => return tool_call(gs, args, meta, trace_context, agent_context).await,
        "lease" => return to_text_result(tool_lease(gs, args).await),
        "load_skill" => {
            let (text, is_error) =
                crate::gateway::tools::tool_load_skill_for_session(gs, args, client_session_id)
                    .await;
            crate::gateway::tools::record_load_skill_search_followup(
                gs,
                args,
                meta,
                trace_context,
                !is_error,
            );
            return (text, is_error);
        }
        "unload_skill" => {
            return crate::gateway::tools::tool_unload_skill_for_session(
                gs,
                args,
                client_session_id,
            )
            .await;
        }
        // RFC-0007 §3.1 rule 5: let a caller pin (or release) the instance
        // that unqualified calls resolve to, instead of guessing a UUID.
        "bind_instance" => {
            return to_text_result(
                crate::gateway::tools::tool_bind_instance(gs, args, client_session_id).await,
            );
        }
        "unbind_instance" => {
            return to_text_result(
                crate::gateway::tools::tool_unbind_instance(gs, args, client_session_id).await,
            );
        }
        _ => {}
    }

    // ── Legacy aliases (hidden from tools/list; keeps old clients working) ─
    match tool {
        "acquire_dcc_instance" => return to_text_result(tool_acquire_instance(gs, args).await),
        "release_dcc_instance" => return to_text_result(tool_release_instance(gs, args).await),
        "search_tools" => {
            return to_text_result(
                tool_search_tools(gs, args, trace_context, client_session_id, agent_context).await,
            );
        }
        "describe_tool" => {
            return to_text_result(tool_describe_tool(gs, args, meta, trace_context).await);
        }
        "call_tool" => return tool_call_tool(gs, args, meta, trace_context, agent_context).await,
        "call_tools" => {
            return tool_call_tools(gs, args, meta, trace_context, agent_context).await;
        }
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
        "Unknown gateway tool '{tool}'. The advertised gateway MCP surface exposes \
         only four workflow tools: `search` (tools and/or skills), `describe` \
         (tool schema or skill detail), `load_skill` (progressive activation), \
         and `call` (single `tool_slug` or ordered `calls` batch). Put backend \
         parameters inside `arguments` (e.g. export_fbx uses `path`)."
    );
    (hint, true)
}
