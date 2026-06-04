//! Main-thread vs worker routing for registry tool dispatch.

use serde_json::Value;

use dcc_mcp_actions::{
    DispatchError, DispatchExecutionContext, DispatchResult, with_execution_context,
};
use dcc_mcp_models::ThreadAffinity;

use crate::executor::DccExecutorHandle;
use crate::inflight::CANCEL_GRACE_PERIOD;
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
    let cancel_wait = async {
        let deadline = tokio::time::Instant::now() + CANCEL_GRACE_PERIOD;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }
    };
    tokio::select! {
        result = dispatch_fut => result
            .map_err(|err| DispatchError::HandlerError(err.to_string()))?
            ,
        _ = cancel_wait => Err(DispatchError::HandlerError("CANCELLED".to_string())),
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
