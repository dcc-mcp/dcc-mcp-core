//! `WorkflowExecutor::resume` — drive a previously-persisted workflow
//! forward from the first non-completed step (issue #565).
//!
//! Resume is only available when the executor was built with
//! `WorkflowExecutorBuilder::storage(...)` and the
//! `job-persist-sqlite` Cargo feature is on. The persisted shape it
//! consumes lives in `crate::sqlite::ResumeSnapshot`.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use uuid::Uuid;

use super::*;
use crate::context::{StepOutput, WorkflowContext};
use crate::error::WorkflowResumeError;
use crate::spec::{StepId, WorkflowSpec, WorkflowStatus};
use crate::sqlite::{ResumeSnapshot, compute_spec_hash};

/// Caller-provided knobs for [`WorkflowExecutor::resume`].
#[derive(Debug, Default, Clone)]
pub struct ResumeOptions {
    /// Step ids to re-run even if their previous attempt is recorded
    /// as `completed` in storage. Lets operators force a re-export
    /// after a downstream pipeline correction.
    pub force_steps: Vec<String>,
    /// Caller-asserted spec hash (sha256 hex of the canonical spec
    /// JSON). When `strict` is true and this differs from the hash of
    /// the persisted spec, resume refuses with
    /// [`WorkflowResumeError::SpecChanged`].
    pub expected_spec_hash: Option<String>,
    /// When `true`, refuse to resume if `expected_spec_hash` does not
    /// match the persisted spec. When `false` (default), the persisted
    /// spec is always treated as the source of truth and any caller
    /// hash mismatch is logged at WARN level only.
    pub strict: bool,
}

impl WorkflowExecutor {
    /// Resume a previously-persisted workflow run from storage.
    ///
    /// Reads the persisted spec + inputs + per-step status, hydrates
    /// the executor context with every completed step's recorded
    /// output, marks those step ids as preloaded, then drives the spec
    /// to completion. Steps already recorded `completed` are skipped at
    /// the dispatcher (a single `step_skipped_resume` event is emitted
    /// for observers); steps that were `running` or `interrupted` re-run
    /// from the start.
    ///
    /// Returns [`WorkflowResumeError::NoStorage`] if the executor was
    /// built without `WorkflowStorage`.
    pub fn resume(
        &self,
        workflow_id: Uuid,
        opts: ResumeOptions,
    ) -> Result<WorkflowRunHandle, WorkflowResumeError> {
        let storage = self
            .storage
            .as_ref()
            .ok_or(WorkflowResumeError::NoStorage)?;
        let snap = storage
            .load_resume_snapshot(workflow_id)?
            .ok_or(WorkflowResumeError::NotFound(workflow_id))?;
        ensure_resumable(workflow_id, &snap, !opts.force_steps.is_empty())?;
        let spec = decode_spec(workflow_id, &snap)?;
        spec.validate()
            .map_err(|e| WorkflowResumeError::Validation(e, workflow_id))?;
        check_spec_hash(workflow_id, &spec, &opts)?;

        let inputs: Value = serde_json::from_str(&snap.inputs_json).unwrap_or(Value::Null);
        let force: HashSet<String> = opts.force_steps.iter().cloned().collect();
        let preloaded: HashSet<String> = snap
            .completed_steps
            .iter()
            .map(|(id, _)| id.clone())
            .filter(|id| !force.contains(id))
            .collect();

        // Reset the row's status before driving forward — this also
        // clears any previous error_msg so observers don't see a stale
        // failure marker overlapping with the new run.
        if let Err(e) = storage.reset_for_resume(workflow_id) {
            warn!(error = %e, "reset_for_resume failed; continuing");
        }

        // Build context with completed step outputs already in place.
        let context = WorkflowContext::new(inputs.clone());
        for (id, output) in &snap.completed_steps {
            if force.contains(id) {
                continue;
            }
            let step_id = StepId::from(id.clone());
            context.record_step(&step_id, StepOutput::from_value(output.clone()));
        }

        let cancel_token = CancellationToken::new();
        let total_steps = count_steps(&spec);
        let root_job_id = Uuid::new_v4();
        let state = RunState {
            workflow_id,
            root_job_id,
            context,
            notifier: Arc::clone(&self.notifier),
            artefacts: self.artefacts.clone(),
            tool_caller: Arc::clone(&self.tool_caller),
            remote_caller: Arc::clone(&self.remote_caller),
            idempotency: Arc::clone(&self.idempotency),
            approval_gate: self.approval_gate.clone(),
            cancel_token: cancel_token.clone(),
            storage: self.storage.clone(),
            total_steps,
            completed: Arc::new(parking_lot::Mutex::new(0)),
            outputs_snapshot: Arc::new(parking_lot::RwLock::new(
                snap.completed_steps.iter().cloned().collect(),
            )),
            preloaded_steps: Arc::new(preloaded),
        };

        state.emit(
            WorkflowStatus::Pending,
            None,
            serde_json::json!({
                "kind": "workflow_resumed",
                "workflow_id": workflow_id.to_string(),
                "preloaded_count": state.preloaded_steps.len(),
                "force_steps": opts.force_steps,
            }),
        );

        let default_approve_timeout = self.default_approve_timeout;
        let spec_clone = spec.clone();
        let state_clone = state.clone();
        let join = tokio::spawn(async move {
            let run_state = state_clone;
            let terminal = Self::drive(
                run_state.clone(),
                spec_clone.steps.clone(),
                default_approve_timeout,
            )
            .await;
            if let Some(storage) = &run_state.storage {
                if let Err(e) =
                    storage.update_workflow_status(run_state.workflow_id, terminal, None)
                {
                    warn!(error = %e, "workflow storage status update failed");
                }
                if let Err(e) =
                    storage.update_step_outputs(run_state.workflow_id, &run_state.outputs_json())
                {
                    warn!(error = %e, "workflow storage step outputs update failed");
                }
            }
            run_state.emit(
                terminal,
                None,
                serde_json::json!({"kind": "workflow_terminal"}),
            );
            terminal
        });

        Ok(WorkflowRunHandle {
            workflow_id,
            root_job_id,
            cancel_token,
            join,
        })
    }
}

fn ensure_resumable(
    id: Uuid,
    snap: &ResumeSnapshot,
    has_force_steps: bool,
) -> Result<(), WorkflowResumeError> {
    match snap.status {
        WorkflowStatus::Failed | WorkflowStatus::Interrupted | WorkflowStatus::Pending => Ok(()),
        // Completed workflows can be resumed only when the caller asks
        // to re-run specific steps (operator-driven "redo this export"
        // scenario). A bare `resume` on a Completed workflow is a
        // no-op and gets rejected to surface the misuse loudly.
        WorkflowStatus::Completed if has_force_steps => Ok(()),
        other => Err(WorkflowResumeError::NotResumable {
            workflow_id: id,
            status: other.as_str().to_string(),
        }),
    }
}

fn decode_spec(id: Uuid, snap: &ResumeSnapshot) -> Result<WorkflowSpec, WorkflowResumeError> {
    serde_json::from_str(&snap.spec_json).map_err(|e| WorkflowResumeError::CorruptSpec {
        workflow_id: id,
        reason: e.to_string(),
    })
}

fn check_spec_hash(
    id: Uuid,
    spec: &WorkflowSpec,
    opts: &ResumeOptions,
) -> Result<(), WorkflowResumeError> {
    let Some(expected) = opts.expected_spec_hash.as_ref() else {
        return Ok(());
    };
    let actual = compute_spec_hash(spec);
    if &actual == expected {
        return Ok(());
    }
    if opts.strict {
        return Err(WorkflowResumeError::SpecChanged {
            workflow_id: id,
            expected: expected.clone(),
            actual,
        });
    }
    warn!(
        workflow_id = %id,
        expected = %expected,
        actual = %actual,
        "expected_spec_hash mismatch in non-strict mode; using persisted spec"
    );
    Ok(())
}
