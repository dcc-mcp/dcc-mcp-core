//! REST `/v1/call` invoker that honours `thread_affinity` like MCP `tools/call`.
//!
//! [`dcc_mcp_skill_rest::DispatcherInvoker`] calls [`ToolDispatcher::dispatch`]
//! directly on a Tokio worker, which trips `THREAD_AFFINITY_VIOLATION` for
//! `affinity: main` tools when `enforce_thread_affinity` is enabled. MCP
//! `tools/call` already routes through [`crate::rmcp_tool_call_dispatch`];
//! this invoker applies the same routing for the REST surface.

use std::sync::Arc;

use dcc_mcp_actions::{DispatchExecutionContext, ToolDispatcher, with_execution_context};
use dcc_mcp_skill_rest::{
    CallOutcome, ServiceError, ServiceErrorKind, ToolInvoker, ToolSlug,
    dispatch_error_to_service_error,
};
use serde_json::Value;
use tokio::runtime::Handle;

use crate::executor::DccExecutorHandle;
use crate::rmcp_tool_call_dispatch::{ThreadRoutingDispatch, dispatch_action_with_thread_routing};

/// Invoke backend actions with main-thread routing when a host executor is wired.
pub struct ThreadRoutedInvoker {
    dispatcher: Arc<ToolDispatcher>,
    executor: DccExecutorHandle,
    /// Runtime that drains the host-bridge mpsc (see [`crate::host_bridge`]).
    ///
    /// [`dispatch_action_with_thread_routing`] must `.await` here — not on a
    /// nested `current_thread` runtime — so `run_on_main_thread` can complete
    /// while the dedicated HTTP thread is blocked in [`ToolInvoker::invoke`].
    bridge_runtime: Handle,
}

impl ThreadRoutedInvoker {
    #[must_use]
    pub fn new(
        dispatcher: Arc<ToolDispatcher>,
        executor: DccExecutorHandle,
        bridge_runtime: Handle,
    ) -> Self {
        Self {
            dispatcher,
            executor,
            bridge_runtime,
        }
    }
}

impl ToolInvoker for ThreadRoutedInvoker {
    fn invoke(
        &self,
        action_name: &str,
        params: Value,
        meta: Option<Value>,
    ) -> Result<CallOutcome, ServiceError> {
        let action_meta = self
            .dispatcher
            .registry()
            .get_action(action_name, None)
            .ok_or_else(|| {
                ServiceError::new(
                    ServiceErrorKind::UnknownSlug,
                    format!("no handler registered for '{action_name}'"),
                )
            })?;

        let dispatcher = Arc::clone(&self.dispatcher);
        let executor = self.executor.clone();
        let action = action_name.to_string();
        let affinity = action_meta.thread_affinity;
        let enforce = action_meta.enforce_thread_affinity;

        // `SkillRestService::call` is synchronous on the dedicated HTTP
        // thread's `current_thread` runtime — `Handle::block_on` there
        // panics. Hop to a plain OS thread and block on the host-bridge
        // runtime so `run_on_main_thread` can `.await` the bridge mpsc.
        let bridge_runtime = self.bridge_runtime.clone();
        let host_dispatcher_attached = true;
        let dispatch_result = std::thread::scope(|scope| {
            let join = scope.spawn(move || {
                with_execution_context(
                    DispatchExecutionContext {
                        host_dispatcher_attached: Some(host_dispatcher_attached),
                    },
                    || {
                        bridge_runtime.block_on(dispatch_action_with_thread_routing(
                            ThreadRoutingDispatch {
                                dispatcher: dispatcher.as_ref().clone(),
                                executor: Some(&executor),
                                resolved_name: &action,
                                call_params: params,
                                meta,
                                thread_affinity: affinity,
                                enforce_thread_affinity: enforce,
                                standalone_main_thread_execution: false,
                            },
                        ))
                    },
                )
                .map_err(dispatch_error_to_service_error)
            });
            join.join().map_err(|_| {
                ServiceError::new(
                    ServiceErrorKind::Internal,
                    "thread-routed REST invoke panicked",
                )
            })?
        })?;

        Ok(CallOutcome {
            slug: ToolSlug(action_name.to_string()),
            output: dispatch_result.output,
            validation_skipped: dispatch_result.validation_skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use dcc_mcp_actions::ToolDispatcher;
    use dcc_mcp_actions::registry::{ToolMeta, ToolRegistry};
    use dcc_mcp_host::{DccDispatcher, QueueDispatcher};
    use dcc_mcp_models::ThreadAffinity;
    use dcc_mcp_skill_rest::ToolInvoker;
    use serde_json::json;
    use tokio::runtime::Handle;

    use super::*;
    use crate::host_bridge::dispatcher_to_executor_handle;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rest_invoke_routes_main_affinity_through_dispatcher_tick() {
        let registry = Arc::new(ToolRegistry::new());
        registry.register_action(ToolMeta {
            name: "thread_probe".into(),
            dcc: "test".into(),
            description: "probe".into(),
            thread_affinity: ThreadAffinity::Main,
            enforce_thread_affinity: true,
            enabled: true,
            ..Default::default()
        });
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        dispatcher.register_handler("thread_probe", |_| Ok(json!({"ok": true})));

        let host_dispatcher: Arc<dyn DccDispatcher> = Arc::new(QueueDispatcher::new());
        let bridge_rt = Handle::current();
        let executor = dispatcher_to_executor_handle(host_dispatcher.clone(), &bridge_rt);

        let tick_dispatcher = host_dispatcher.clone();
        let ticker = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                if tick_dispatcher.tick(16).jobs_executed > 0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("dispatcher tick thread saw no jobs");
        });

        let invoker = ThreadRoutedInvoker::new(dispatcher, executor, bridge_rt);
        // Production path: the dedicated HTTP thread is outside the bridge
        // runtime; it calls `invoke` which `block_on`s the bridge runtime.
        let outcome = std::thread::spawn(move || {
            invoker
                .invoke("thread_probe", json!({}), None)
                .expect("main-thread REST invoke")
        })
        .join()
        .expect("invoke thread");
        assert_eq!(outcome.output["ok"], true);
        ticker.join().unwrap();
    }
}
