//! Workflow notifier abstraction — decouples the executor from the HTTP
//! crate's SSE notification plumbing.
//!
//! The executor depends only on this trait. `dcc-mcp-http`'s
//! `JobNotifier` implements [`WorkflowNotifier`] so that when the two crates
//! are wired together every executor transition surfaces on
//! `notifications/$/dcc.workflowUpdated`.

use std::sync::Arc;

use serde::Serialize;
use uuid::Uuid;

use crate::spec::WorkflowStatus;

/// Progress counters published alongside a [`WorkflowUpdate`].
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct WorkflowUpdateProgress {
    /// Number of steps that finished successfully so far.
    pub completed_steps: u32,
    /// Total step count in the workflow (top-level + children).
    pub total_steps: u32,
}

/// Workflow-level state transition event.
///
/// Fired on: step enter, step terminal, workflow terminal, approval request,
/// approval response.
#[derive(Debug, Clone)]
pub struct WorkflowUpdate {
    /// Workflow UUID (the runtime id, not the spec name).
    pub workflow_id: Uuid,
    /// Outer job UUID wrapping execution.
    pub job_id: Uuid,
    /// Aggregated status after the transition.
    pub status: WorkflowStatus,
    /// Step id whose transition triggered this update, if applicable.
    pub current_step_id: Option<String>,
    /// Progress counters.
    pub progress: WorkflowUpdateProgress,
    /// Free-form detail payload (e.g. `{"kind": "approve_requested", "prompt": "..."}`).
    pub detail: serde_json::Value,
}

/// Abstraction over the HTTP crate's SSE push path.
pub trait WorkflowNotifier: Send + Sync {
    /// Emit a workflow update (fires a `$/dcc.workflowUpdated` SSE frame on
    /// every subscribed session).
    fn emit(&self, update: WorkflowUpdate);
}

/// No-op notifier — used when the executor runs outside an MCP server (e.g.
/// in unit tests).
#[derive(Debug, Default, Clone)]
pub struct NullNotifier;

impl WorkflowNotifier for NullNotifier {
    fn emit(&self, _update: WorkflowUpdate) {}
}

/// Shared, thread-safe notifier alias.
pub type SharedNotifier = Arc<dyn WorkflowNotifier>;

/// Recording notifier that stores every emission. Useful for tests.
#[derive(Debug, Default)]
pub struct RecordingNotifier {
    events: parking_lot::RwLock<Vec<WorkflowUpdate>>,
}

impl RecordingNotifier {
    /// New empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of every event received so far.
    pub fn events(&self) -> Vec<WorkflowUpdate> {
        self.events.read().clone()
    }

    /// Count of events received so far.
    pub fn len(&self) -> usize {
        self.events.read().len()
    }

    /// Whether no events have been received yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl WorkflowNotifier for RecordingNotifier {
    fn emit(&self, update: WorkflowUpdate) {
        self.events.write().push(update);
    }
}
