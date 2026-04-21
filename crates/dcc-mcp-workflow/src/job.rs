//! [`WorkflowJob`] — runtime tracker for an in-flight or finished workflow.
//!
//! This is a **placeholder** in the skeleton PR: the signatures are final so
//! downstream issues (#349 / #353) can build against them, but step
//! execution itself returns [`WorkflowError::NotImplemented`].

use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;
use crate::spec::{StepId, WorkflowId, WorkflowSpec, WorkflowStatus};

/// Aggregated progress counters for a workflow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowProgress {
    /// Number of steps that finished successfully.
    pub completed_steps: u32,
    /// Total number of steps in the workflow (shallow count of top-level +
    /// children).
    pub total_steps: u32,
}

/// Runtime record of a workflow execution.
///
/// The placeholder retains the final field shape so serialisation contracts
/// won't change between this PR and the execution PR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowJob {
    /// Runtime id (distinct from the spec's declared `name`).
    pub id: WorkflowId,
    /// The spec being executed.
    pub spec: WorkflowSpec,
    /// Aggregated status.
    pub status: WorkflowStatus,
    /// Id of the step currently executing, if any.
    pub current_step_id: Option<StepId>,
    /// Wall-clock start (unix seconds), if the job has started.
    pub started_at: Option<u64>,
    /// Wall-clock completion (unix seconds), if terminal.
    pub completed_at: Option<u64>,
    /// Progress counters.
    pub progress: WorkflowProgress,
}

impl WorkflowJob {
    /// Construct a `Pending` job around a spec. Does not run anything.
    #[must_use]
    pub fn pending(spec: WorkflowSpec) -> Self {
        let total = count_steps(&spec);
        Self {
            id: WorkflowId::new(),
            spec,
            status: WorkflowStatus::Pending,
            current_step_id: None,
            started_at: None,
            completed_at: None,
            progress: WorkflowProgress {
                completed_steps: 0,
                total_steps: total,
            },
        }
    }

    /// Kick off execution.
    ///
    /// **Skeleton**: always returns
    /// [`WorkflowError::NotImplemented`]. The signature is final so #349
    /// can build against it. See issue #348.
    ///
    /// # Errors
    ///
    /// Always returns [`WorkflowError::NotImplemented`] in the skeleton.
    pub fn start(&mut self) -> Result<(), WorkflowError> {
        Err(WorkflowError::NotImplemented(
            "step execution pending follow-up PR",
        ))
    }
}

fn count_steps(spec: &WorkflowSpec) -> u32 {
    fn count(steps: &[crate::spec::Step]) -> u32 {
        steps
            .iter()
            .map(|s| {
                1 + match &s.kind {
                    crate::spec::StepKind::Foreach { steps, .. }
                    | crate::spec::StepKind::Parallel { steps } => count(steps),
                    crate::spec::StepKind::Branch {
                        then, else_steps, ..
                    } => count(then) + count(else_steps),
                    _ => 0,
                }
            })
            .sum()
    }
    count(&spec.steps)
}
