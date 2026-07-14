//! Job-aware decorator for the REST [`ToolInvoker`] port.
//!
//! The decorator keeps host-specific invocation in the wrapped invoker while
//! owning only the background job lifecycle. This lets direct dispatch and
//! main-thread-routed adapters share the same async REST contract.

use std::sync::Arc;

use dcc_mcp_job::job::JobManager;
use dcc_mcp_skill_rest::{CallOutcome, PendingCall, ServiceError, ToolInvoker, ToolSlug};
use serde_json::Value;

/// Adds JobManager-backed asynchronous dispatch to any [`ToolInvoker`].
pub struct JobAwareInvoker {
    inner: Arc<dyn ToolInvoker>,
    jobs: Arc<JobManager>,
}

impl JobAwareInvoker {
    #[must_use]
    pub fn new(inner: Arc<dyn ToolInvoker>, jobs: Arc<JobManager>) -> Self {
        Self { inner, jobs }
    }
}

impl ToolInvoker for JobAwareInvoker {
    fn invoke(
        &self,
        action_name: &str,
        params: Value,
        meta: Option<Value>,
    ) -> Result<CallOutcome, ServiceError> {
        self.inner.invoke(action_name, params, meta)
    }

    fn invoke_async(
        &self,
        tool_slug: &ToolSlug,
        action_name: &str,
        params: Value,
        meta: Option<Value>,
    ) -> Result<Option<PendingCall>, ServiceError> {
        let parent_job_id = meta
            .as_ref()
            .and_then(|value| value.get("dcc"))
            .and_then(|dcc| dcc.get("parentJobId").or_else(|| dcc.get("parent_job_id")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let handle = self
            .jobs
            .create_with_parent(tool_slug.as_str(), parent_job_id.clone());
        let (job_id, cancel_token) = {
            let job = handle.read();
            (job.id.clone(), job.cancel_token.clone())
        };

        let jobs = Arc::clone(&self.jobs);
        let invoker = Arc::clone(&self.inner);
        let action_name = action_name.to_string();
        let spawned_job_id = job_id.clone();
        tokio::task::spawn_blocking(move || {
            if cancel_token.is_cancelled() || jobs.start(&spawned_job_id).is_none() {
                return;
            }
            match invoker.invoke(&action_name, params, meta) {
                Ok(outcome) => {
                    let _ = jobs.complete(&spawned_job_id, outcome.output);
                }
                Err(error) => {
                    let _ = jobs.fail(&spawned_job_id, error.message);
                }
            }
        });

        Ok(Some(PendingCall::new(job_id, parent_job_id)))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dcc_mcp_job::job::JobStatus;
    use serde_json::json;

    use super::*;

    struct EchoInvoker;

    impl ToolInvoker for EchoInvoker {
        fn invoke(
            &self,
            action_name: &str,
            params: Value,
            _meta: Option<Value>,
        ) -> Result<CallOutcome, ServiceError> {
            Ok(CallOutcome {
                slug: ToolSlug(action_name.to_string()),
                output: params,
                validation_skipped: false,
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queued_invocation_completes_in_shared_job_manager() {
        let jobs = Arc::new(JobManager::new());
        let invoker = JobAwareInvoker::new(Arc::new(EchoInvoker), Arc::clone(&jobs));
        let pending = invoker
            .invoke_async(
                &ToolSlug("nuke.comp.render".into()),
                "render",
                json!({"frame": 1}),
                None,
            )
            .expect("queue call")
            .expect("job-aware invoker");

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let status = jobs.get(&pending.job_id).expect("job").read().status;
                if status.is_terminal() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("job completes");

        let job = jobs.get(&pending.job_id).expect("job");
        let job = job.read();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.result, Some(json!({"frame": 1})));
    }
}
