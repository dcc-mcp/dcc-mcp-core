use super::*;

/// Handle returned by [`WorkflowExecutor::run`].
#[derive(Debug)]
pub struct WorkflowRunHandle {
    /// Runtime workflow id (matches the `workflow_id` field in
    /// `$/dcc.workflowUpdated`).
    pub workflow_id: Uuid,
    /// Root job id — stable across step transitions. Parents can be linked
    /// via the `parent_job_id` argument to
    /// [`WorkflowExecutor::run`].
    pub root_job_id: Uuid,
    /// Shared cancellation token. Cancel to abort the workflow; children
    /// inherit child tokens so the abort cascades within one cooperative
    /// checkpoint.
    pub cancel_token: CancellationToken,
    /// Join handle on the driver task. Resolves with the terminal status.
    pub join: JoinHandle<WorkflowStatus>,
}

impl WorkflowRunHandle {
    /// Cancel the workflow. No-op if already terminal.
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Wait for the workflow to reach a terminal state.
    pub async fn wait(self) -> WorkflowStatus {
        self.join.await.unwrap_or(WorkflowStatus::Failed)
    }
}
