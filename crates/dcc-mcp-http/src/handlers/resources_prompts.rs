// ── Resources (issue #350) ─────────────────────────────────────────────────

use super::*;

pub async fn handle_resources_list(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let resources = state.resources.list();
    let result = ListResourcesResult {
        resources,
        next_cursor: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
    ))
}

pub async fn handle_resources_read(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<ReadResourceParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid resources/read params (expected {uri: string})",
        ));
    };

    match state.resources.read(&params.uri) {
        Ok(result) => Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(result)?,
        )),
        Err(ResourceError::NotEnabled(msg)) => Ok(JsonRpcResponse::error(
            req.id.clone(),
            RESOURCE_NOT_ENABLED_ERROR,
            msg,
        )),
        Err(ResourceError::NotFound(msg)) => Ok(JsonRpcResponse::error(
            req.id.clone(),
            RESOURCE_NOT_ENABLED_ERROR,
            format!("resource not found: {msg}"),
        )),
        Err(ResourceError::Read(msg)) => Ok(JsonRpcResponse::internal_error(
            req.id.clone(),
            format!("resource read failed: {msg}"),
        )),
    }
}

pub async fn handle_resources_subscribe(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(sid) = session_id else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "resources/subscribe requires Mcp-Session-Id header",
        ));
    };
    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<SubscribeResourceParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid resources/subscribe params (expected {uri: string})",
        ));
    };
    state.resources.subscribe(sid, &params.uri);
    Ok(JsonRpcResponse::success(req.id.clone(), json!({})))
}

pub async fn handle_resources_unsubscribe(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(sid) = session_id else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "resources/unsubscribe requires Mcp-Session-Id header",
        ));
    };
    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<SubscribeResourceParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid resources/unsubscribe params (expected {uri: string})",
        ));
    };
    state.resources.unsubscribe(sid, &params.uri);
    Ok(JsonRpcResponse::success(req.id.clone(), json!({})))
}

// ── Prompts (issues #351, #355) ────────────────────────────────────────────

pub async fn handle_prompts_list(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let catalog = state.catalog.clone();
    let prompts = state.prompts.list(|visit| {
        catalog.for_each_loaded_metadata(|md| visit(md));
    });
    let result = ListPromptsResult {
        prompts,
        next_cursor: None,
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
    ))
}

pub async fn handle_prompts_get(
    state: &AppState,
    req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<GetPromptParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid prompts/get params (expected {name: string, arguments?: object})",
        ));
    };
    let catalog = state.catalog.clone();
    let lookup = state.prompts.get(&params.name, &params.arguments, |visit| {
        catalog.for_each_loaded_metadata(|md| visit(md));
    });
    match lookup {
        Ok(result) => Ok(JsonRpcResponse::success(
            req.id.clone(),
            serde_json::to_value(result)?,
        )),
        Err(PromptError::NotFound(name)) => Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            format!("prompt not found: {name}"),
        )),
        Err(PromptError::MissingArg(arg)) => Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            format!("missing required argument: {arg}"),
        )),
        Err(PromptError::Load(msg)) => Ok(JsonRpcResponse::internal_error(
            req.id.clone(),
            format!("prompts/get load failure: {msg}"),
        )),
    }
}

/// Emit `notifications/prompts/list_changed` to every session whose SSE
/// stream is live. Called from skill load / unload paths.
pub(crate) fn notify_prompts_list_changed_all(state: &AppState) {
    if !state.enable_prompts {
        return;
    }
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/prompts/list_changed",
        "params": {}
    });
    let event = format_sse_event(&notification, None);
    for sid in state.sessions.all_ids() {
        state.sessions.push_event(&sid, event.clone());
    }
}

pub async fn handle_logging_set_level(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    let Some(sid) = session_id else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "logging/setLevel requires Mcp-Session-Id header",
        ));
    };

    let Some(params) = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value::<LoggingSetLevelParams>(p.clone()).ok())
    else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid logging/setLevel params",
        ));
    };

    let Some(level) = SessionLogLevel::parse(&params.level) else {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Invalid logging level. Expected one of: debug, info, warning, error",
        ));
    };

    if !state.sessions.set_log_level(sid, level) {
        return Ok(JsonRpcResponse::error(
            req.id.clone(),
            protocol::error_codes::INVALID_PARAMS,
            "Session not found",
        ));
    }

    let request_id = request_id_to_string(req.id.as_ref());
    notify_message(
        &state.sessions,
        sid,
        SessionLogMessage {
            level: SessionLogLevel::Info,
            logger: "dcc_mcp_http.logging".to_string(),
            data: json!({
                "event": "set_level",
                "level": level.as_str(),
            }),
            request_id,
        },
    );

    Ok(JsonRpcResponse::success(req.id.clone(), json!({})))
}

pub async fn handle_elicitation_create(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
) -> Result<JsonRpcResponse, HttpError> {
    // Spec gate: only exposed on 2025-06-18 sessions.
    let is_2025_06_18 = session_id
        .and_then(|sid| state.sessions.get_protocol_version(sid))
        .as_deref()
        == Some("2025-06-18");
    if !is_2025_06_18 {
        return Ok(JsonRpcResponse::method_not_found(
            req.id.clone(),
            "elicitation/create",
        ));
    }
    let sid = match session_id {
        Some(s) => s,
        None => {
            return Err(HttpError::Internal(
                "elicitation/create requires Mcp-Session-Id".to_string(),
            ));
        }
    };
    let elicit_id = req.id.clone().ok_or_else(|| {
        HttpError::Internal("elicitation/create requires a JSON-RPC request id".to_string())
    })?;
    let req_id = match &elicit_id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    if req_id.is_empty() {
        return Err(HttpError::Internal(
            "elicitation/create request id cannot be empty".to_string(),
        ));
    }

    let params: ElicitationCreateParams = req
        .params
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .ok_or_else(|| HttpError::Internal("invalid elicitation/create params".to_string()))?;

    let (tx, rx) = oneshot::channel::<ElicitationCreateResult>();
    state.pending_elicitations.insert(req_id.clone(), tx);

    let notification = json!({
        "jsonrpc": "2.0",
        "method": "elicitation/create",
        "params": {
            "id": elicit_id,
            "message": params.message,
            "requestedSchema": params.requested_schema,
        }
    });
    let event = format_sse_event(&notification, None);
    state.sessions.push_event(sid, event);

    let waited = tokio::time::timeout(ELICITATION_TIMEOUT, rx).await;
    state.pending_elicitations.remove(&req_id);

    let result = match waited {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => ElicitationCreateResult {
            action: "decline".to_string(),
            content: None,
        },
        Err(_) => {
            let envelope = DccMcpError::new(
                "dcc",
                "ELICITATION_TIMEOUT",
                format!(
                    "Client did not answer elicitation request {req_id} within {} seconds.",
                    ELICITATION_TIMEOUT.as_secs()
                ),
            )
            .with_hint("Ask the user again or proceed with a conservative default.");
            return Ok(JsonRpcResponse::success(
                req.id.clone(),
                serde_json::to_value(CallToolResult::error(envelope.to_json()))?,
            ));
        }
    };

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(result)?,
    ))
}
