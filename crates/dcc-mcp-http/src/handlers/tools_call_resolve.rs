use super::*;

use dcc_mcp_actions::ActionMeta;

pub(super) struct ResolvedToolCall {
    pub params: CallToolParams,
    pub tool_name: String,
    pub resolved_name: String,
    pub call_params: Value,
    pub action_meta: ActionMeta,
}

pub(super) enum ToolCallResolution {
    Response(JsonRpcResponse),
    Dispatch(Box<ResolvedToolCall>),
}

pub(super) async fn resolve_tool_call(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<ToolCallResolution, HttpError> {
    let params: CallToolParams = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .ok_or_else(|| HttpError::Internal("invalid tools/call params".to_string()))?;

    let tool_name = params.name.clone();

    if let Some(response) = route_core_tool(state, req, session_id, &params, &tool_name).await? {
        return Ok(ToolCallResolution::Response(response));
    }

    if let Some(response) = handle_stub_tool(req, &tool_name)? {
        return Ok(ToolCallResolution::Response(response));
    }

    let call_params = params.arguments.clone().unwrap_or(json!({}));
    let resolved_name = resolve_action_name(state, &tool_name);
    let action_meta = match state.registry.get_action(&resolved_name, None) {
        Some(meta) => meta,
        None => {
            let envelope = DccMcpError::new(
                "registry",
                "ACTION_NOT_FOUND",
                format!("Unknown tool: {tool_name}"),
            )
            .with_hint(
                "Use tools/list to see available tools, or load a skill first with load_skill."
                    .to_string(),
            );
            return Ok(ToolCallResolution::Response(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
            )));
        }
    };

    if let Some(response) = capability_gate(state, req, &resolved_name, &action_meta) {
        return Ok(ToolCallResolution::Response(response));
    }

    Ok(ToolCallResolution::Dispatch(Box::new(ResolvedToolCall {
        params,
        tool_name,
        resolved_name,
        call_params,
        action_meta,
    })))
}

async fn route_core_tool(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
    params: &CallToolParams,
    tool_name: &str,
) -> Result<Option<JsonRpcResponse>, HttpError> {
    let response = match tool_name {
        "list_roots" => Some(handle_list_roots(state, req, session_id).await?),
        "find_skills" => Some(handle_find_skills(state, req, params).await?),
        "list_skills" => Some(handle_list_skills(state, req, params).await?),
        "get_skill_info" => Some(handle_get_skill_info(state, req, params).await?),
        "load_skill" => Some(handle_load_skill(state, req, params, session_id).await?),
        "unload_skill" => Some(handle_unload_skill(state, req, params, session_id).await?),
        "search_skills" => Some(handle_search_skills(state, req, params).await?),
        "activate_tool_group" => {
            Some(handle_activate_tool_group(state, req, params, session_id).await?)
        }
        "deactivate_tool_group" => {
            Some(handle_deactivate_tool_group(state, req, params, session_id).await?)
        }
        "search_tools" => Some(handle_search_tools(state, req, params).await?),
        "jobs.get_status" => Some(handle_jobs_get_status(state, req, params).await?),
        "jobs.cleanup" => Some(handle_jobs_cleanup(state, req, params).await?),
        "list_actions" if state.lazy_actions => {
            Some(handle_list_actions(state, req, params).await?)
        }
        "describe_action" if state.lazy_actions => {
            Some(handle_describe_action(state, req, params, session_id).await?)
        }
        "call_action" if state.lazy_actions => {
            Some(handle_call_action(state, req, params, session_id).await?)
        }
        _ => None,
    };
    Ok(response)
}

fn handle_stub_tool(
    req: &JsonRpcRequest,
    tool_name: &str,
) -> Result<Option<JsonRpcResponse>, HttpError> {
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
        return Ok(Some(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        )));
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
        return Ok(Some(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        )));
    }

    Ok(None)
}

fn resolve_action_name(state: &AppState, tool_name: &str) -> String {
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
            if state.bare_tool_names {
                crate::gateway::namespace::warn_legacy_prefixed_once(tool_name);
            }
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
        if !state.bare_tool_names {
            let canonical = skill_tool_name(meta.skill_name.as_deref().unwrap_or(""), &meta.name)
                .unwrap_or_else(|| meta.name.clone());
            tracing::warn!(bare_name=%tool_name, "Deprecated bare name -- use {canonical}.");
        }
        meta.name
    } else {
        tool_name.to_string()
    }
}

fn capability_gate(
    state: &AppState,
    req: &JsonRpcRequest,
    resolved_name: &str,
    action_meta: &ActionMeta,
) -> Option<JsonRpcResponse> {
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
    Some(JsonRpcResponse::error_with_data(
        req.id.clone(),
        crate::protocol::error_codes::CAPABILITY_MISSING,
        msg,
        Some(serde_json::json!({
            "tool": resolved_name,
            "required_capabilities": action_meta.required_capabilities,
            "declared_capabilities": state.declared_capabilities.as_ref(),
            "missing_capabilities": missing,
        })),
    ))
}
