//! REST `/v1/call` invoker that honours `thread_affinity` like MCP `tools/call`.
//!
//! [`dcc_mcp_skill_rest::DispatcherInvoker`] calls [`ToolDispatcher::dispatch`]
//! directly on a Tokio worker, which trips `THREAD_AFFINITY_VIOLATION` for
//! `affinity: main` tools when `enforce_thread_affinity` is enabled. MCP
//! `tools/call` already routes through [`crate::rmcp_tool_call_dispatch`];
//! this invoker applies the same routing for the REST surface.

use std::sync::Arc;

use dcc_mcp_actions::ToolDispatcher;
use dcc_mcp_skill_rest::{
    CallOutcome, ServiceError, ServiceErrorKind, ToolInvoker, ToolSlug,
    dispatch_error_to_service_error,
};
use serde_json::Value;

use crate::executor::DccExecutorHandle;
use crate::rmcp_tool_call_dispatch::dispatch_action_with_thread_routing;

/// Invoke backend actions with main-thread routing when a host executor is wired.
pub struct ThreadRoutedInvoker {
    dispatcher: Arc<ToolDispatcher>,
    executor: DccExecutorHandle,
}

impl ThreadRoutedInvoker {
    #[must_use]
    pub fn new(dispatcher: Arc<ToolDispatcher>, executor: DccExecutorHandle) -> Self {
        Self {
            dispatcher,
            executor,
        }
    }
}

impl ToolInvoker for ThreadRoutedInvoker {
    fn invoke(&self, action_name: &str, params: Value) -> Result<CallOutcome, ServiceError> {
        let meta = self
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
        let affinity = meta.thread_affinity;
        let enforce = meta.enforce_thread_affinity;

        // `SkillRestService::call` is synchronous and runs on the dedicated
        // HTTP thread's `current_thread` Tokio runtime. We cannot
        // `Handle::block_on` there (deadlock / panic). A nested
        // `current_thread` runtime on a helper OS thread can `.await` the same
        // `dispatch_action_with_thread_routing` path MCP `tools/call` uses.
        let dispatch_result = std::thread::scope(|scope| {
            let join = scope.spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| {
                        ServiceError::new(
                            ServiceErrorKind::Internal,
                            format!("thread-routed REST runtime: {e}"),
                        )
                    })?;
                rt.block_on(dispatch_action_with_thread_routing(
                    dispatcher.as_ref().clone(),
                    Some(&executor),
                    &action,
                    params,
                    affinity,
                    enforce,
                ))
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
