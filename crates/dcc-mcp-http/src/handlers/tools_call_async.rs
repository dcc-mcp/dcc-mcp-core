use super::*;

use dcc_mcp_actions::ActionMeta;

pub(super) struct AsyncDispatchConfig {
    pub parent_job_id: Option<String>,
    pub progress_token: Option<Value>,
    pub thread_affinity: dcc_mcp_models::ThreadAffinity,
}

pub(super) fn async_dispatch_config(
    params: &CallToolParams,
    action_meta: &ActionMeta,
) -> Option<AsyncDispatchConfig> {
    let meta_dcc = params.meta.as_ref().and_then(|m| m.dcc.as_ref());
    let async_opt_in = meta_dcc.is_some_and(|dcc| dcc.r#async);
    let progress_token = params.meta.as_ref().and_then(|m| m.progress_token.clone());
    let action_declares_async =
        matches!(action_meta.execution, dcc_mcp_models::ExecutionMode::Async)
            || action_meta.timeout_hint_secs.unwrap_or(0) > 0;

    if !(async_opt_in || progress_token.is_some() || action_declares_async) {
        return None;
    }

    Some(AsyncDispatchConfig {
        parent_job_id: meta_dcc.and_then(|dcc| dcc.parent_job_id.clone()),
        progress_token,
        thread_affinity: action_meta.thread_affinity,
    })
}

/// Async job dispatch path for `tools/call` (issue #318).
///
/// Creates a [`crate::job::Job`] via `state.jobs`, spawns the actual tool
/// execution on Tokio, and returns immediately with a spec-compliant
/// `CallToolResult` envelope.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_async_job(
    state: &AppState,
    req: &JsonRpcRequest,
    resolved_name: String,
    call_params: Value,
    parent_job_id: Option<String>,
    session_id: Option<&str>,
    progress_token: Option<Value>,
    thread_affinity: dcc_mcp_models::ThreadAffinity,
) -> Result<JsonRpcResponse, HttpError> {
    let job_handle = state
        .jobs
        .create_with_parent(resolved_name.clone(), parent_job_id.clone());
    let (job_id, cancel_token) = {
        let job = job_handle.read();
        (job.id.clone(), job.cancel_token.clone())
    };

    if let Some(session) = session_id {
        state.job_notifier.subscribe_session(session);
        state
            .job_notifier
            .register_job(&job_id, session, progress_token.clone());
    }

    tracing::info!(
        job_id = %job_id,
        tool = %resolved_name,
        parent_job_id = ?parent_job_id,
        affinity = %thread_affinity,
        "async job dispatched"
    );

    spawn_async_execution(
        state,
        job_id.clone(),
        cancel_token,
        resolved_name,
        call_params,
        thread_affinity,
    );

    Ok(JsonRpcResponse::success(
        req.id.clone(),
        serde_json::to_value(build_pending_envelope(&job_id, parent_job_id))?,
    ))
}

fn spawn_async_execution(
    state: &AppState,
    job_id: String,
    cancel_token: tokio_util::sync::CancellationToken,
    resolved_name: String,
    call_params: Value,
    thread_affinity: dcc_mcp_models::ThreadAffinity,
) {
    let jobs = Arc::clone(&state.jobs);
    let dispatcher = Arc::clone(&state.dispatcher);
    let executor = state.executor.clone();
    let spawn_job_id = job_id.clone();
    let spawn_name = resolved_name.clone();
    let spawn_params = call_params;
    let use_main_thread = matches!(thread_affinity, dcc_mcp_models::ThreadAffinity::Main);

    if use_main_thread && executor.is_none() {
        tracing::warn!(
            tool = %spawn_name,
            "tool declares thread_affinity=main but no DeferredExecutor is wired; \
             falling back to Tokio worker — scene API calls will be unsafe"
        );
    }

    tokio::spawn(async move {
        if cancel_token.is_cancelled() {
            tracing::debug!(job_id = %spawn_job_id, "job cancelled before execution");
            return;
        }
        if jobs.start(&spawn_job_id).is_none() {
            tracing::debug!(job_id = %spawn_job_id, "job could not enter Running state");
            return;
        }

        let exec_result = run_async_execution_lane(
            dispatcher,
            executor,
            spawn_name.clone(),
            spawn_params,
            cancel_token.clone(),
            use_main_thread,
        )
        .await;

        match exec_result {
            Ok(output) => {
                if jobs.complete(&spawn_job_id, output).is_none() {
                    tracing::debug!(
                        job_id = %spawn_job_id,
                        "job.complete rejected — likely cancelled concurrently"
                    );
                }
            }
            Err(msg) if msg == "CANCELLED" => {
                if jobs
                    .get(&spawn_job_id)
                    .map(|handle| handle.read().status)
                    .is_some_and(|status| !status.is_terminal())
                {
                    jobs.cancel(&spawn_job_id);
                }
            }
            Err(msg) => {
                jobs.fail(&spawn_job_id, msg);
            }
        }
    });
}

async fn run_async_execution_lane(
    dispatcher: Arc<dcc_mcp_actions::ActionDispatcher>,
    executor: Option<crate::executor::DccExecutorHandle>,
    tool_name: String,
    call_params: Value,
    cancel_token: tokio_util::sync::CancellationToken,
    use_main_thread: bool,
) -> Result<Value, String> {
    if let Some(executor) = executor.as_ref().filter(|_| use_main_thread) {
        let dispatch_name = tool_name.clone();
        let dispatch_params = call_params.clone();
        let dispatch = Arc::clone(&dispatcher);
        let response = executor.submit_deferred(
            &tool_name,
            cancel_token.clone(),
            Box::new(
                move || match dispatch.dispatch(&dispatch_name, dispatch_params) {
                    Ok(result) => {
                        serde_json::to_string(&result.output).unwrap_or_else(|_| "null".into())
                    }
                    Err(err) => {
                        serde_json::to_string(&json!({"__dispatch_error": err.to_string()}))
                            .unwrap_or_default()
                    }
                },
            ),
        );

        tokio::select! {
            outcome = response => match outcome {
                Ok(json_str) => decode_dispatch_output(&json_str),
                Err(_) => Err("CANCELLED".to_string()),
            },
            _ = cancel_token.cancelled() => Err("CANCELLED".to_string()),
        }
    } else {
        let dispatch = Arc::clone(&dispatcher);
        let dispatch_name = tool_name;
        let dispatch_params = call_params;
        let dispatch_cancel = cancel_token.clone();
        let blocking = tokio::task::spawn_blocking(move || {
            if dispatch_cancel.is_cancelled() {
                return Err("CANCELLED".to_string());
            }
            dispatch
                .dispatch(&dispatch_name, dispatch_params)
                .map(|result| result.output)
                .map_err(|err| err.to_string())
        });

        tokio::select! {
            outcome = blocking => outcome.map_err(|err| err.to_string()).and_then(|inner| inner),
            _ = cancel_token.cancelled() => Err("CANCELLED".to_string()),
        }
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

fn build_pending_envelope(job_id: &str, parent_job_id: Option<String>) -> CallToolResult {
    let structured = json!({
        "job_id": job_id,
        "status": "pending",
        "parent_job_id": parent_job_id,
    });
    let mut meta = serde_json::Map::new();
    meta.insert("status".to_string(), json!("pending"));
    let mut dcc_meta = serde_json::Map::new();
    dcc_meta.insert("jobId".to_string(), json!(job_id));
    dcc_meta.insert(
        "parentJobId".to_string(),
        parent_job_id
            .as_ref()
            .map(|parent| json!(parent))
            .unwrap_or(Value::Null),
    );
    meta.insert("dcc".to_string(), Value::Object(dcc_meta));

    let structured_with_meta = {
        let mut payload = structured.as_object().cloned().unwrap_or_default();
        payload.insert("_meta".to_string(), Value::Object(meta));
        Value::Object(payload)
    };

    CallToolResult {
        content: vec![protocol::ToolContent::Text {
            text: format!("Job {job_id} queued"),
        }],
        structured_content: Some(structured_with_meta),
        is_error: false,
        meta: None,
    }
}
