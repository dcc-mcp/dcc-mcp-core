//! Main-thread vs worker routing for registry tool dispatch.

use serde_json::Value;

use dcc_mcp_actions::{DispatchError, DispatchResult};
use dcc_mcp_models::ThreadAffinity;

use crate::executor::DccExecutorHandle;
use crate::inflight::CANCEL_GRACE_PERIOD;
use crate::server_state::ServerState;

use super::wire::{decode_dispatch_wire, encode_dispatch_wire, use_main_thread_route};

async fn run_on_main_thread(
    executor: &DccExecutorHandle,
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    resolved_name: String,
    call_params: Value,
) -> Result<DispatchResult, DispatchError> {
    let json_str = executor
        .execute(Box::new(move || {
            encode_dispatch_wire(dcc_mcp_actions::with_thread_affinity(
                ThreadAffinity::Main,
                || dispatcher.dispatch(&resolved_name, call_params),
            ))
        }))
        .await
        .map_err(|e| DispatchError::HandlerError(e.to_string()))?;
    decode_dispatch_wire(&json_str)
}

async fn run_on_worker(
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    resolved_name: String,
    call_params: Value,
) -> Result<DispatchResult, DispatchError> {
    let dispatch_fut =
        tokio::task::spawn_blocking(move || dispatcher.dispatch(&resolved_name, call_params));
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
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    executor: Option<&DccExecutorHandle>,
    resolved_name: &str,
    call_params: Value,
    thread_affinity: ThreadAffinity,
    enforce_thread_affinity: bool,
) -> Result<DispatchResult, DispatchError> {
    let executor_present = executor.is_some();
    let on_main = use_main_thread_route(thread_affinity, executor_present);

    if matches!(thread_affinity, ThreadAffinity::Main) && !executor_present {
        if enforce_thread_affinity {
            return Err(DispatchError::HandlerError(
                "THREAD_AFFINITY_UNAVAILABLE: tool declares thread_affinity=main, \
                 but no DeferredExecutor is wired"
                    .to_string(),
            ));
        }
        tracing::warn!(
            tool = %resolved_name,
            "sync tool declares thread_affinity=main but no DeferredExecutor is wired; \
             falling back to Tokio worker — scene API calls will be unsafe"
        );
    }

    if on_main {
        let executor = executor.expect("executor presence gated by use_main_thread_route");
        run_on_main_thread(executor, dispatcher, resolved_name.to_string(), call_params).await
    } else {
        run_on_worker(dispatcher, resolved_name.to_string(), call_params).await
    }
}

pub(super) async fn execute_threaded_dispatch(
    state: &ServerState,
    resolved_name: &str,
    call_params: Value,
    thread_affinity: ThreadAffinity,
    enforce_thread_affinity: bool,
) -> Result<Value, String> {
    dispatch_action_with_thread_routing(
        state.dispatcher.as_ref().clone(),
        state.executor.as_ref(),
        resolved_name,
        call_params,
        thread_affinity,
        enforce_thread_affinity,
    )
    .await
    .map(|r| r.output)
    .map_err(|e| e.to_string())
}
