// ── Core discovery tool handlers ──────────────────────────────────────────

use super::*;

pub async fn handle_list_skills(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let status = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("status"))
        .and_then(Value::as_str);

    let results = state.catalog.list_skills(status);

    let text = serde_json::to_string_pretty(&json!({
        "skills": results,
        "total": results.len()
    }))
    .unwrap_or_default();

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(text))?,
    ))
}

pub async fn handle_get_skill_info(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let skill_name = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("skill_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if skill_name.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "Missing required parameter: skill_name",
            ))?,
        ));
    }

    match state.catalog.get_skill_info(skill_name) {
        Some(info) => {
            let text = serde_json::to_string_pretty(&info).unwrap_or_default();
            Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::text(text))?,
            ))
        }
        None => Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(format!(
                "Skill '{skill_name}' not found"
            )))?,
        )),
    }
}

pub async fn handle_load_skill(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let skill_name = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("skill_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let skill_names: Vec<String> = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("skill_names"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    if skill_name.is_empty() && skill_names.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "Missing required parameter: skill_name or skill_names",
            ))?,
        ));
    }

    // Collect the full set of requested skills, deduping `skill_name` vs the
    // `skill_names` array so callers passing both don't trigger the work twice.
    let mut requested: Vec<String> = Vec::new();
    if !skill_name.is_empty() {
        requested.push(skill_name.to_string());
    }
    for name in &skill_names {
        if !requested.contains(name) {
            requested.push(name.clone());
        }
    }

    let mut all_registered_tools: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut newly_loaded: Vec<String> = Vec::new();
    let mut already_loaded: Vec<String> = Vec::new();

    for name in &requested {
        let was_loaded = state.catalog.is_loaded(name);
        match state.catalog.load_skill(name) {
            Ok(tools) => {
                all_registered_tools.extend(tools);
                if was_loaded {
                    already_loaded.push(name.clone());
                } else {
                    newly_loaded.push(name.clone());
                }
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }

    // Only notify when a skill actually transitioned to loaded.
    if !newly_loaded.is_empty() {
        state.bump_registry_generation(); // #438
        if let Some(sid) = session_id {
            let added = all_registered_tools.clone();
            let removed: Vec<String> = newly_loaded
                .iter()
                .map(|n| format!("__skill__{n}"))
                .collect();
            notify_tools_changed(&state.sessions, sid, &added, &removed);
        }
        // Skill content changed — invalidate the prompt cache and
        // broadcast `notifications/prompts/list_changed` (issues
        // #351, #355).
        state.prompts.invalidate();
        notify_prompts_list_changed_all(state);
    }

    // Build the full tool metadata so agents can invoke the new tools without
    // a second round-trip to `tools/list`.  One registry read per newly or
    // previously loaded skill; keeps the payload self-contained.
    let mut tool_schemas: Vec<Value> = Vec::new();
    for name in newly_loaded.iter().chain(already_loaded.iter()) {
        for meta in state.catalog.registry().list_actions_by_skill(name) {
            tool_schemas.push(json!({
                "name":          meta.name,
                "description":   meta.description,
                "inputSchema":   meta.input_schema,
                "outputSchema":  meta.output_schema,
                "skill_name":    meta.skill_name,
            }));
        }
    }

    // Response semantics:
    // - `loaded` is true when at least one requested skill ended up loaded
    //   (even if some others failed). This matches the caller's intuition
    //   that "tools were registered" rather than treating any failure as total.
    // - `partial` distinguishes mixed success/failure from clean success.
    let loaded_ok = !all_registered_tools.is_empty();
    let partial = loaded_ok && !errors.is_empty();

    let mut body = json!({
        "loaded":           loaded_ok,
        "partial":          partial,
        "registered_tools": all_registered_tools,
        "tool_count":       all_registered_tools.len(),
        "newly_loaded":     newly_loaded,
        "already_loaded":   already_loaded,
        "tools":            tool_schemas,
    });
    if !errors.is_empty()
        && let Some(obj) = body.as_object_mut()
    {
        obj.insert("errors".to_string(), json!(errors));
    }

    let text = serde_json::to_string_pretty(&body).unwrap_or_default();

    // `isError` only when every requested skill failed — partial success is
    // still reported as success so clients see the registered-tool list.
    let result = if loaded_ok {
        CallToolResult::text(text)
    } else {
        CallToolResult::error(text)
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
    ))
}

pub async fn handle_unload_skill(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let skill_name = params
        .arguments
        .as_ref()
        .and_then(|a| a.get("skill_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if skill_name.is_empty() {
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(
                "Missing required parameter: skill_name",
            ))?,
        ));
    }

    match state.catalog.unload_skill(skill_name) {
        Ok(count) => {
            state.bump_registry_generation(); // #438
            if let Some(sid) = session_id {
                let removed: Vec<String> = state
                    .registry
                    .list_actions_by_skill(skill_name)
                    .iter()
                    .map(|m| m.name.clone())
                    .collect();
                let added = vec![format!("__skill__{skill_name}")];
                notify_tools_changed(&state.sessions, sid, &added, &removed);
            }
            // Invalidate prompt cache and fire
            // `notifications/prompts/list_changed` (issues #351, #355).
            state.prompts.invalidate();
            notify_prompts_list_changed_all(state);

            let text = serde_json::to_string_pretty(&json!({
                "unloaded": true,
                "tools_removed": count
            }))
            .unwrap_or_default();

            Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::text(text))?,
            ))
        }
        Err(e) => Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(e))?,
        )),
    }
}
