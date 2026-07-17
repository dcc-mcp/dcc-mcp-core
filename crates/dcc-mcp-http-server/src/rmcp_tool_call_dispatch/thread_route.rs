//! Main-thread vs worker routing for registry tool dispatch.

use serde_json::Value;

use dcc_mcp_actions::{
    DispatchError, DispatchExecutionContext, DispatchResult, with_execution_context,
};
use dcc_mcp_models::ThreadAffinity;

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

async fn run_on_main_thread(
    executor: &DccExecutorHandle,
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    resolved_name: String,
    call_params: Value,
    meta: Option<Value>,
    exec_ctx: DispatchExecutionContext,
) -> Result<DispatchResult, DispatchError> {
    let json_str = executor
        .execute(Box::new(move || {
            with_execution_context(exec_ctx, || {
                encode_dispatch_wire(dcc_mcp_actions::with_thread_affinity(
                    ThreadAffinity::Main,
                    || dispatcher.dispatch(&resolved_name, call_params, meta),
                ))
            })
        }))
        .await
        .map_err(|e| DispatchError::HandlerError(e.to_string()))?;
    decode_dispatch_wire(&json_str)
}

async fn run_on_worker(
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    resolved_name: String,
    call_params: Value,
    meta: Option<Value>,
    exec_ctx: DispatchExecutionContext,
    standalone_main_thread_execution: bool,
    thread_affinity: ThreadAffinity,
) -> Result<DispatchResult, DispatchError> {
    let dispatch_fut = tokio::task::spawn_blocking(move || {
        with_execution_context(exec_ctx, || {
            if standalone_main_thread_execution && matches!(thread_affinity, ThreadAffinity::Main) {
                dcc_mcp_actions::with_thread_affinity(ThreadAffinity::Main, || {
                    dispatcher.dispatch(&resolved_name, call_params, meta)
                })
            } else {
                dispatcher.dispatch(&resolved_name, call_params, meta)
            }
        })
    });
    dispatch_fut
        .await
        .map_err(|err| DispatchError::HandlerError(err.to_string()))?
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

        let task = tokio::spawn(run_on_worker(
            dispatcher,
            "slow_worker".into(),
            Value::Null,
            None,
            DispatchExecutionContext {
                host_dispatcher_attached: Some(false),
            },
            false,
            ThreadAffinity::Any,
        ));
        tokio::task::yield_now().await;
        started_rx.recv().unwrap();
        tokio::time::advance(crate::inflight::CANCEL_GRACE_PERIOD * 2).await;
        release_tx.send(()).unwrap();

        let result = task.await.unwrap().unwrap();
        assert_eq!(result.output["ok"], true);
    }
}

/// Route a tool dispatch through the same main-thread executor path as MCP
/// `tools/call`. Used by REST `POST /v1/call` via [`crate::ThreadRoutedInvoker`].
pub async fn dispatch_action_with_thread_routing(
    request: ThreadRoutingDispatch<'_>,
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
        )
        .await
    } else {
        run_on_worker(
            dispatcher,
            resolved_name.to_string(),
            call_params,
            meta,
            exec_ctx,
            standalone_main,
            thread_affinity,
        )
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
