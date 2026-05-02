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
    let params: CallToolParams = match req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
    {
        Some(params) => params,
        None => {
            return Ok(ToolCallResolution::Response(JsonRpcResponse::error(
                req.id.clone(),
                protocol::error_codes::INVALID_PARAMS,
                "Invalid tools/call params (expected {name: string, arguments?: object})",
            )));
        }
    };

    let tool_name = params.name.clone();

    if let Some(response) = route_core_tool(state, req, session_id, &params, &tool_name).await? {
        return Ok(ToolCallResolution::Response(response));
    }

    if let Some(response) = handle_stub_tool(req, &tool_name)? {
        return Ok(ToolCallResolution::Response(response));
    }

    // Check session-scoped dynamic tools before the global registry (issue #462).
    if tool_name.starts_with(crate::dynamic_tools::DYNAMIC_TOOL_PREFIX)
        && let Some(response) =
            route_dynamic_tool_call(state, req, session_id, &params, &tool_name)?
    {
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
        // Dynamic tool management (issue #462)
        "register_tool" => Some(handle_register_tool_call(state, req, session_id, params)?),
        "deregister_tool" => Some(handle_deregister_tool_call(state, req, session_id, params)?),
        "list_dynamic_tools" => Some(handle_list_dynamic_tools_call(state, req, session_id)?),
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

// ── Dynamic tool execution routing (issue #462) ───────────────────────────────

fn route_dynamic_tool_call(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
    params: &CallToolParams,
    tool_name: &str,
) -> Result<Option<JsonRpcResponse>, HttpError> {
    let sid = match session_id {
        Some(id) => id,
        None => {
            // No session — dynamic tools only exist within a session.
            return Ok(None);
        }
    };

    // Fetch the ToolSpec for this tool from the session's registry.
    let spec: Option<crate::dynamic_tools::ToolSpec> = state
        .sessions
        .with_dynamic_tools_mut(sid, |dyn_tools| {
            dyn_tools.get(tool_name).map(|e| e.spec.clone())
        })
        .flatten();

    let spec = match spec {
        Some(s) => s,
        None => {
            // Not a session dynamic tool; let the regular registry handle it.
            return Ok(None);
        }
    };

    let call_args = params.arguments.clone().unwrap_or(json!({}));

    // Execute the tool's code via the DeferredExecutor when available.
    let result = execute_dynamic_tool_code(state, &spec, call_args);
    let response = JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result)?);
    Ok(Some(response))
}

/// Execute a dynamic tool's Python code.
///
/// **Stage 1 implementation**: code inspection and argument binding are
/// available; actual in-process Python execution is a Stage 2 feature
/// (`dcc-mcp-dynamic-tools` crate, issue #462 follow-up).
///
/// For now, the executor path is set up and the generated wrapper script is
/// returned in the response so developers can verify the plumbing. When the
/// DCC adapter wires in a `PythonEvalHandler`, it will intercept this call
/// and run the script on the DCC main thread.
fn execute_dynamic_tool_code(
    state: &AppState,
    spec: &crate::dynamic_tools::ToolSpec,
    call_args: Value,
) -> Value {
    let wrapper_script = build_execution_wrapper(&spec.name, &spec.code, &call_args);

    if state.executor.is_none() {
        // No DCC executor wired — return the generated script so the caller
        // can see what would be executed. Useful in testing / pure-HTTP mode.
        return json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({
                    "status": "no_executor",
                    "message": format!(
                        "Dynamic tool '{}' is registered but requires an in-process \
                         DeferredExecutor to execute. Wire DeferredExecutor via \
                         McpHttpConfig::set_executor() in your DCC adapter.",
                        spec.name
                    ),
                    "generated_script": wrapper_script,
                    "call_args": call_args
                })).unwrap_or_default()
            }]
        });
    }

    // Executor is present — will be dispatched to the DCC main thread once
    // the Python eval pathway is completed (Stage 2, issue #462).
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "status": "pending_stage2",
                "message": format!(
                    "Dynamic tool '{}' execution queued. Full in-process Python \
                     evaluation (Stage 2, issue #462) is not yet implemented. \
                     The DCC adapter must provide a PythonEvalHandler.",
                    spec.name
                ),
                "generated_script": wrapper_script,
                "call_args": call_args
            })).unwrap_or_default()
        }]
    })
}

/// Build the Python wrapper script that binds `params` to the call arguments.
fn build_execution_wrapper(name: &str, code: &str, args: &Value) -> String {
    let args_json = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    // Escape single-quotes for embedding in a Python string literal.
    let escaped = args_json.replace('\\', "\\\\").replace('\'', "\\'");
    format!(
        "import json as _json\n\
         # Bind call arguments as 'params'\n\
         params = _json.loads('{escaped}')\n\
         \n\
         # -- dynamic tool: {name} --\n\
         {code}\n",
        escaped = escaped,
        name = name,
        code = code,
    )
}

// ── Dynamic tool management call handlers (issue #462) ───────────────────────

fn handle_register_tool_call(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let call_args = params.arguments.clone().unwrap_or(json!({}));
    let sid = match session_id {
        Some(id) => id,
        None => {
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(
                    "register_tool requires an active session (send Mcp-Session-Id header)",
                ))?,
            ));
        }
    };

    let result = state.sessions.with_dynamic_tools_mut(sid, |dyn_tools| {
        crate::dynamic_tools::handle_register_tool(dyn_tools, &call_args)
    });

    let result_value = result.unwrap_or_else(|| {
        json!({
            "isError": true,
            "content": [{ "type": "text", "text": "Session not found" }]
        })
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result_value)?,
    ))
}

fn handle_deregister_tool_call(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let call_args = params.arguments.clone().unwrap_or(json!({}));
    let sid = match session_id {
        Some(id) => id,
        None => {
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(
                    "deregister_tool requires an active session",
                ))?,
            ));
        }
    };

    let result = state.sessions.with_dynamic_tools_mut(sid, |dyn_tools| {
        crate::dynamic_tools::handle_deregister_tool(dyn_tools, &call_args)
    });

    let result_value = result.unwrap_or_else(|| {
        json!({
            "isError": true,
            "content": [{ "type": "text", "text": "Session not found" }]
        })
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result_value)?,
    ))
}

fn handle_list_dynamic_tools_call(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let sid = match session_id {
        Some(id) => id,
        None => {
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(crate::dynamic_tools::handle_list_dynamic_tools(
                    &mut crate::dynamic_tools::SessionDynamicTools::new(),
                ))?,
            ));
        }
    };

    let result = state.sessions.with_dynamic_tools_mut(sid, |dyn_tools| {
        crate::dynamic_tools::handle_list_dynamic_tools(dyn_tools)
    });

    let result_value = result.unwrap_or_else(|| {
        json!({
            "content": [{ "type": "text", "text": "{\"dynamic_tools\":[], \"count\":0}" }]
        })
    });
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result_value)?,
    ))
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
