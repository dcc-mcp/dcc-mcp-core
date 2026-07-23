//! Tick-driven, main-affinity job execution.

use std::collections::VecDeque;
use std::sync::Arc;

use serde_json::Value;

use super::{JobManager, JobProgress, JobStatus};

/// Result of one bounded host-main step.
pub struct ChunkedStepOutput {
    pub value: Value,
    pub message: Option<String>,
}

impl ChunkedStepOutput {
    #[must_use]
    pub fn new(value: Value, message: Option<String>) -> Self {
        Self { value, message }
    }
}

/// One bounded unit of host-main work.
pub type ChunkedStep = Box<dyn FnOnce() -> Result<ChunkedStepOutput, String> + Send + 'static>;

/// Runs at most one bounded step per host event-loop tick.
///
/// The host owns scheduling and calls [`Self::tick`] once from each event-loop
/// callback. Lifecycle, progress, failure, and acknowledged cancellation are
/// written directly to the shared [`JobManager`].
pub struct ChunkedJobRunner {
    jobs: Arc<JobManager>,
    job_id: String,
    steps: VecDeque<ChunkedStep>,
    results: Vec<Value>,
    total: u64,
}

impl ChunkedJobRunner {
    #[must_use]
    pub fn new(
        jobs: Arc<JobManager>,
        tool_name: impl Into<String>,
        steps: impl IntoIterator<Item = ChunkedStep>,
    ) -> Self {
        let steps: VecDeque<_> = steps.into_iter().collect();
        let total = steps.len() as u64;
        let job_id = jobs.create(tool_name).read().id.clone();
        Self {
            jobs,
            job_id,
            steps,
            results: Vec::with_capacity(total as usize),
            total,
        }
    }

    #[must_use]
    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    /// Execute one step. Returns `true` only while another tick is needed.
    pub fn tick(&mut self) -> bool {
        let Some(handle) = self.jobs.get(&self.job_id) else {
            return false;
        };
        let (status, cancelled) = {
            let job = handle.read();
            (job.status, job.cancel_token.is_cancelled())
        };
        if status.is_terminal() {
            return false;
        }
        if cancelled {
            let _ = self.jobs.acknowledge_cancel(&self.job_id);
            return false;
        }
        if status == JobStatus::Pending && self.jobs.start(&self.job_id).is_none() {
            return false;
        }

        let Some(step) = self.steps.pop_front() else {
            let _ = self.jobs.complete(
                &self.job_id,
                Value::Array(std::mem::take(&mut self.results)),
            );
            return false;
        };

        match step() {
            Ok(output) => {
                self.results.push(output.value);
                let current = self.results.len() as u64;
                let _ = self.jobs.update_progress(
                    &self.job_id,
                    JobProgress {
                        current,
                        total: self.total,
                        message: output.message,
                    },
                );
                if handle.read().cancel_token.is_cancelled() {
                    let _ = self.jobs.acknowledge_cancel(&self.job_id);
                    return false;
                }
                if self.steps.is_empty() {
                    let _ = self.jobs.complete(
                        &self.job_id,
                        Value::Array(std::mem::take(&mut self.results)),
                    );
                    false
                } else {
                    true
                }
            }
            Err(error) => {
                let _ = self.jobs.fail(&self.job_id, error);
                false
            }
        }
    }

    /// Request cancellation; the next tick acknowledges it.
    pub fn cancel(&self) -> bool {
        self.jobs.cancel(&self.job_id).is_some()
    }
}
