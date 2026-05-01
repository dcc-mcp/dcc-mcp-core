//! Data structures for [`crate::job`] — the in-process job tracker.
//!
//! Carries `JobStatus`, `JobProgress`, `Job`, and the `JobEvent` /
//! `JobSubscriber` transport types used by the notification layer.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Event emitted whenever a job transitions between [`JobStatus`] values.
///
/// Subscribers receive a snapshot of the job **after** the transition.
/// The transport layer ([`crate::notifications::JobNotifier`]) converts
/// these events into MCP `notifications/progress` and the
/// `notifications/$/dcc.jobUpdated` vendor-extension channel (#326).
#[derive(Debug, Clone)]
pub struct JobEvent {
    /// Job id.
    pub id: String,
    /// Fully-qualified tool name (e.g. `scene.get_info`).
    pub tool_name: String,
    /// New status after the transition.
    pub status: JobStatus,
    /// Last-known progress (may be `None` for `Pending`).
    pub progress: Option<JobProgress>,
    /// Error message attached when `status == Failed`.
    pub error: Option<String>,
    /// Wall-clock time of the transition.
    pub updated_at: DateTime<Utc>,
    /// Wall-clock time when the job was created.
    pub created_at: DateTime<Utc>,
}

/// Boxed subscriber callback. See [`crate::job::JobManager::subscribe`].
pub type JobSubscriber = Arc<dyn Fn(JobEvent) + Send + Sync + 'static>;

/// Lifecycle state of a tracked [`Job`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Created but not yet picked up for execution.
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
    /// Cancelled by a client or supervisor.
    Cancelled,
    /// Reserved for crash / restart recovery (issue #328).
    Interrupted,
}

impl JobStatus {
    /// Returns `true` if the job is in a terminal state.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobStatus::Completed
                | JobStatus::Failed
                | JobStatus::Cancelled
                | JobStatus::Interrupted
        )
    }
}

/// Coarse progress indicator emitted by the tool handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgress {
    pub current: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A tracked async tool invocation.
#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: String,
    pub tool_name: String,
    pub status: JobStatus,
    /// Parent job id (issue #318 — workflow nesting / cascading cancel).
    ///
    /// When set, this job is a child of another tracked job. The child's
    /// `cancel_token` is derived via [`CancellationToken::child_token`] from
    /// the parent's, so cancelling the parent cancels every descendant
    /// within one cooperative checkpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<JobProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Cooperative cancellation signal for the running tool.
    ///
    /// Not serialised — clients observe cancellation through `status`.
    /// For child jobs this is a child token of the parent's, so parent
    /// cancellation propagates automatically.
    #[serde(skip)]
    pub cancel_token: CancellationToken,
}

impl Job {
    pub(crate) fn new(
        tool_name: String,
        parent_job_id: Option<String>,
        cancel_token: CancellationToken,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            tool_name,
            status: JobStatus::Pending,
            parent_job_id,
            progress: None,
            result: None,
            error: None,
            created_at: now,
            updated_at: now,
            cancel_token,
        }
    }

    /// JSON status snapshot used by `jobs.get_status` (#319) and the async
    /// dispatch envelope returned by `tools/call` (#318).
    pub fn to_status_json(&self) -> serde_json::Value {
        serde_json::json!({
            "job_id": self.id,
            "tool_name": self.tool_name,
            "status": self.status,
            "parent_job_id": self.parent_job_id,
            "progress": self.progress,
            "result": self.result,
            "error": self.error,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        })
    }
}
