//! Async `tools/call` job dispatch for rmcp (issue #318).

use std::sync::Arc;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use dcc_mcp_actions::registry::ToolMeta;
use dcc_mcp_actions::{DispatchJobContext, with_dispatch_job_context};
use dcc_mcp_jsonrpc::{CallToolMeta, CallToolResult, ToolContent};
use dcc_mcp_models::{ExecutionMode, ThreadAffinity};

use crate::job_aware_invoker::attach_job_id_to_meta;
use crate::server_state::ServerState;

use crate::rmcp_tool_call_dispatch::{
    decode_dispatch_output, encode_dispatch_wire, use_main_thread_route,
};

pub(super) struct AsyncDispatchConfig {
    pub parent_job_id: Option<String>,
    pub progress_token: Option<Value>,
    pub thread_affinity: ThreadAffinity,
    pub enforce_thread_affinity: bool,
}

struct AsyncExecutionRequest {
    job_id: String,
    cancel_token: CancellationToken,
    resolved_name: String,
    call_params: Value,
    call_meta: Option<Value>,
    thread_affinity: ThreadAffinity,
    enforce_thread_affinity: bool,
}

fn dispatch_job_context(job_id: &str, cancel_token: &CancellationToken) -> DispatchJobContext {
    let cancel_token = cancel_token.clone();
    DispatchJobContext::new(job_id.to_string(), move || cancel_token.is_cancelled())
}

pub(super) fn async_dispatch_config(
    call_meta: Option<&CallToolMeta>,
    action_meta: &ToolMeta,
) -> Option<AsyncDispatchConfig> {
    let meta_dcc = call_meta.and_then(|m| m.dcc.as_ref());
    let async_opt_in = meta_dcc.is_some_and(|dcc| dcc.r#async);
    let progress_token = call_meta.and_then(|m| m.progress_token.clone());
    let action_declares_async = matches!(action_meta.execution, ExecutionMode::Async)
        || action_meta.timeout_hint_secs.unwrap_or(0) > 0;

    if !(async_opt_in || progress_token.is_some() || action_declares_async) {
        return None;
    }

    Some(AsyncDispatchConfig {
        parent_job_id: meta_dcc.and_then(|dcc| dcc.parent_job_id.clone()),
        progress_token,
        thread_affinity: action_meta.thread_affinity,
        enforce_thread_affinity: action_meta.enforce_thread_affinity,
    })
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
        content: vec![ToolContent::Text {
            text: format!("Job {job_id} queued"),
        }],
        structured_content: Some(structured_with_meta),
        is_error: false,
        meta: None,
    }
}

async fn run_async_execution_lane(
    state: &ServerState,
    request: AsyncExecutionRequest,
) -> Result<Value, String> {
    let AsyncExecutionRequest {
        job_id,
        cancel_token,
        resolved_name,
        call_params,
        call_meta,
        thread_affinity,
        enforce_thread_affinity,
    } = request;
    let call_meta = attach_job_id_to_meta(call_meta, &job_id);
    let dispatcher = state.dispatcher.as_ref().clone();
    let use_main_thread = use_main_thread_route(thread_affinity, state.executor.is_some());
    let standalone_main =
        state.standalone_main_thread_execution && matches!(thread_affinity, ThreadAffinity::Main);

    if matches!(thread_affinity, ThreadAffinity::Main)
        && state.executor.is_none()
        && !standalone_main
    {
        if enforce_thread_affinity {
            return Err(
                "THREAD_AFFINITY_UNAVAILABLE: tool declares thread_affinity=main, \
                 but no DeferredExecutor is wired"
                    .to_string(),
            );
        }
        tracing::warn!(
            tool = %resolved_name,
            "tool declares thread_affinity=main but no DeferredExecutor is wired; \
             falling back to Tokio worker — scene API calls will be unsafe"
        );
    }

    if let Some(executor) = state.executor.as_ref().filter(|_| use_main_thread) {
        let dispatch_name = resolved_name.clone();
        let dispatch_params = call_params.clone();
        let dispatch = dispatcher.clone();
        let job_context = dispatch_job_context(&job_id, &cancel_token);
        let response = executor.submit_deferred(
            &resolved_name,
            cancel_token.clone(),
            Box::new(move || {
                with_dispatch_job_context(job_context, || {
                    match dcc_mcp_actions::with_thread_affinity(ThreadAffinity::Main, || {
                        dispatch.dispatch(&dispatch_name, dispatch_params, call_meta)
                    }) {
                        Ok(result) => encode_dispatch_wire(Ok(result)),
                        Err(err) => encode_dispatch_wire(Err(err)),
                    }
                })
            }),
        );

        let outcome = match response.await {
            Ok(json_str) => decode_dispatch_output(&json_str),
            Err(_) => Err("CANCELLED".to_string()),
        };
        if cancel_token.is_cancelled() {
            Err("CANCELLED".to_string())
        } else {
            outcome
        }
    } else {
        let dispatch = dispatcher;
        let dispatch_name = resolved_name;
        let dispatch_params = call_params;
        let dispatch_cancel = cancel_token.clone();
        let job_context = dispatch_job_context(&job_id, &cancel_token);
        let blocking = tokio::task::spawn_blocking(move || {
            if dispatch_cancel.is_cancelled() {
                return Err("CANCELLED".to_string());
            }
            let result = with_dispatch_job_context(job_context, || {
                if standalone_main {
                    dcc_mcp_actions::with_thread_affinity(ThreadAffinity::Main, || {
                        dispatch.dispatch(&dispatch_name, dispatch_params, call_meta)
                    })
                } else {
                    dispatch.dispatch(&dispatch_name, dispatch_params, call_meta)
                }
            });
            result
                .map(|result| result.output)
                .map_err(|err| err.to_string())
        });

        let outcome = blocking
            .await
            .map_err(|err| err.to_string())
            .and_then(|inner| inner);
        if cancel_token.is_cancelled() {
            Err("CANCELLED".to_string())
        } else {
            outcome
        }
    }
}

fn spawn_async_registry_dispatch(state: &ServerState, request: AsyncExecutionRequest) {
    let jobs = Arc::clone(&state.jobs);
    let server = state.clone();
    let spawn_job_id = request.job_id.clone();

    tokio::spawn(async move {
        if request.cancel_token.is_cancelled() {
            tracing::debug!(job_id = %spawn_job_id, "job cancelled before execution");
            let _ = jobs.acknowledge_cancel(&spawn_job_id);
            return;
        }
        if jobs.start(&spawn_job_id).is_none() {
            tracing::debug!(job_id = %spawn_job_id, "job could not enter Running state");
            return;
        }

        let exec_result = run_async_execution_lane(&server, request).await;

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
                    let _ = jobs.acknowledge_cancel(&spawn_job_id);
                }
            }
            Err(msg) => {
                jobs.fail(&spawn_job_id, msg);
            }
        }
    });
}

pub(super) async fn dispatch_async_registry_tool(
    state: &ServerState,
    session_id: Option<&str>,
    resolved_name: String,
    call_params: Value,
    call_meta: Option<Value>,
    cfg: AsyncDispatchConfig,
) -> CallToolResult {
    let job_handle = state
        .jobs
        .create_with_parent(resolved_name.clone(), cfg.parent_job_id.clone());
    let (job_id, cancel_token) = {
        let job = job_handle.read();
        (job.id.clone(), job.cancel_token.clone())
    };

    if let Some(session) = session_id {
        state.job_notifier.subscribe_session(session);
        state
            .job_notifier
            .register_job(&job_id, session, cfg.progress_token.clone());
    }

    tracing::info!(
        job_id = %job_id,
        tool = %resolved_name,
        parent_job_id = ?cfg.parent_job_id,
        affinity = %cfg.thread_affinity,
        "async job dispatched"
    );

    spawn_async_registry_dispatch(
        state,
        AsyncExecutionRequest {
            job_id: job_id.clone(),
            cancel_token,
            resolved_name,
            call_params,
            call_meta,
            thread_affinity: cfg.thread_affinity,
            enforce_thread_affinity: cfg.enforce_thread_affinity,
        },
    );

    build_pending_envelope(&job_id, cfg.parent_job_id)
}
