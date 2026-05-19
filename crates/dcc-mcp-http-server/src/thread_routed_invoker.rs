//! REST `/v1/call` invoker that honours `thread_affinity` like MCP `tools/call`.
//!
//! [`dcc_mcp_skill_rest::DispatcherInvoker`] calls [`ToolDispatcher::dispatch`]
//! directly on a Tokio worker, which trips `THREAD_AFFINITY_VIOLATION` for
//! `affinity: main` tools when `enforce_thread_affinity` is enabled. MCP
//! `tools/call` already routes through [`crate::rmcp_tool_call_dispatch`];
//! this invoker applies the same routing for the REST surface.

use std::sync::Arc;

use dcc_mcp_actions::ToolDispatcher;
use dcc_mcp_skill_rest::{CallOutcome, ServiceError, ServiceErrorKind, ToolInvoker, ToolSlug};
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

        let handle = tokio::runtime::Handle::try_current().map_err(|_| {
            ServiceError::new(
                ServiceErrorKind::Internal,
                "thread-routed REST invoke requires a Tokio runtime",
            )
        })?;

        let dispatcher = Arc::clone(&self.dispatcher);
        let executor = self.executor.clone();
        let action = action_name.to_string();
        let affinity = meta.thread_affinity;
        let enforce = meta.enforce_thread_affinity;

        let output = handle
            .block_on(async move {
                dispatch_action_with_thread_routing(
                    dispatcher.as_ref().clone(),
                    Some(&executor),
                    &action,
                    params,
                    affinity,
                    enforce,
                )
                .await
            })
            .map_err(|err| dispatch_err_to_service_error(&err))?;

        Ok(CallOutcome {
            slug: ToolSlug(action_name.to_string()),
            output,
            validation_skipped: false,
        })
    }
}

fn dispatch_err_to_service_error(err: &str) -> ServiceError {
    if err.starts_with("THREAD_AFFINITY_UNAVAILABLE:") {
        return ServiceError::new(ServiceErrorKind::ThreadAffinityViolation, err.to_string())
            .with_hint("attach a host QueueDispatcher/BlockingDispatcher before start()");
    }
    if err.starts_with("THREAD_AFFINITY_VIOLATION:") {
        return ServiceError::new(ServiceErrorKind::ThreadAffinityViolation, err.to_string())
            .with_hint(
                "check the action tools.yaml thread_affinity, or marshal through the host main-thread dispatcher",
            );
    }
    if err == "CANCELLED" {
        return ServiceError::new(ServiceErrorKind::BackendError, err.to_string());
    }
    ServiceError::new(ServiceErrorKind::BackendError, err.to_string())
}
