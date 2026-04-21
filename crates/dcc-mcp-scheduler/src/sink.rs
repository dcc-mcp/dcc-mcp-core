//! Trait that receives fired schedules and hands them off to a real
//! workflow dispatcher.
//!
//! The scheduler crate is deliberately agnostic about how workflows are
//! executed: it only decides **when** to fire. Downstream callers wire a
//! [`JobSink`] that resolves the workflow name against their
//! `WorkflowCatalog` and enqueues a `WorkflowJob`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Origin of a fire event, passed to the [`JobSink`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerKind {
    /// Fired by a cron next-fire-time calculation.
    Cron,
    /// Fired by an HTTP POST to a registered webhook path.
    Webhook,
}

/// A single fire event.
///
/// Produced by [`SchedulerService`](crate::SchedulerService) and consumed by
/// [`JobSink::enqueue`]. The `inputs` field has already had any
/// `{{trigger.payload.<jsonpath>}}` placeholders rendered against
/// `payload`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerFire {
    /// Origin of the fire.
    pub kind: TriggerKind,
    /// Id of the [`ScheduleSpec`](crate::ScheduleSpec) that fired.
    pub schedule_id: String,
    /// Workflow name the schedule is bound to.
    pub workflow: String,
    /// Rendered workflow inputs.
    pub inputs: serde_json::Value,
    /// Raw webhook payload (empty JSON object for cron fires).
    pub payload: serde_json::Value,
    /// UNIX seconds at fire time.
    pub fired_at: u64,
}

/// Callback trait implemented by the host to actually enqueue a workflow.
///
/// Implementations are expected to:
/// 1. Resolve `fire.workflow` against their `WorkflowCatalog`.
/// 2. Build a `WorkflowJob` (skeleton in `dcc_mcp_workflow`) from
///    `fire.inputs`.
/// 3. Submit it to their dispatch path (once #348 execution lands).
///
/// The scheduler does not retry on sink failure — it logs the error and
/// moves on. Idempotency / retry is the caller's responsibility, typically
/// via `StepPolicy::retry` at the workflow level.
pub trait JobSink: Send + Sync + 'static {
    /// Enqueue one fired trigger. Return `Err` only if the host is in an
    /// unrecoverable state; transient failures should be absorbed inside
    /// the implementation.
    fn enqueue(&self, fire: TriggerFire) -> Result<(), String>;
}

/// Type-erased handle to a [`JobSink`], used internally by the scheduler.
pub type SharedJobSink = Arc<dyn JobSink>;

/// Sink that records every fire in a `Vec` — useful for tests and demos.
///
/// Not intended for production: memory grows without bound.
#[derive(Debug, Default)]
pub struct RecordingSink {
    fires: parking_lot::Mutex<Vec<TriggerFire>>,
}

impl RecordingSink {
    /// New empty recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot every fire seen so far.
    #[must_use]
    pub fn fires(&self) -> Vec<TriggerFire> {
        self.fires.lock().clone()
    }

    /// Number of fires recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fires.lock().len()
    }

    /// `true` when no fires have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fires.lock().is_empty()
    }
}

impl JobSink for RecordingSink {
    fn enqueue(&self, fire: TriggerFire) -> Result<(), String> {
        self.fires.lock().push(fire);
        Ok(())
    }
}
