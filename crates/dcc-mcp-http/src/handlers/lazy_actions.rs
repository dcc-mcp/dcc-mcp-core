use super::*;

pub async fn handle_list_actions(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();
    let dcc = args.and_then(|a| a.get("dcc")).and_then(Value::as_str);
    let skill_filter = args.and_then(|a| a.get("skill")).and_then(Value::as_str);

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
        // Action id is the canonical tool name — matches what `tools/list`
        // would have emitted, so `call_action(id=...)` is interchangeable
        // with a direct `tools/call { name: id }`.
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
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string(&payload)?))?,
    ))
}

/// Handle ``describe_action`` — full JSON schema for a single action.
pub async fn handle_describe_action(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let id = match params
        .arguments
        .as_ref()
        .and_then(|a| a.get("id"))
        .and_then(Value::as_str)
    {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error("Missing required parameter: id"))?,
            ));
        }
    };

    // Accept both the canonical skill-prefixed id (what `list_actions`
    // returns) and the bare registry name, so the agent can round-trip
    // through either `tools/list` or the fast-path.
    let meta = resolve_action_by_id(&state.registry, &id);

    let Some(meta) = meta else {
        let envelope = DccMcpError::new(
            "registry",
            "ACTION_NOT_FOUND",
            format!("Unknown action id: {id}"),
        )
        .with_hint("Call list_actions to see available ids.");
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    };

    // Mirror the exact shape `tools/list` would have produced for this
    // action so agents can reuse a single parser.
    let include_output_schema = session_id
        .and_then(|sid| state.sessions.get_protocol_version(sid))
        .as_deref()
        == Some("2025-06-18");
    // `describe_action` is a single-action view — passing an empty
    // bare-eligible set keeps it on the canonical `<skill>.<action>` form
    // rather than synthesising a bare name that might collide against a
    // peer action the caller didn't ask about.
    let bare_eligible_for_describe = std::collections::HashSet::new();
    let tool = action_meta_to_mcp_tool(
        &meta,
        include_output_schema,
        &bare_eligible_for_describe,
        state.declared_capabilities.as_ref(),
    );
    let payload = serde_json::to_value(tool)?;

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::text(serde_json::to_string(&payload)?))?,
    ))
}

/// Handle ``call_action`` — generic dispatcher that delegates to the same
/// tool-call path as a direct `tools/call`.
///
/// Implementation strategy: rewrite the incoming request's ``params``
/// into a standard ``CallToolParams { name: id, arguments: args }`` shape
/// and recurse into [`handle_tools_call`]. Because the recursion target
/// rejects ``list_actions`` / ``describe_action`` / ``call_action`` names
/// (the dispatch branch only matches when `state.lazy_actions` is true
/// **and** the name is one of the three), we guard against infinite
/// recursion by rejecting those three names explicitly.
pub async fn handle_call_action(
    state: &AppState,
    req: &JsonRpcRequest,
    params: &CallToolParams,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let args = params.arguments.as_ref();
    let id = match args.and_then(|a| a.get("id")).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error("Missing required parameter: id"))?,
            ));
        }
    };

    // Guard: refuse to call the fast-path meta-tools through themselves.
    // This also makes their discoverability less surprising — the agent
    // cannot recurse into `call_action(id="call_action")`.
    if matches!(
        id.as_str(),
        "list_actions" | "describe_action" | "call_action"
    ) {
        let envelope = DccMcpError::new(
            "registry",
            "RECURSIVE_META_CALL",
            format!("`call_action` refuses to dispatch meta-tool `{id}`."),
        )
        .with_hint("Call the meta-tool directly via tools/call instead.");
        return Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
        ));
    }

    let inner_args = args.and_then(|a| a.get("args")).cloned();

    // Build a synthetic request that looks identical to a direct
    // `tools/call` on the target action. Preserving the original
    // JSON-RPC id/meta keeps progress/cancellation tokens working.
    let inner_params = CallToolParams {
        name: id,
        arguments: inner_args,
        meta: params.meta.clone(),
    };
    let inner_req = JsonRpcRequest {
        jsonrpc: req.jsonrpc.clone(),
        id: req.id.clone(),
        method: "tools/call".to_string(),
        params: Some(serde_json::to_value(&inner_params)?),
    };

    // `Box::pin` is required because this async fn would otherwise form a
    // recursion cycle with `handle_tools_call` (which routes back into us
    // on the `call_action` branch). The meta-tool guard above guarantees
    // the recursion terminates in one step — we only ever call through
    // to a real action.
    // Recurse through the `_inner` variant — the outer wrapper has
    // already started the Prometheus timer for this request; letting
    // the recursion hit the wrapper again would double-count.
    Box::pin(handle_tools_call_inner(state, &inner_req, session_id)).await
}

/// Look up an action by the id surfaced in `list_actions` (canonical
/// `<skill>.<tool>` form or bare registry name), returning a cloned
/// [`ActionMeta`] for downstream inspection.
pub fn resolve_action_by_id(
    registry: &dcc_mcp_actions::registry::ActionRegistry,
    id: &str,
) -> Option<dcc_mcp_actions::registry::ActionMeta> {
    // Fast path: direct registry hit (happens for bare action names).
    if let Some(m) = registry.get_action(id, None) {
        return Some(m);
    }
    // Canonical `<skill>.<tool>` form — decode and match by skill.
    if let Some((skill_part, bare_tool)) = decode_skill_tool_name(id) {
        return registry
            .list_actions_by_skill(skill_part)
            .into_iter()
            .find(|m| extract_bare_tool_name(skill_part, &m.name) == bare_tool);
    }
    None
}

/// Send a `notifications/tools/list_changed` event to a session's SSE subscribers.
pub fn notify_tools_list_changed(sessions: &SessionManager, session_id: &str) {
    let event = NotificationBuilder::new("notifications/tools/list_changed")
        .with_empty_params()
        .as_sse_event();
    sessions.push_event(session_id, event);
    tracing::debug!("Sent tools/list_changed notification to session {session_id}");
}

/// Send a delta or full list_changed notification depending on client capability.
pub fn notify_tools_changed(
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
        tracing::debug!(
            session_id,
            added = added.len(),
            removed = removed.len(),
            "Sent tools/delta notification"
        );
    } else {
        notify_tools_list_changed(sessions, session_id);
    }
}

/// Emit an MCP `notifications/message` event when the message level passes the
/// session threshold. Every message is still retained for `details.log_tail`.
pub fn notify_message(sessions: &SessionManager, session_id: &str, entry: SessionLogMessage) {
    let emit_level = entry.level;
    let request_id = entry.request_id.clone();
    let logger = entry.logger.clone();
    let data = entry.data.clone();
    let _ = sessions.push_log_message(session_id, entry);

    let threshold = sessions.get_log_level(session_id);
    if !threshold.allows(emit_level) {
        return;
    }

    let event = NotificationBuilder::new("notifications/message")
        .with_params(json!({
            "level": emit_level.as_str(),
            "logger": logger.clone(),
            "data": data,
        }))
        .as_sse_event();
    sessions.push_event(session_id, event);
    tracing::debug!(
        session_id,
        level = emit_level.as_str(),
        logger,
        request_id = request_id.unwrap_or_default(),
        "Sent notifications/message"
    );
}

pub fn request_id_to_string(id: Option<&Value>) -> Option<String> {
    let id = id?;
    let s = match id {
        Value::String(v) => v.clone(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    if s.is_empty() { None } else { Some(s) }
}

// ── Helpers ───────────────────────────────────────────────────────────────

pub fn parse_raw_values(body: &str) -> Result<Vec<Value>, serde_json::Error> {
    if body.trim_start().starts_with('[') {
        serde_json::from_str::<Vec<Value>>(body)
    } else {
        serde_json::from_str::<Value>(body).map(|v| vec![v])
    }
}

pub fn parse_body(body: &str) -> Result<JsonRpcBatch, serde_json::Error> {
    // Try array first, then single object.
    // JSON-RPC 2.0: a "notification" is a request WITHOUT an "id" field.
    // We normalise both to JsonRpcMessage so callers can use `has_id` to
    // decide whether a response is expected.
    if body.trim_start().starts_with('[') {
        serde_json::from_str::<JsonRpcBatch>(body)
    } else {
        serde_json::from_str::<JsonRpcMessage>(body).map(|m| vec![m])
    }
}

pub fn json_error_response(
    status: StatusCode,
    id: Option<Value>,
    code: i64,
    message: impl Into<String>,
) -> Response {
    let body =
        serde_json::to_string(&JsonRpcResponse::error(id, code, message)).unwrap_or_default();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

pub async fn refresh_roots_cache_for_session(
    sessions: &SessionManager,
    session_id: &str,
) -> Vec<crate::protocol::ClientRoot> {
    let event = JsonRpcRequestBuilder::new(format!("roots-refresh-{session_id}"), "roots/list")
        .with_params(json!({}))
        .as_sse_event();
    sessions.push_event(session_id, event);

    // Current low-risk phase: opportunistically keep existing cache.
    // Full client response correlation will be added in follow-up.
    let _ = tokio::time::timeout(ROOTS_REFRESH_TIMEOUT, async {}).await;
    sessions.get_client_roots(session_id)
}
