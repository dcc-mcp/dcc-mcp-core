//! Shared helpers for rmcp `tools/call` dispatch (gates, results, notifications).

use serde_json::{Value, json};

use dcc_mcp_actions::registry::ToolMeta;
use dcc_mcp_gateway_core::naming::{decode_skill_tool_name, extract_bare_tool_name};
use dcc_mcp_jsonrpc::{
    CallToolResult, DELTA_TOOLS_METHOD, NotificationBuilder, ToolContent,
    error_codes::{BACKEND_NOT_READY, CAPABILITY_MISSING},
};
use dcc_mcp_models::NextTools;
use dcc_mcp_protocols::error_envelope::DccMcpError;

use crate::mcp_tool_catalog::missing_capabilities;
use crate::rmcp_registry_context::RegistryContext;
use crate::server_state::ServerState;
use crate::session::SessionManager;

fn notify_tools_list_changed(sessions: &SessionManager, session_id: &str) {
    let event = NotificationBuilder::new("notifications/tools/list_changed")
        .with_empty_params()
        .as_sse_event();
    sessions.push_event(session_id, event);
}

pub(in crate::rmcp_tool_call_dispatch) fn notify_tools_changed(
    sessions: &SessionManager,
    session_id: &str,
    added: &[String],
    removed: &[String],
) {
    if sessions.supports_delta_tools(session_id) {
        let event = NotificationBuilder::new(DELTA_TOOLS_METHOD)
            .with_params(json!({ "added": added, "removed": removed }))
            .as_sse_event();
        sessions.push_event(session_id, event);
    } else {
        notify_tools_list_changed(sessions, session_id);
    }
}

pub(crate) fn attach_next_tools_meta(result: &mut CallToolResult, next_tools: &NextTools) {
    let list = if result.is_error {
        &next_tools.on_failure
    } else {
        &next_tools.on_success
    };
    if list.is_empty() {
        return;
    }
    let key = if result.is_error {
        "on_failure"
    } else {
        "on_success"
    };
    let mut next_tools_meta = serde_json::Map::new();
    next_tools_meta.insert(
        key.to_string(),
        Value::Array(
            list.iter()
                .map(|name| Value::String(name.clone()))
                .collect(),
        ),
    );
    let meta = result.meta.get_or_insert_with(serde_json::Map::new);
    meta.insert("dcc.next_tools".to_string(), Value::Object(next_tools_meta));
}

pub(crate) fn resolve_action_name(state: &ServerState, tool_name: &str) -> String {
    if state.registry.get_action(tool_name, None).is_some() {
        return tool_name.to_string();
    }

    if let Some((skill_part, bare_tool)) = decode_skill_tool_name(tool_name) {
        let matched = state
            .registry
            .list_actions_by_skill(skill_part)
            .into_iter()
            .find(|m| extract_bare_tool_name(skill_part, &m.name) == bare_tool);
        if let Some(m) = matched {
            return m.name;
        }
        return tool_name.to_string();
    }

    let matched = state.registry.list_actions(None).into_iter().find(|m| {
        m.skill_name
            .as_deref()
            .map(|skill_name| extract_bare_tool_name(skill_name, &m.name) == tool_name)
            .unwrap_or(false)
    });
    if let Some(meta) = matched {
        return meta.name;
    }

    tool_name.to_string()
}

pub(crate) fn capability_gate_result(
    state: &ServerState,
    resolved_name: &str,
    action_meta: &ToolMeta,
) -> Option<CallToolResult> {
    let missing = missing_capabilities(
        &action_meta.required_capabilities,
        state.declared_capabilities.as_ref(),
    );
    if missing.is_empty() {
        return None;
    }
    let msg = format!(
        "tool {:?} requires capabilities not advertised by this DCC: {}",
        resolved_name,
        missing.join(", ")
    );
    Some(CallToolResult {
        content: vec![ToolContent::Text { text: msg.clone() }],
        structured_content: Some(json!({
            "code": CAPABILITY_MISSING,
            "message": msg,
            "data": {
                "tool": resolved_name,
                "required_capabilities": action_meta.required_capabilities,
                "declared_capabilities": state.declared_capabilities.as_ref(),
                "missing_capabilities": missing,
            }
        })),
        is_error: true,
        meta: None,
    })
}

pub(crate) fn readiness_gate_result(
    _state: &ServerState,
    ctx: &RegistryContext,
    tool_name: &str,
) -> Option<CallToolResult> {
    let report = ctx.readiness.report();
    if report.is_ready() {
        return None;
    }
    tracing::warn!(
        tool = tool_name,
        readiness.process = report.process,
        readiness.dcc = report.dcc,
        readiness.skill_catalog = report.skill_catalog,
        readiness.dispatcher = report.dispatcher,
        readiness.host_execution_bridge = report.host_execution_bridge,
        readiness.main_thread_executor = report.main_thread_executor,
        "tools/call refused: backend not ready (issue #714)"
    );
    let msg = format!(
        "Backend is not ready yet: {}. \
         Refusing to queue `tools/call` for `{tool_name}` — retry once \
         `/v1/readyz` reports ready.",
        report.status_hint()
    );
    Some(CallToolResult {
        content: vec![ToolContent::Text { text: msg.clone() }],
        structured_content: Some(json!({
            "code": BACKEND_NOT_READY,
            "message": msg,
            "data": {
                "tool": tool_name,
                "readiness": {
                    "process": report.process,
                    "dcc": report.dcc,
                    "skill_catalog": report.skill_catalog,
                    "dispatcher": report.dispatcher,
                    "host_execution_bridge": report.host_execution_bridge,
                    "main_thread_executor": report.main_thread_executor,
                }
            }
        })),
        is_error: true,
        meta: None,
    })
}

pub(crate) fn dispatch_json_result(output: Value) -> CallToolResult {
    let text = serde_json::to_string(&output).unwrap_or_else(|_| output.to_string());
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(output),
        is_error: false,
        meta: None,
    }
}

pub(crate) fn dispatch_err_result(tool_name: &str, msg: impl Into<String>) -> CallToolResult {
    let err_msg = msg.into();
    if err_msg.contains("no handler registered") {
        let envelope = DccMcpError::new(
            "instance",
            "NO_HANDLER",
            format!("Tool '{tool_name}' is registered but has no handler."),
        )
        .with_hint("Register a handler via ToolDispatcher.register_handler().");
        return CallToolResult::error(envelope.to_json());
    }
    CallToolResult::error(err_msg)
}

pub(crate) fn handle_stub_tool(tool_name: &str) -> Option<CallToolResult> {
    if let Some(skill_name) = tool_name.strip_prefix("__skill__") {
        let envelope = DccMcpError::new(
            "gateway",
            "SKILL_NOT_LOADED",
            format!("Skill '{skill_name}' is not loaded."),
        )
        .with_hint(format!(
            "Call load_skill with skill_name=\"{skill_name}\" to register its tools, \
             then call the specific tool you need."
        ));
        return Some(CallToolResult::error(envelope.to_json().to_string()));
    }
    if let Some(group_name) = tool_name.strip_prefix("__group__") {
        let envelope = DccMcpError::new(
            "gateway",
            "GROUP_NOT_ACTIVATED",
            format!("Tool group '{group_name}' is inactive."),
        )
        .with_hint(format!(
            "Call activate_tool_group with group=\"{group_name}\" to enable its tools, \
             then re-list with tools/list."
        ));
        return Some(CallToolResult::error(envelope.to_json().to_string()));
    }
    None
}
