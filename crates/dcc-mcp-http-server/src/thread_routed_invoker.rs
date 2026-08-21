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
    CallOutcome, InvocationCancellation, ServiceError, ServiceErrorKind, ToolInvoker, ToolSlug,
    dispatch_error_to_service_error,
};
use serde_json::Value;

use crate::executor::DccExecutorHandle;
use crate::rmcp_tool_call_dispatch::{
    ThreadRoutingDispatch, dispatch_action_with_thread_routing,
    dispatch_action_with_thread_routing_cancellable,
};

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

    async fn invoke_inner(
        &self,
        action_name: &str,
        params: Value,
        meta: Option<Value>,
        cancellation: Option<InvocationCancellation>,
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

        let request = ThreadRoutingDispatch {
            dispatcher: dispatcher.as_ref().clone(),
            executor: Some(&executor),
            resolved_name: &action,
            call_params: params,
            meta,
            thread_affinity: affinity,
            enforce_thread_affinity: enforce,
            standalone_main_thread_execution: false,
        };
        let dispatch_result = match cancellation {
            Some(cancellation) => {
                dispatch_action_with_thread_routing_cancellable(request, cancellation).await
            }
            None => dispatch_action_with_thread_routing(request).await,
        }
        .map_err(dispatch_error_to_service_error)?;

        Ok(CallOutcome {
            slug: ToolSlug(action_name.to_string()),
            output: dispatch_result.output,
            validation_skipped: dispatch_result.validation_skipped,
        })
    }
}

#[async_trait::async_trait]
impl ToolInvoker for ThreadRoutedInvoker {
    async fn invoke(
        &self,
        action_name: &str,
        params: Value,
        meta: Option<Value>,
    ) -> Result<CallOutcome, ServiceError> {
        self.invoke_inner(action_name, params, meta, None).await
    }

    async fn invoke_with_cancellation(
        &self,
        action_name: &str,
        params: Value,
        meta: Option<Value>,
        cancellation: InvocationCancellation,
    ) -> Result<CallOutcome, ServiceError> {
        self.invoke_inner(action_name, params, meta, Some(cancellation))
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use dcc_mcp_actions::ToolDispatcher;
    use dcc_mcp_actions::registry::{ToolMeta, ToolRegistry};
    use dcc_mcp_host::{DccDispatcher, QueueDispatcher};
    use dcc_mcp_job::job::{JobManager, JobStatus};
    use dcc_mcp_models::ThreadAffinity;
    use dcc_mcp_skill_rest::{ToolInvoker, ToolSlug};
    use serde_json::json;
    use tokio::runtime::Handle;

    use super::*;
    use crate::executor::DeferredExecutor;
    use crate::host_bridge::dispatcher_to_executor_handle;
    use crate::job_aware_invoker::JobAwareInvoker;

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

        let invoker = ThreadRoutedInvoker::new(dispatcher, executor);
        let outcome = invoker
            .invoke("thread_probe", json!({}), None)
            .await
            .expect("main-thread REST invoke");
        assert_eq!(outcome.output["ok"], true);
        ticker.join().unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rest_async_cancel_before_pump_skips_main_thread_handler() {
        let registry = Arc::new(ToolRegistry::new());
        registry.register_action(ToolMeta {
            name: "cancel_probe".into(),
            dcc: "houdini".into(),
            description: "cancel before host pump".into(),
            thread_affinity: ThreadAffinity::Main,
            enforce_thread_affinity: true,
            enabled: true,
            ..Default::default()
        });
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let ran = Arc::new(AtomicBool::new(false));
        dispatcher.register_handler("cancel_probe", {
            let ran = Arc::clone(&ran);
            move |_| {
                ran.store(true, Ordering::Release);
                Ok(json!({"ok": true}))
            }
        });

        let mut deferred = DeferredExecutor::new(8);
        let jobs = Arc::new(JobManager::new());
        let routed = ThreadRoutedInvoker::new(dispatcher, deferred.handle());
        let invoker = JobAwareInvoker::new(Arc::new(routed), Arc::clone(&jobs));
        let pending = invoker
            .invoke_async(
                &ToolSlug("houdini.test.cancel_probe".into()),
                "cancel_probe",
                json!({}),
                None,
            )
            .expect("queue REST job")
            .expect("job-aware invoker");

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let status = jobs.get(&pending.job_id).expect("job").read().status;
                if status == JobStatus::Running && deferred.handle().pending() > 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("job reaches the host queue");

        jobs.cancel(&pending.job_id).expect("cancel running job");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                // `DccExecutorHandle::pending` is intentionally an approximate
                // observability counter: submit records the enqueue before it
                // hands the task to the mpsc channel. Mirror a real DCC event
                // loop by pumping until the execution lane acknowledges the
                // cancellation instead of assuming one poll is sufficient.
                let _ = deferred.poll_pending();
                assert!(!ran.load(Ordering::Acquire));
                if jobs.get(&pending.job_id).expect("job").read().status == JobStatus::Cancelled {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("job remains cancelled");
        assert!(!ran.load(Ordering::Acquire));
    }
}
