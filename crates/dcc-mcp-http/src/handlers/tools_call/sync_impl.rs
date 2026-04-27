use super::*;

use super::resolve_impl::ResolvedToolCall;

pub(super) async fn dispatch_sync_tool_call(
    state: &AppState,
    req: &JsonRpcRequest,
    session_id: Option<&str>,
    resolved: ResolvedToolCall,
) -> Result<JsonRpcResponse, HttpError> {
    let request_id = request_id_string(req);
    log_tool_call_received(
        state,
        session_id,
        &request_id,
        &resolved.tool_name,
        &resolved.resolved_name,
    );

    let progress_token = resolved
        .params
        .meta
        .as_ref()
        .and_then(|meta| meta.progress_token.clone());
    let cancel_token = CancelToken::new();
    let progress_reporter = ProgressReporter::new(
        progress_token.clone(),
        session_id.map(str::to_owned),
        state.sessions.clone(),
        request_id.clone().unwrap_or_default(),
    );
    let tracked_job_id = register_sync_job_tracking(
        state,
        session_id,
        progress_token.clone(),
        &resolved.tool_name,
    );

    register_in_flight(
        state,
        &request_id,
        cancel_token.clone(),
        progress_reporter,
        progress_token.is_some(),
    );
    if let Some(response) = early_cancelled_response(state, req, &request_id)? {
        return Ok(response);
    }

    let dispatch_outcome = execute_sync_dispatch(
        state,
        &resolved.resolved_name,
        resolved.call_params.clone(),
        cancel_token.clone(),
    )
    .await;

    if let Some(ref rid) = request_id {
        state.in_flight.remove(rid);
    }
    update_tracked_job(state, tracked_job_id.as_deref(), &dispatch_outcome);

    let mut call_result = match build_call_result(
        state,
        session_id,
        &request_id,
        &resolved.tool_name,
        dispatch_outcome,
    )? {
        Some(result) => result,
        None => {
            return cancelled_response(
                state,
                req,
                request_id.as_deref().unwrap_or("unknown"),
                false,
            );
        }
    };

    if let Some(response) = suppress_cancelled_result(state, req, &request_id)? {
        return Ok(response);
    }

    attach_next_tools_meta(&mut call_result, &resolved.action_meta.next_tools);
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(call_result)?,
    ))
}

fn request_id_string(req: &JsonRpcRequest) -> Option<String> {
    req.id.as_ref().map(|id| match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    })
}

fn log_tool_call_received(
    state: &AppState,
    session_id: Option<&str>,
    request_id: &Option<String>,
    tool_name: &str,
    resolved_name: &str,
) {
    if let Some(session) = session_id {
        notify_message(
            &state.sessions,
            session,
            SessionLogMessage {
                level: SessionLogLevel::Debug,
                logger: "dcc_mcp_http.tools".to_string(),
                data: json!({
                    "event": "tools_call_received",
                    "tool_name": tool_name,
                    "resolved_name": resolved_name,
                }),
                request_id: request_id.clone(),
            },
        );
    }
}

fn register_sync_job_tracking(
    state: &AppState,
    session_id: Option<&str>,
    progress_token: Option<Value>,
    tool_name: &str,
) -> Option<String> {
    let tracking_session = session_id.map(str::to_owned);
    let session = tracking_session.as_deref()?;
    if progress_token.is_none() && !state.job_notifier.job_updates_enabled() {
        return None;
    }
    state.job_notifier.subscribe_session(session);
    let handle = state.jobs.create(tool_name.to_string());
    let job_id = handle.read().id.clone();
    state
        .job_notifier
        .register_job(&job_id, session, progress_token);
    state.jobs.start(&job_id);
    Some(job_id)
}

fn register_in_flight(
    state: &AppState,
    request_id: &Option<String>,
    cancel_token: CancelToken,
    progress_reporter: ProgressReporter,
    has_progress_token: bool,
) {
    if let Some(rid) = request_id {
        let entry = InFlightEntry::new(cancel_token, progress_reporter);
        state.in_flight.insert(rid.clone(), entry);
        tracing::debug!(
            request_id = %rid,
            has_progress_token,
            "registered in-flight request"
        );
    }
}

fn early_cancelled_response(
    state: &AppState,
    req: &JsonRpcRequest,
    request_id: &Option<String>,
) -> Result<Option<JsonRpcResponse>, HttpError> {
    let Some(rid) = request_id else {
        return Ok(None);
    };

    let already_cancelled = state
        .cancelled_requests
        .get(rid)
        .is_some_and(|timestamp| timestamp.elapsed() < CANCELLED_REQUEST_TTL);
    if !already_cancelled {
        return Ok(None);
    }

    state.in_flight.remove(rid);
    state.cancelled_requests.remove(rid);
    tracing::info!(request_id = %rid, "request cancelled before dispatch");
    Ok(Some(cancelled_response(state, req, rid, true)?))
}

async fn execute_sync_dispatch(
    state: &AppState,
    resolved_name: &str,
    call_params: Value,
    cancel_token: CancelToken,
) -> Result<Value, String> {
    if let Some(executor) = &state.executor {
        run_on_main_thread(
            state,
            executor,
            resolved_name.to_string(),
            call_params,
            cancel_token,
        )
        .await
    } else {
        run_on_worker(state, resolved_name.to_string(), call_params, cancel_token).await
    }
}

async fn run_on_main_thread(
    state: &AppState,
    executor: &crate::executor::DccExecutorHandle,
    resolved_name: String,
    call_params: Value,
    cancel_token: CancelToken,
) -> Result<Value, String> {
    let dispatcher = state.dispatcher.clone();
    executor
        .execute(Box::new(move || {
            if cancel_token.is_cancelled() {
                return serde_json::to_string(&json!({"__dispatch_error": "CANCELLED"}))
                    .unwrap_or_default();
            }
            match dispatcher.dispatch(&resolved_name, call_params) {
                Ok(result) => {
                    serde_json::to_string(&result.output).unwrap_or_else(|_| "null".to_string())
                }
                Err(err) => serde_json::to_string(&json!({"__dispatch_error": err.to_string()}))
                    .unwrap_or_default(),
            }
        }))
        .await
        .map(|json_str| decode_dispatch_output(&json_str))
        .unwrap_or_else(|err| Err(err.to_string()))
}

async fn run_on_worker(
    state: &AppState,
    resolved_name: String,
    call_params: Value,
    cancel_token: CancelToken,
) -> Result<Value, String> {
    let dispatcher = state.dispatcher.clone();
    let dispatch_cancel = cancel_token.clone();
    let dispatch_fut = tokio::task::spawn_blocking(move || {
        if dispatch_cancel.is_cancelled() {
            return Err("CANCELLED".to_string());
        }
        dispatcher
            .dispatch(&resolved_name, call_params)
            .map(|result| result.output)
            .map_err(|err| err.to_string())
    });

    tokio::select! {
        result = dispatch_fut => result.map_err(|err| err.to_string()).and_then(|inner| inner),
        _ = async {
            let deadline = tokio::time::Instant::now() + crate::inflight::CANCEL_GRACE_PERIOD;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                if cancel_token.is_cancelled() || tokio::time::Instant::now() >= deadline {
                    break;
                }
            }
        } => Err("CANCELLED".to_string()),
    }
}

fn decode_dispatch_output(json_str: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(json_str).unwrap_or(json!({}));
    if let Some(err) = value.get("__dispatch_error") {
        Err(err.as_str().unwrap_or("dispatch error").to_string())
    } else {
        Ok(value)
    }
}

fn update_tracked_job(
    state: &AppState,
    tracked_job_id: Option<&str>,
    dispatch_outcome: &Result<Value, String>,
) {
    let Some(job_id) = tracked_job_id else {
        return;
    };
    match dispatch_outcome {
        Ok(output) => {
            state.jobs.complete(job_id, output.clone());
        }
        Err(msg) if msg == "CANCELLED" => {
            state.jobs.cancel(job_id);
        }
        Err(msg) => {
            state.jobs.fail(job_id, msg.clone());
        }
    }
}

fn build_call_result(
    state: &AppState,
    session_id: Option<&str>,
    request_id: &Option<String>,
    tool_name: &str,
    dispatch_outcome: Result<Value, String>,
) -> Result<Option<CallToolResult>, HttpError> {
    match dispatch_outcome {
        Ok(output) => Ok(Some(success_result(state, session_id, output))),
        Err(err_msg) if err_msg == "CANCELLED" => {
            let rid = request_id.as_deref().unwrap_or("unknown");
            tracing::info!(request_id = %rid, "tool call cancelled cooperatively");
            if let Some(request_id) = request_id {
                state.cancelled_requests.remove(request_id);
            }
            Ok(None)
        }
        Err(err_msg) => Ok(Some(error_result(
            state, session_id, request_id, tool_name, &err_msg,
        ))),
    }
}

fn success_result(state: &AppState, session_id: Option<&str>, output: Value) -> CallToolResult {
    let text = match &output {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    };
    let mut content = vec![protocol::ToolContent::Text { text }];
    let is_2025_06_18 = session_id
        .and_then(|sid| state.sessions.get_protocol_version(sid))
        .as_deref()
        == Some("2025-06-18");

    if is_2025_06_18 {
        content.extend(crate::resource_link::extract_resource_links(&output));
    }

    let structured_content =
        if is_2025_06_18 && matches!(&output, Value::Object(_) | Value::Array(_)) {
            Some(output)
        } else {
            None
        };

    CallToolResult {
        content,
        structured_content,
        is_error: false,
        meta: None,
    }
}

fn error_result(
    state: &AppState,
    session_id: Option<&str>,
    request_id: &Option<String>,
    tool_name: &str,
    err_msg: &str,
) -> CallToolResult {
    if let Some(session) = session_id {
        notify_message(
            &state.sessions,
            session,
            SessionLogMessage {
                level: SessionLogLevel::Error,
                logger: "dcc_mcp_http.tools".to_string(),
                data: json!({
                    "event": "tools_call_failed",
                    "tool_name": tool_name,
                    "error": err_msg,
                }),
                request_id: request_id.clone(),
            },
        );
    }

    let mut envelope = if err_msg.contains("no handler registered") {
        DccMcpError::new(
            "instance",
            "NO_HANDLER",
            format!("Tool '{tool_name}' is registered but has no handler."),
        )
        .with_hint("Register a handler via ActionDispatcher.register_handler().")
    } else {
        DccMcpError::new("instance", "EXECUTION_FAILED", err_msg)
    };

    if let (Some(session), Some(request_id)) = (session_id, request_id.as_deref()) {
        let log_tail = state
            .sessions
            .tail_logs_for_request(session, request_id, 20);
        if !log_tail.is_empty() {
            envelope = envelope.with_details(json!({ "log_tail": log_tail }));
        }
    }

    CallToolResult {
        content: vec![protocol::ToolContent::Text {
            text: envelope.to_json(),
        }],
        structured_content: None,
        is_error: true,
        meta: None,
    }
}

fn suppress_cancelled_result(
    state: &AppState,
    req: &JsonRpcRequest,
    request_id: &Option<String>,
) -> Result<Option<JsonRpcResponse>, HttpError> {
    let Some(rid) = request_id else {
        return Ok(None);
    };

    let cancelled = state
        .cancelled_requests
        .remove(rid)
        .is_some_and(|(_, recorded_at)| recorded_at.elapsed() < CANCELLED_REQUEST_TTL);
    if !cancelled {
        return Ok(None);
    }

    tracing::info!(request_id = %rid, "Suppressing result — request was cancelled");
    Ok(Some(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::error(
            DccMcpError::new(
                "gateway",
                "REQUEST_CANCELLED",
                format!("Request {rid} was cancelled by the client."),
            )
            .with_hint("Re-send the request if you still need the result.")
            .to_json(),
        ))?,
    )))
}

fn cancelled_response(
    state: &AppState,
    req: &JsonRpcRequest,
    request_id: &str,
    before_dispatch: bool,
) -> Result<JsonRpcResponse, HttpError> {
    if !before_dispatch {
        state.cancelled_requests.remove(request_id);
    }
    let message = if before_dispatch {
        format!("Request {request_id} was cancelled before dispatch.")
    } else {
        format!("Request {request_id} was cancelled by the client.")
    };
    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(CallToolResult::error(
            DccMcpError::new("registry", "CANCELLED", message)
                .with_hint("Re-send the request if you still need the result.")
                .to_json(),
        ))?,
    ))
}
