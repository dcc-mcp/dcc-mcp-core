//! Session dynamic tools and lazy registry handlers.

use serde_json::{Value, json};

use dcc_mcp_gateway_core::naming::skill_tool_name;
use dcc_mcp_jsonrpc::{CallToolResult, ToolContent};
use dcc_mcp_protocols::error_envelope::DccMcpError;

use crate::dynamic_tools::{
    build_execution_wrapper, handle_deregister_tool, handle_list_dynamic_tools,
    handle_register_tool,
};
use crate::mcp_tool_catalog::{action_meta_to_mcp_tool, resolve_action_by_id};
use crate::server_state::ServerState;

pub(in crate::rmcp_tool_call_dispatch) fn handle_list_actions(
    state: &ServerState,
    arguments: &Value,
) -> CallToolResult {
    let dcc = arguments.get("dcc").and_then(Value::as_str);
    let skill_filter = arguments.get("skill").and_then(Value::as_str);

    let mut items: Vec<Value> = Vec::new();
    for meta in state.registry.list_actions(dcc) {
        if !meta.enabled {
            continue;
        }
        if let Some(want) = skill_filter
            && meta.skill_name.as_deref() != Some(want)
        {
            continue;
        }
        let id = meta
            .skill_name
            .as_deref()
            .and_then(|sn| skill_tool_name(sn, &meta.name))
            .unwrap_or_else(|| meta.name.clone());
        items.push(json!({
            "id": id,
            "summary": meta.description,
            "tags": meta.tags,
        }));
    }

    let payload = json!({
        "total": items.len(),
        "actions": items,
    });
    CallToolResult::text(serde_json::to_string(&payload).unwrap_or_default())
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_describe_action(
    state: &ServerState,
    arguments: &Value,
    _session_id: Option<&str>,
) -> CallToolResult {
    let id = match arguments.get("id").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return CallToolResult::error("Missing required parameter: id"),
    };

    let Some(meta) = resolve_action_by_id(&state.registry, &id) else {
        let envelope = DccMcpError::new(
            "registry",
            "ACTION_NOT_FOUND",
            format!("Unknown action id: {id}"),
        )
        .with_hint("Call list_actions to see available ids.");
        return CallToolResult::error(envelope.to_json().to_string());
    };

    let include_output_schema = true;
    let bare_eligible_for_describe = std::collections::HashSet::new();
    let tool = action_meta_to_mcp_tool(
        &meta,
        include_output_schema,
        &bare_eligible_for_describe,
        state.declared_capabilities.as_ref(),
    );
    let payload = serde_json::to_value(tool).unwrap_or_default();
    CallToolResult::text(serde_json::to_string(&payload).unwrap_or_default())
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_register_tool_dynamic(
    state: &ServerState,
    session_id: Option<&str>,
    arguments: &Value,
) -> CallToolResult {
    let sid = match session_id {
        Some(id) => id,
        None => {
            return CallToolResult::error(
                "register_tool requires an active session (send Mcp-Session-Id header)",
            );
        }
    };

    let result_value = state
        .sessions
        .with_dynamic_tools_mut(sid, |dyn_tools| handle_register_tool(dyn_tools, arguments))
        .unwrap_or_else(|| {
            json!({
                "isError": true,
                "content": [{ "type": "text", "text": "Session not found" }]
            })
        });

    let text = serde_json::to_string(&result_value).unwrap_or_default();
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(result_value.clone()),
        is_error: result_value
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        meta: None,
    }
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_deregister_tool_dynamic(
    state: &ServerState,
    session_id: Option<&str>,
    arguments: &Value,
) -> CallToolResult {
    let sid = match session_id {
        Some(id) => id,
        None => return CallToolResult::error("deregister_tool requires an active session"),
    };

    let result_value = state
        .sessions
        .with_dynamic_tools_mut(sid, |dyn_tools| {
            handle_deregister_tool(dyn_tools, arguments)
        })
        .unwrap_or_else(|| {
            json!({
                "isError": true,
                "content": [{ "type": "text", "text": "Session not found" }]
            })
        });

    let text = serde_json::to_string(&result_value).unwrap_or_default();
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(result_value.clone()),
        is_error: result_value
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        meta: None,
    }
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_list_dynamic_tools_dynamic(
    state: &ServerState,
    session_id: Option<&str>,
) -> CallToolResult {
    let result_value = if let Some(sid) = session_id {
        state
            .sessions
            .with_dynamic_tools_mut(sid, handle_list_dynamic_tools)
            .unwrap_or_else(|| {
                json!({
                    "content": [{ "type": "text", "text": "{\"dynamic_tools\":[], \"count\":0}" }]
                })
            })
    } else {
        handle_list_dynamic_tools(&mut crate::dynamic_tools::SessionDynamicTools::new())
    };

    let text = serde_json::to_string(&result_value).unwrap_or_default();
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(result_value.clone()),
        is_error: result_value
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        meta: None,
    }
}

pub(in crate::rmcp_tool_call_dispatch) fn route_dynamic_execution(
    state: &ServerState,
    session_id: Option<&str>,
    tool_name: &str,
    arguments: Value,
) -> Option<CallToolResult> {
    let sid = session_id?;

    let spec_opt = state.sessions.with_dynamic_tools_mut(sid, |dyn_tools| {
        dyn_tools.get(tool_name).map(|e| e.spec.clone())
    });
    let spec = spec_opt.flatten()?;

    let wrapper_script = build_execution_wrapper(&spec.name, &spec.code, &arguments);

    if state.executor.is_none() {
        let payload = json!({
            "status": "no_executor",
            "message": format!(
                "Dynamic tool '{}' is registered but requires an in-process \
                    DeferredExecutor to execute. Wire DeferredExecutor via \
                    McpHttpConfig::set_executor() in your DCC adapter.",
                spec.name
            ),
            "generated_script": wrapper_script,
            "call_args": arguments
        });
        return Some(CallToolResult::text(
            serde_json::to_string(&payload).unwrap_or_default(),
        ));
    }

    let payload = json!({
        "status": "pending_stage2",
        "message": format!(
            "Dynamic tool '{}' execution queued. Full in-process Python \
                evaluation (Stage 2, issue #462) is not yet implemented. \
                The DCC adapter must provide a PythonEvalHandler.",
            spec.name
        ),
        "generated_script": wrapper_script,
        "call_args": arguments
    });
    Some(CallToolResult::text(
        serde_json::to_string(&payload).unwrap_or_default(),
    ))
}
