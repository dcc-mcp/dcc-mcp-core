//! Main-thread vs worker routing for registry tool dispatch.

use serde_json::Value;

use dcc_mcp_actions::{
    DispatchError, DispatchExecutionContext, DispatchJobContext, DispatchResult,
    with_dispatch_job_context, with_execution_context,
};
use dcc_mcp_models::ThreadAffinity;
use dcc_mcp_skill_rest::InvocationCancellation;

use crate::executor::DccExecutorHandle;
use crate::server_state::ServerState;

use super::wire::{decode_dispatch_wire, encode_dispatch_wire, use_main_thread_route};

pub struct ThreadRoutingDispatch<'a> {
    pub dispatcher: dcc_mcp_actions::ToolDispatcher,
    pub executor: Option<&'a DccExecutorHandle>,
    pub resolved_name: &'a str,
    pub call_params: Value,
    pub meta: Option<Value>,
    pub thread_affinity: ThreadAffinity,
    pub enforce_thread_affinity: bool,
    pub standalone_main_thread_execution: bool,
}

fn with_optional_job_context<R>(
    context: Option<DispatchJobContext>,
    dispatch: impl FnOnce() -> R,
) -> R {
    match context {
        Some(context) => with_dispatch_job_context(context, dispatch),
        None => dispatch(),
    }
}

async fn run_on_main_thread(
    executor: &DccExecutorHandle,
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    resolved_name: String,
    call_params: Value,
    meta: Option<Value>,
    exec_ctx: DispatchExecutionContext,
    cancellation: Option<InvocationCancellation>,
) -> Result<DispatchResult, DispatchError> {
    let job_context = cancellation
        .as_ref()
        .map(InvocationCancellation::dispatch_job_context);
    let tool_name = resolved_name.clone();
    let task = Box::new(move || {
        with_optional_job_context(job_context, || {
            with_execution_context(exec_ctx, || {
                encode_dispatch_wire(dcc_mcp_actions::with_thread_affinity(
                    ThreadAffinity::Main,
                    || dispatcher.dispatch(&resolved_name, call_params, meta),
                ))
            })
        })
    });
    let json_str = if let Some(cancellation) = cancellation {
        let cancel_token = cancellation.cancel_token().clone();
        let response = executor.submit_deferred(&tool_name, cancel_token.clone(), task);
        tokio::select! {
            outcome = response => outcome
                .map_err(|_| DispatchError::HandlerError("CANCELLED".to_string()))?,
            _ = cancel_token.cancelled() => {
                return Err(DispatchError::HandlerError("CANCELLED".to_string()));
            }
        }
    } else {
        executor
            .execute(task)
            .await
            .map_err(|e| DispatchError::HandlerError(e.to_string()))?
    };
    decode_dispatch_wire(&json_str)
}

struct WorkerDispatch {
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    resolved_name: String,
    call_params: Value,
    meta: Option<Value>,
    exec_ctx: DispatchExecutionContext,
    standalone_main_thread_execution: bool,
    thread_affinity: ThreadAffinity,
    cancellation: Option<InvocationCancellation>,
}

async fn run_on_worker(request: WorkerDispatch) -> Result<DispatchResult, DispatchError> {
    let WorkerDispatch {
        dispatcher,
        resolved_name,
        call_params,
        meta,
        exec_ctx,
        standalone_main_thread_execution,
        thread_affinity,
        cancellation,
    } = request;
    if cancellation
        .as_ref()
        .is_some_and(|context| context.cancel_token().is_cancelled())
    {
        return Err(DispatchError::HandlerError("CANCELLED".to_string()));
    }
    let job_context = cancellation
        .as_ref()
        .map(InvocationCancellation::dispatch_job_context);
    let cancel_token = cancellation
        .as_ref()
        .map(|context| context.cancel_token().clone());
    let dispatch_fut = tokio::task::spawn_blocking(move || {
        with_optional_job_context(job_context, || {
            with_execution_context(exec_ctx, || {
                if standalone_main_thread_execution
                    && matches!(thread_affinity, ThreadAffinity::Main)
                {
                    dcc_mcp_actions::with_thread_affinity(ThreadAffinity::Main, || {
                        dispatcher.dispatch(&resolved_name, call_params, meta)
                    })
                } else {
                    dispatcher.dispatch(&resolved_name, call_params, meta)
                }
            })
        })
    });
    if let Some(cancel_token) = cancel_token {
        tokio::select! {
            outcome = dispatch_fut => outcome
                .map_err(|err| DispatchError::HandlerError(err.to_string()))?,
            _ = cancel_token.cancelled() => {
                Err(DispatchError::HandlerError("CANCELLED".to_string()))
            }
        }
    } else {
        dispatch_fut
            .await
            .map_err(|err| DispatchError::HandlerError(err.to_string()))?
    }
}

/// Route a tool dispatch through the same main-thread executor path as MCP
/// `tools/call`. Used by REST `POST /v1/call` via [`crate::ThreadRoutedInvoker`].
pub async fn dispatch_action_with_thread_routing(
    request: ThreadRoutingDispatch<'_>,
) -> Result<DispatchResult, DispatchError> {
    dispatch_action_with_thread_routing_inner(request, None).await
}

pub(crate) async fn dispatch_action_with_thread_routing_cancellable(
    request: ThreadRoutingDispatch<'_>,
    cancellation: InvocationCancellation,
) -> Result<DispatchResult, DispatchError> {
    dispatch_action_with_thread_routing_inner(request, Some(cancellation)).await
}

async fn dispatch_action_with_thread_routing_inner(
    request: ThreadRoutingDispatch<'_>,
    cancellation: Option<InvocationCancellation>,
) -> Result<DispatchResult, DispatchError> {
    let ThreadRoutingDispatch {
        dispatcher,
        executor,
        resolved_name,
        call_params,
        meta,
        thread_affinity,
        enforce_thread_affinity,
        standalone_main_thread_execution,
    } = request;
    let executor_present = executor.is_some();
    let standalone_main =
        standalone_main_thread_execution && matches!(thread_affinity, ThreadAffinity::Main);
    let on_main = use_main_thread_route(thread_affinity, executor_present);
    let exec_ctx = DispatchExecutionContext {
        host_dispatcher_attached: Some(executor_present),
    };

    if matches!(thread_affinity, ThreadAffinity::Main) && !executor_present && !standalone_main {
        if enforce_thread_affinity {
            return Err(DispatchError::HandlerError(format!(
                "THREAD_AFFINITY_UNAVAILABLE: action '{resolved_name}' declares thread_affinity=main, \
                 but no DeferredExecutor is wired"
            )));
        }
        tracing::warn!(
            tool = %resolved_name,
            "sync tool declares thread_affinity=main but no DeferredExecutor is wired; \
             falling back to Tokio worker — scene API calls will be unsafe"
        );
    }

    if on_main {
        let executor = executor.expect("executor presence gated by use_main_thread_route");
        run_on_main_thread(
            executor,
            dispatcher,
            resolved_name.to_string(),
            call_params,
            meta,
            exec_ctx,
            cancellation,
        )
        .await
    } else {
        run_on_worker(WorkerDispatch {
            dispatcher,
            resolved_name: resolved_name.to_string(),
            call_params,
            meta,
            exec_ctx,
            standalone_main_thread_execution: standalone_main,
            thread_affinity,
            cancellation,
        })
        .await
    }
}

pub(super) async fn execute_threaded_dispatch(
    state: &ServerState,
    resolved_name: &str,
    call_params: Value,
    meta: Option<Value>,
    thread_affinity: ThreadAffinity,
    enforce_thread_affinity: bool,
) -> Result<Value, String> {
    dispatch_action_with_thread_routing(ThreadRoutingDispatch {
        dispatcher: state.dispatcher.as_ref().clone(),
        executor: state.executor.as_ref(),
        resolved_name,
        call_params,
        meta,
        thread_affinity,
        enforce_thread_affinity,
        standalone_main_thread_execution: state.standalone_main_thread_execution,
    })
    .await
    .map(|r| r.output)
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, mpsc};

    use dcc_mcp_actions::registry::{ToolMeta, ToolRegistry};
    use serde_json::{Value, json};

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn worker_dispatch_is_not_limited_by_cancellation_grace_period() {
        let registry = ToolRegistry::new();
        registry.register_action(ToolMeta {
            name: "slow_worker".into(),
            dcc: "test".into(),
            description: "slow worker probe".into(),
            thread_affinity: ThreadAffinity::Any,
            enabled: true,
            ..Default::default()
        });
        let dispatcher = dcc_mcp_actions::ToolDispatcher::new(registry);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        dispatcher.register_handler("slow_worker", move |_| {
            started_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
            Ok(json!({"ok": true}))
        });

        let task = tokio::spawn(run_on_worker(WorkerDispatch {
            dispatcher,
            resolved_name: "slow_worker".into(),
            call_params: Value::Null,
            meta: None,
            exec_ctx: DispatchExecutionContext {
                host_dispatcher_attached: Some(false),
            },
            standalone_main_thread_execution: false,
            thread_affinity: ThreadAffinity::Any,
            cancellation: None,
        }));
        tokio::task::yield_now().await;
        started_rx.recv().unwrap();
        tokio::time::advance(crate::inflight::CANCEL_GRACE_PERIOD * 2).await;
        release_tx.send(()).unwrap();

        let result = task.await.unwrap().unwrap();
        assert_eq!(result.output["ok"], true);
    }
}
