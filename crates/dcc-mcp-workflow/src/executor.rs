//! [`WorkflowExecutor`] — runs a [`WorkflowSpec`] end-to-end.
//!
//! This is the step execution engine for issue #348. It consumes the
//! skeleton types landed in the parent PR plus every subsequent workflow
//! improvement (`StepPolicy`, `FileRef`, `$/dcc.workflowUpdated`,
//! parent-job cascade, SQLite persistence) and implements all six
//! [`StepKind`] variants.
//!
//! # Shape
//!
//! ```text
//! WorkflowExecutor::run(spec, inputs, parent)
//!   → creates a root WorkflowRun (id, cancel_token)
//!   → spawns a Tokio task driving the top-level step sequence
//!   → returns a WorkflowRunHandle { workflow_id, root_job_id, cancel_token, join }
//! ```
//!
//! Each step kind has its own driver:
//!
//! | Kind           | Driver                                               |
//! |----------------|------------------------------------------------------|
//! | `Tool`         | [`WorkflowExecutor::run_tool_step`] via [`ToolCaller`] |
//! | `ToolRemote`   | [`WorkflowExecutor::run_remote_step`] via [`RemoteCaller`] |
//! | `Foreach`      | [`WorkflowExecutor::run_foreach`] (JSONPath items)   |
//! | `Parallel`     | [`WorkflowExecutor::run_parallel`] (all-or-abort)    |
//! | `Approve`      | [`WorkflowExecutor::run_approve`] (gate + timeout)   |
//! | `Branch`       | [`WorkflowExecutor::run_branch`] (JSONPath condition) |
//!
//! Each step honours `StepPolicy` (timeout + retry + idempotency).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_artefact::{ArtefactBody, FileRef, SharedArtefactStore};
use jsonpath_rust::JsonPath;
use serde_json::Value;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::approval::{ApprovalGate, ApprovalResponse};
use crate::callers::{
    ActionDispatcherCaller, NullRemoteCaller, SharedRemoteCaller, SharedToolCaller,
};
use crate::context::{StepOutput, WorkflowContext};
use crate::error::WorkflowError;
use crate::idempotency::IdempotencyCache;
use crate::notifier::{NullNotifier, SharedNotifier, WorkflowUpdate, WorkflowUpdateProgress};
use crate::policy::{BackoffKind, IdempotencyScope, RetryPolicy, StepPolicy};
use crate::spec::{Step, StepId, StepKind, WorkflowSpec, WorkflowStatus};

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

/// Shared state for an active workflow — cloned into every step driver.
#[derive(Clone)]
struct RunState {
    workflow_id: Uuid,
    root_job_id: Uuid,
    context: WorkflowContext,
    notifier: SharedNotifier,
    artefacts: Option<SharedArtefactStore>,
    tool_caller: SharedToolCaller,
    remote_caller: SharedRemoteCaller,
    idempotency: IdempotencyCache,
    approval_gate: ApprovalGate,
    cancel_token: CancellationToken,
    #[cfg(feature = "job-persist-sqlite")]
    storage: Option<Arc<crate::sqlite::WorkflowStorage>>,
    total_steps: u32,
    completed: Arc<parking_lot::Mutex<u32>>,
    /// Snapshot accumulator so the executor can expose `step_outputs` to
    /// the outer MCP tool at any time.
    outputs_snapshot: Arc<parking_lot::RwLock<HashMap<String, Value>>>,
}

impl RunState {
    fn progress(&self) -> WorkflowUpdateProgress {
        WorkflowUpdateProgress {
            completed_steps: *self.completed.lock(),
            total_steps: self.total_steps,
        }
    }

    fn emit(&self, status: WorkflowStatus, step: Option<&str>, detail: Value) {
        self.notifier.emit(WorkflowUpdate {
            workflow_id: self.workflow_id,
            job_id: self.root_job_id,
            status,
            current_step_id: step.map(str::to_string),
            progress: self.progress(),
            detail,
        });
    }

    fn inc_completed(&self) {
        *self.completed.lock() += 1;
    }

    fn record_output_snapshot(&self, step_id: &str, output: &Value) {
        self.outputs_snapshot
            .write()
            .insert(step_id.to_string(), output.clone());
    }

    /// Snapshot of every step's output for persistence. Clones the inner
    /// map.
    #[allow(dead_code)]
    fn outputs_json(&self) -> Value {
        let map = self.outputs_snapshot.read().clone();
        let mut out = serde_json::Map::new();
        for (k, v) in map {
            out.insert(k, v);
        }
        Value::Object(out)
    }
}

// ── Executor ─────────────────────────────────────────────────────────────

/// Top-level workflow step execution engine.
///
/// Cheap to clone — all state is `Arc`-wrapped.
#[derive(Clone)]
pub struct WorkflowExecutor {
    tool_caller: SharedToolCaller,
    remote_caller: SharedRemoteCaller,
    notifier: SharedNotifier,
    artefacts: Option<SharedArtefactStore>,
    idempotency: IdempotencyCache,
    approval_gate: ApprovalGate,
    #[cfg(feature = "job-persist-sqlite")]
    storage: Option<Arc<crate::sqlite::WorkflowStorage>>,
    /// Default approval timeout when a step declares no `timeout_secs`.
    /// `None` means indefinite (matches the issue #348 spec).
    default_approve_timeout: Option<Duration>,
}

impl std::fmt::Debug for WorkflowExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowExecutor")
            .field("has_artefacts", &self.artefacts.is_some())
            .field("idempotency_entries", &self.idempotency.len())
            .field("pending_approvals", &self.approval_gate.pending_count())
            .finish()
    }
}

/// Builder for [`WorkflowExecutor`].
#[derive(Default)]
pub struct WorkflowExecutorBuilder {
    tool_caller: Option<SharedToolCaller>,
    remote_caller: Option<SharedRemoteCaller>,
    notifier: Option<SharedNotifier>,
    artefacts: Option<SharedArtefactStore>,
    idempotency: Option<IdempotencyCache>,
    approval_gate: Option<ApprovalGate>,
    #[cfg(feature = "job-persist-sqlite")]
    storage: Option<Arc<crate::sqlite::WorkflowStorage>>,
    default_approve_timeout: Option<Duration>,
}

impl WorkflowExecutorBuilder {
    /// Set the local tool caller.
    pub fn tool_caller(mut self, caller: SharedToolCaller) -> Self {
        self.tool_caller = Some(caller);
        self
    }

    /// Convenience: wrap an [`dcc_mcp_actions::dispatcher::ActionDispatcher`]
    /// as the local tool caller.
    pub fn dispatcher(mut self, dispatcher: dcc_mcp_actions::dispatcher::ActionDispatcher) -> Self {
        self.tool_caller = Some(Arc::new(ActionDispatcherCaller::new(dispatcher)));
        self
    }

    /// Set the remote / gateway caller (defaults to [`NullRemoteCaller`]).
    pub fn remote_caller(mut self, caller: SharedRemoteCaller) -> Self {
        self.remote_caller = Some(caller);
        self
    }

    /// Set the SSE notifier (defaults to [`NullNotifier`]).
    pub fn notifier(mut self, notifier: SharedNotifier) -> Self {
        self.notifier = Some(notifier);
        self
    }

    /// Set the artefact store.
    pub fn artefacts(mut self, store: SharedArtefactStore) -> Self {
        self.artefacts = Some(store);
        self
    }

    /// Override the default approval timeout (applies when a step omits
    /// `timeout_secs`).
    pub fn default_approve_timeout(mut self, d: Duration) -> Self {
        self.default_approve_timeout = Some(d);
        self
    }

    /// Attach a shared idempotency cache (defaults to a fresh one).
    pub fn idempotency(mut self, cache: IdempotencyCache) -> Self {
        self.idempotency = Some(cache);
        self
    }

    /// Attach a shared approval gate registry (defaults to a fresh one).
    pub fn approval_gate(mut self, gate: ApprovalGate) -> Self {
        self.approval_gate = Some(gate);
        self
    }

    /// Attach a SQLite storage backend. When present, every workflow/step
    /// transition is persisted and `recover()` flips non-terminal rows to
    /// `interrupted` on restart.
    #[cfg(feature = "job-persist-sqlite")]
    pub fn storage(mut self, storage: Arc<crate::sqlite::WorkflowStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Finalise. Panics if no tool caller is configured — there's no
    /// sensible default and every workflow has at least one `tool` step.
    pub fn build(self) -> WorkflowExecutor {
        WorkflowExecutor {
            tool_caller: self
                .tool_caller
                .expect("WorkflowExecutor requires a tool_caller"),
            remote_caller: self
                .remote_caller
                .unwrap_or_else(|| Arc::new(NullRemoteCaller)),
            notifier: self.notifier.unwrap_or_else(|| Arc::new(NullNotifier)),
            artefacts: self.artefacts,
            idempotency: self.idempotency.unwrap_or_default(),
            approval_gate: self.approval_gate.unwrap_or_default(),
            #[cfg(feature = "job-persist-sqlite")]
            storage: self.storage,
            default_approve_timeout: self.default_approve_timeout,
        }
    }
}

impl WorkflowExecutor {
    /// Open a builder.
    pub fn builder() -> WorkflowExecutorBuilder {
        WorkflowExecutorBuilder::default()
    }

    /// Access the approval gate — used by the MCP handler that receives
    /// `$/dcc.approveResponse` notifications.
    pub fn approval_gate(&self) -> ApprovalGate {
        self.approval_gate.clone()
    }

    /// Access the idempotency cache.
    pub fn idempotency(&self) -> IdempotencyCache {
        self.idempotency.clone()
    }

    /// Recover any interrupted workflows from persistence (issue #348).
    /// Returns the number of rows flipped to `interrupted`. On every
    /// flipped row, a final `$/dcc.workflowUpdated` is emitted via the
    /// configured notifier so connected MCP clients observe the
    /// interruption.
    #[cfg(feature = "job-persist-sqlite")]
    pub fn recover_persisted(&self) -> Result<usize, crate::sqlite::WorkflowStorageError> {
        let Some(ref storage) = self.storage else {
            return Ok(0);
        };
        let rows = storage.recover()?;
        for row in &rows {
            let update = WorkflowUpdate {
                workflow_id: row.id,
                job_id: row.root_job_id,
                status: WorkflowStatus::Interrupted,
                current_step_id: row.current_step_id.clone(),
                progress: WorkflowUpdateProgress::default(),
                detail: serde_json::json!({"kind": "interrupted", "reason": "server restart"}),
            };
            self.notifier.emit(update);
        }
        Ok(rows.len())
    }

    /// Kick off execution of `spec`. Returns a [`WorkflowRunHandle`] whose
    /// `.join` resolves when the workflow reaches a terminal state.
    pub fn run(
        &self,
        spec: WorkflowSpec,
        inputs: Value,
        _parent_job_id: Option<Uuid>,
    ) -> Result<WorkflowRunHandle, WorkflowError> {
        spec.validate()?;

        let workflow_id = Uuid::new_v4();
        let root_job_id = Uuid::new_v4();
        let cancel_token = CancellationToken::new();
        let total_steps = count_steps(&spec);

        let state = RunState {
            workflow_id,
            root_job_id,
            context: WorkflowContext::new(inputs.clone()),
            notifier: Arc::clone(&self.notifier),
            artefacts: self.artefacts.clone(),
            tool_caller: Arc::clone(&self.tool_caller),
            remote_caller: Arc::clone(&self.remote_caller),
            idempotency: self.idempotency.clone(),
            approval_gate: self.approval_gate.clone(),
            cancel_token: cancel_token.clone(),
            #[cfg(feature = "job-persist-sqlite")]
            storage: self.storage.clone(),
            total_steps,
            completed: Arc::new(parking_lot::Mutex::new(0)),
            outputs_snapshot: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        };

        // Persist initial row.
        #[cfg(feature = "job-persist-sqlite")]
        if let Some(storage) = &state.storage {
            if let Err(e) = storage.insert_workflow(workflow_id, root_job_id, &spec, &inputs) {
                warn!(error = %e, "workflow storage insert failed");
            }
        }

        // Initial pending emit.
        state.emit(
            WorkflowStatus::Pending,
            None,
            serde_json::json!({"kind": "workflow_started"}),
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
            #[cfg(feature = "job-persist-sqlite")]
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

    /// Drive a sequence of steps in order. Returns the aggregated
    /// [`WorkflowStatus`] reached after processing all steps (or the first
    /// failing / cancelled step).
    async fn drive(
        state: RunState,
        steps: Vec<Step>,
        default_approve_timeout: Option<Duration>,
    ) -> WorkflowStatus {
        for step in steps {
            if state.cancel_token.is_cancelled() {
                return WorkflowStatus::Cancelled;
            }
            match Self::run_step(state.clone(), step, default_approve_timeout).await {
                StepOutcome::Ok => {}
                StepOutcome::Cancelled => return WorkflowStatus::Cancelled,
                StepOutcome::Failed(_) => return WorkflowStatus::Failed,
            }
        }
        WorkflowStatus::Completed
    }

    /// Run a single step including its policy wrapping.
    fn run_step<'a>(
        state: RunState,
        step: Step,
        default_approve_timeout: Option<Duration>,
    ) -> Pin<Box<dyn Future<Output = StepOutcome> + Send + 'a>> {
        Box::pin(async move {
            if state.cancel_token.is_cancelled() {
                return StepOutcome::Cancelled;
            }
            let step_id = step.id.clone();
            state.emit(
                WorkflowStatus::Running,
                Some(step_id.as_str()),
                serde_json::json!({"kind": "step_enter", "step_id": step_id.0}),
            );
            #[cfg(feature = "job-persist-sqlite")]
            if let Some(ref s) = state.storage {
                let _ = s.upsert_step(state.workflow_id, step_id.as_str(), "running", None, None);
                let _ = s.update_workflow_status(
                    state.workflow_id,
                    WorkflowStatus::Running,
                    Some(step_id.as_str()),
                );
            }

            let outcome = match &step.kind {
                StepKind::Tool { .. } => Self::run_tool_step(&state, &step).await,
                StepKind::ToolRemote { .. } => Self::run_remote_step(&state, &step).await,
                StepKind::Foreach { .. } => {
                    Self::run_foreach(state.clone(), step.clone(), default_approve_timeout).await
                }
                StepKind::Parallel { .. } => {
                    Self::run_parallel(state.clone(), step.clone(), default_approve_timeout).await
                }
                StepKind::Approve { .. } => {
                    Self::run_approve(state.clone(), step.clone(), default_approve_timeout).await
                }
                StepKind::Branch { .. } => {
                    Self::run_branch(state.clone(), step.clone(), default_approve_timeout).await
                }
            };

            match &outcome {
                StepOutcome::Ok => {
                    state.inc_completed();
                    state.emit(
                        WorkflowStatus::Running,
                        Some(step_id.as_str()),
                        serde_json::json!({"kind": "step_exit", "step_id": step_id.0, "status": "completed"}),
                    );
                    #[cfg(feature = "job-persist-sqlite")]
                    if let Some(ref s) = state.storage {
                        let out = state
                            .context
                            .step(step_id.as_str())
                            .map(|o| o.output)
                            .unwrap_or(Value::Null);
                        let _ = s.upsert_step(
                            state.workflow_id,
                            step_id.as_str(),
                            "completed",
                            Some(&out),
                            None,
                        );
                    }
                }
                StepOutcome::Cancelled => {
                    state.emit(
                        WorkflowStatus::Cancelled,
                        Some(step_id.as_str()),
                        serde_json::json!({"kind": "step_exit", "step_id": step_id.0, "status": "cancelled"}),
                    );
                    #[cfg(feature = "job-persist-sqlite")]
                    if let Some(ref s) = state.storage {
                        let _ = s.upsert_step(
                            state.workflow_id,
                            step_id.as_str(),
                            "cancelled",
                            None,
                            Some("cancelled"),
                        );
                    }
                }
                StepOutcome::Failed(e) => {
                    state.emit(
                        WorkflowStatus::Failed,
                        Some(step_id.as_str()),
                        serde_json::json!({"kind": "step_exit", "step_id": step_id.0, "status": "failed", "error": e}),
                    );
                    #[cfg(feature = "job-persist-sqlite")]
                    if let Some(ref s) = state.storage {
                        let _ = s.upsert_step(
                            state.workflow_id,
                            step_id.as_str(),
                            "failed",
                            None,
                            Some(e.as_str()),
                        );
                    }
                }
            }

            outcome
        })
    }

    // ── Tool step ────────────────────────────────────────────────────────

    async fn run_tool_step(state: &RunState, step: &Step) -> StepOutcome {
        let (name, args) = match &step.kind {
            StepKind::Tool { tool, args } => (tool.clone(), args.clone()),
            _ => unreachable!(),
        };
        let call = |rendered_args: Value, cancel: CancellationToken| {
            let caller = Arc::clone(&state.tool_caller);
            let name = name.clone();
            async move { caller.call(&name, rendered_args, cancel).await }
        };
        run_with_policy(state, step, args, call).await
    }

    // ── ToolRemote step ──────────────────────────────────────────────────

    async fn run_remote_step(state: &RunState, step: &Step) -> StepOutcome {
        let (dcc, tool, args) = match &step.kind {
            StepKind::ToolRemote { dcc, tool, args } => (dcc.clone(), tool.clone(), args.clone()),
            _ => unreachable!(),
        };
        let call = |rendered_args: Value, cancel: CancellationToken| {
            let caller = Arc::clone(&state.remote_caller);
            let dcc = dcc.clone();
            let tool = tool.clone();
            async move { caller.call(&dcc, &tool, rendered_args, cancel).await }
        };
        run_with_policy(state, step, args, call).await
    }

    // ── Foreach step ─────────────────────────────────────────────────────

    async fn run_foreach(
        state: RunState,
        step: Step,
        default_approve_timeout: Option<Duration>,
    ) -> StepOutcome {
        let (items_expr, item_name, body) = match &step.kind {
            StepKind::Foreach { items, r#as, steps } => {
                (items.clone(), r#as.clone(), steps.clone())
            }
            _ => unreachable!(),
        };
        let root = state.context.as_json();
        let items_val = match eval_jsonpath(&items_expr, &root) {
            Ok(v) => v,
            Err(e) => return StepOutcome::Failed(format!("foreach.items: {e}")),
        };
        let items: Vec<Value> = match items_val {
            Value::Array(arr) => arr,
            Value::Null => Vec::new(),
            other => vec![other],
        };
        let mut agg_outputs: Vec<Value> = Vec::with_capacity(items.len());
        for (i, item) in items.into_iter().enumerate() {
            if state.cancel_token.is_cancelled() {
                return StepOutcome::Cancelled;
            }
            let _guard = state.context.push_item(&item_name, item.clone());
            debug!(step_id = %step.id, index = i, "foreach iteration");
            match Self::drive(state.clone(), body.clone(), default_approve_timeout).await {
                WorkflowStatus::Completed => {
                    // Snapshot inner step outputs for this iteration.
                    let snap = state
                        .context
                        .steps_snapshot()
                        .into_iter()
                        .map(|(k, v)| (k, v.output))
                        .collect::<HashMap<_, _>>();
                    agg_outputs.push(serde_json::to_value(snap).unwrap_or(Value::Null));
                }
                WorkflowStatus::Cancelled => return StepOutcome::Cancelled,
                WorkflowStatus::Failed => {
                    return StepOutcome::Failed(format!("foreach iteration {i} failed"));
                }
                other => return StepOutcome::Failed(format!("foreach reached unexpected {other}")),
            }
        }
        let out_val = serde_json::json!({"iterations": agg_outputs});
        state
            .context
            .record_step(&step.id, StepOutput::from_value(out_val.clone()));
        state.record_output_snapshot(step.id.as_str(), &out_val);
        StepOutcome::Ok
    }

    // ── Parallel step ────────────────────────────────────────────────────

    async fn run_parallel(
        state: RunState,
        step: Step,
        default_approve_timeout: Option<Duration>,
    ) -> StepOutcome {
        let body = match &step.kind {
            StepKind::Parallel { steps } => steps.clone(),
            _ => unreachable!(),
        };
        let mut joins = Vec::with_capacity(body.len());
        for branch in body {
            let st = state.clone();
            let child_cancel = state.cancel_token.child_token();
            let child_state = RunState {
                cancel_token: child_cancel,
                ..st
            };
            let handle = tokio::spawn(async move {
                Self::drive(child_state, vec![branch], default_approve_timeout).await
            });
            joins.push(handle);
        }
        let mut branch_results: Vec<WorkflowStatus> = Vec::with_capacity(joins.len());
        for h in joins {
            match h.await {
                Ok(status) => branch_results.push(status),
                Err(e) => return StepOutcome::Failed(format!("parallel join error: {e}")),
            }
        }
        let any_cancel = branch_results
            .iter()
            .any(|s| matches!(s, WorkflowStatus::Cancelled));
        let any_fail = branch_results
            .iter()
            .any(|s| matches!(s, WorkflowStatus::Failed));
        let out_val = serde_json::json!({"branch_results": branch_results.iter().map(|s| s.as_str()).collect::<Vec<_>>()});
        state
            .context
            .record_step(&step.id, StepOutput::from_value(out_val.clone()));
        state.record_output_snapshot(step.id.as_str(), &out_val);
        if any_cancel {
            return StepOutcome::Cancelled;
        }
        if any_fail {
            return StepOutcome::Failed("one or more parallel branches failed".to_string());
        }
        StepOutcome::Ok
    }

    // ── Approve step ─────────────────────────────────────────────────────

    async fn run_approve(
        state: RunState,
        step: Step,
        default_approve_timeout: Option<Duration>,
    ) -> StepOutcome {
        let prompt = match &step.kind {
            StepKind::Approve { prompt } => prompt.clone(),
            _ => unreachable!(),
        };
        let rendered_prompt = state
            .context
            .render(&Value::String(prompt))
            .unwrap_or(Value::Null);

        let rx = state
            .approval_gate
            .wait_handle(state.workflow_id, step.id.as_str());

        state.emit(
            WorkflowStatus::Running,
            Some(step.id.as_str()),
            serde_json::json!({
                "kind": "approve_requested",
                "step_id": step.id.0,
                "prompt": rendered_prompt,
            }),
        );

        let timeout_dur = step.policy.timeout.or(default_approve_timeout);
        let cancel = state.cancel_token.clone();

        let response = if let Some(d) = timeout_dur {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    state.approval_gate.discard(state.workflow_id, step.id.as_str());
                    return StepOutcome::Cancelled;
                }
                _ = tokio::time::sleep(d) => {
                    state.approval_gate.discard(state.workflow_id, step.id.as_str());
                    ApprovalResponse::timeout()
                }
                r = rx => match r {
                    Ok(v) => v,
                    Err(_) => ApprovalResponse::cancelled(),
                }
            }
        } else {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    state.approval_gate.discard(state.workflow_id, step.id.as_str());
                    return StepOutcome::Cancelled;
                }
                r = rx => match r {
                    Ok(v) => v,
                    Err(_) => ApprovalResponse::cancelled(),
                }
            }
        };

        let out_val = serde_json::json!({
            "approved": response.approved,
            "reason": response.reason,
        });
        state
            .context
            .record_step(&step.id, StepOutput::from_value(out_val.clone()));
        state.record_output_snapshot(step.id.as_str(), &out_val);
        if response.approved {
            StepOutcome::Ok
        } else {
            StepOutcome::Failed(format!(
                "approval denied: {}",
                response.reason.unwrap_or_else(|| "unspecified".to_string())
            ))
        }
    }

    // ── Branch step ──────────────────────────────────────────────────────

    async fn run_branch(
        state: RunState,
        step: Step,
        default_approve_timeout: Option<Duration>,
    ) -> StepOutcome {
        let (on, then, else_steps) = match &step.kind {
            StepKind::Branch {
                on,
                then,
                else_steps,
            } => (on.clone(), then.clone(), else_steps.clone()),
            _ => unreachable!(),
        };
        let root = state.context.as_json();
        let result = match eval_jsonpath(&on, &root) {
            Ok(v) => v,
            Err(e) => return StepOutcome::Failed(format!("branch.on: {e}")),
        };
        let truthy = is_truthy(&result);
        let branch = if truthy { then } else { else_steps };
        let out_val =
            serde_json::json!({"condition": result, "taken": if truthy {"then"} else {"else"}});
        state
            .context
            .record_step(&step.id, StepOutput::from_value(out_val.clone()));
        state.record_output_snapshot(step.id.as_str(), &out_val);
        match Self::drive(state.clone(), branch, default_approve_timeout).await {
            WorkflowStatus::Completed => StepOutcome::Ok,
            WorkflowStatus::Cancelled => StepOutcome::Cancelled,
            WorkflowStatus::Failed => StepOutcome::Failed("branch body failed".to_string()),
            other => StepOutcome::Failed(format!("branch body reached unexpected {other}")),
        }
    }
}

// ── Policy wrapper for Tool / ToolRemote ─────────────────────────────────

async fn run_with_policy<F, Fut>(
    state: &RunState,
    step: &Step,
    raw_args: Value,
    mut call: F,
) -> StepOutcome
where
    F: FnMut(Value, CancellationToken) -> Fut,
    Fut: Future<Output = Result<Value, String>>,
{
    // Render args once. Template errors are fatal — no retry helps.
    let rendered_args = match state.context.render(&raw_args) {
        Ok(v) => v,
        Err(e) => return StepOutcome::Failed(format!("template error: {e}")),
    };

    // Idempotency — render the key against the context too.
    let idem_key = match &step.policy.idempotency_key {
        Some(tpl) => match state.context.render(&Value::String(tpl.clone())) {
            Ok(Value::String(s)) => Some(s),
            Ok(other) => Some(other.to_string()),
            Err(e) => return StepOutcome::Failed(format!("idempotency key template: {e}")),
        },
        None => None,
    };
    if let Some(ref rendered_key) = idem_key {
        if let Some(cached) = state.idempotency.get(
            step.policy.idempotency_scope,
            state.workflow_id,
            rendered_key,
        ) {
            debug!(step_id = %step.id, key = %rendered_key, "idempotency cache hit");
            let step_out = ingest_output(state, &step.id, cached);
            state.context.record_step(&step.id, step_out.clone());
            state.record_output_snapshot(step.id.as_str(), &step_out.output);
            return StepOutcome::Ok;
        }
    }

    // Retry loop.
    let retry = step.policy.retry.clone();
    let max_attempts = retry.as_ref().map(|r| r.max_attempts).unwrap_or(1).max(1);
    let mut last_err: Option<String> = None;
    for attempt in 1..=max_attempts {
        if state.cancel_token.is_cancelled() {
            return StepOutcome::Cancelled;
        }
        // Pre-attempt delay.
        if attempt > 1 {
            let d = retry
                .as_ref()
                .map(|r| r.next_delay(attempt))
                .unwrap_or(Duration::ZERO);
            if d > Duration::ZERO {
                tokio::select! {
                    biased;
                    _ = state.cancel_token.cancelled() => return StepOutcome::Cancelled,
                    _ = tokio::time::sleep(d) => {},
                }
            }
        }

        let child_cancel = state.cancel_token.child_token();
        let call_fut = call(rendered_args.clone(), child_cancel.clone());

        // Timeout wrapper.
        let result: Result<Result<Value, String>, tokio::time::error::Elapsed> =
            match step.policy.timeout {
                Some(d) => {
                    tokio::select! {
                        biased;
                        _ = state.cancel_token.cancelled() => return StepOutcome::Cancelled,
                        r = tokio::time::timeout(d, call_fut) => r,
                    }
                }
                None => Ok({
                    tokio::select! {
                        biased;
                        _ = state.cancel_token.cancelled() => return StepOutcome::Cancelled,
                        r = call_fut => r,
                    }
                }),
            };

        match result {
            Ok(Ok(output)) => {
                let step_out = ingest_output(state, &step.id, output);
                state.context.record_step(&step.id, step_out.clone());
                state.record_output_snapshot(step.id.as_str(), &step_out.output);
                if let Some(ref rendered_key) = idem_key {
                    state.idempotency.put(
                        step.policy.idempotency_scope,
                        state.workflow_id,
                        rendered_key,
                        step_out.output.clone(),
                    );
                }
                return StepOutcome::Ok;
            }
            Ok(Err(e)) => {
                // Handler error — retryable only if the policy says so.
                last_err = Some(e.clone());
                let retryable = retry
                    .as_ref()
                    .map(|r| r.is_retryable(&classify_error(&e)))
                    .unwrap_or(false);
                if !retryable {
                    break;
                }
            }
            Err(_elapsed) => {
                last_err = Some("timeout".to_string());
                let retryable = retry
                    .as_ref()
                    .map(|r| r.is_retryable("timeout"))
                    .unwrap_or(false);
                if !retryable {
                    break;
                }
            }
        }
    }
    StepOutcome::Failed(last_err.unwrap_or_else(|| "unknown".to_string()))
}

/// Turn a raw tool output into a [`StepOutput`], persisting any inline
/// artefacts into the configured store when present.
fn ingest_output(state: &RunState, step_id: &StepId, mut output: Value) -> StepOutput {
    // Promote `file_refs` from raw output to artefact store when possible.
    // We consult `output.file_refs` and `output.context.file_refs`; any
    // entry that has `inline_bytes` (base64) is re-put into the store.
    if let Some(store) = &state.artefacts {
        maybe_upload_inline_refs(store.as_ref(), &mut output, state.root_job_id);
    }
    let mut step_out = StepOutput::from_value(output);
    // Ensure each FileRef picks up the producer_job_id for downstream filters.
    for fr in step_out.file_refs.iter_mut() {
        if fr.producer_job_id.is_none() {
            fr.producer_job_id = Some(state.root_job_id);
        }
    }
    let _ = step_id; // reserved for future step-level artefact tagging
    step_out
}

fn maybe_upload_inline_refs(
    store: &dyn dcc_mcp_artefact::ArtefactStore,
    output: &mut Value,
    producer_job: Uuid,
) {
    let upload_one = |entry: &mut Value| {
        if let Some(obj) = entry.as_object_mut() {
            // If the entry already has a `uri`, leave it alone.
            if obj.get("uri").and_then(Value::as_str).is_some() {
                return;
            }
            if let Some(b64) = obj.get("inline_b64").and_then(Value::as_str) {
                use base64::Engine as _;
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                    match store.put(ArtefactBody::Inline(bytes)) {
                        Ok(mut fr) => {
                            if fr.producer_job_id.is_none() {
                                fr.producer_job_id = Some(producer_job);
                            }
                            if let Ok(v) = serde_json::to_value(&fr) {
                                *entry = v;
                            }
                        }
                        Err(e) => warn!(error = %e, "inline artefact upload failed"),
                    }
                }
            } else if let Some(path) = obj.get("path").and_then(Value::as_str) {
                let p = std::path::PathBuf::from(path);
                match store.put(ArtefactBody::Path(p)) {
                    Ok(mut fr) => {
                        if fr.producer_job_id.is_none() {
                            fr.producer_job_id = Some(producer_job);
                        }
                        if let Ok(v) = serde_json::to_value(&fr) {
                            *entry = v;
                        }
                    }
                    Err(e) => warn!(error = %e, "path artefact upload failed"),
                }
            }
        }
    };

    let uploaders = |arr_key: &str, root: &mut Value| {
        if let Some(arr) = root.get_mut(arr_key).and_then(Value::as_array_mut) {
            for entry in arr.iter_mut() {
                upload_one(entry);
            }
        }
    };

    uploaders("file_refs", output);
    if let Some(ctx) = output.get_mut("context") {
        if let Some(arr) = ctx.get_mut("file_refs").and_then(Value::as_array_mut) {
            for entry in arr.iter_mut() {
                upload_one(entry);
            }
        }
    }
    // Squash unused warning when `root_job_id` not needed.
    let _ = producer_job;
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Step outcome used internally by the drivers.
#[derive(Debug)]
enum StepOutcome {
    Ok,
    Cancelled,
    Failed(String),
}

fn eval_jsonpath(expr: &str, root: &Value) -> Result<Value, String> {
    // jsonpath-rust 1.x — value can be queried directly.
    match root.query(expr) {
        Ok(hits) => {
            if hits.is_empty() {
                Ok(Value::Null)
            } else if hits.len() == 1 {
                Ok(hits[0].clone())
            } else {
                Ok(Value::Array(hits.into_iter().cloned().collect()))
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn classify_error(e: &str) -> String {
    // Very small heuristic — user-supplied retry_on lists are the canonical
    // source of truth. We only need a string label that aligns with the
    // allowlist (e.g. "timeout", "transient"). Default to "error".
    if e.contains("timeout") {
        "timeout".to_string()
    } else if e.contains("transient") {
        "transient".to_string()
    } else {
        "error".to_string()
    }
}

fn count_steps(spec: &WorkflowSpec) -> u32 {
    fn count(steps: &[Step]) -> u32 {
        steps
            .iter()
            .map(|s| {
                1 + match &s.kind {
                    StepKind::Foreach { steps, .. } | StepKind::Parallel { steps } => count(steps),
                    StepKind::Branch {
                        then, else_steps, ..
                    } => count(then) + count(else_steps),
                    _ => 0,
                }
            })
            .sum()
    }
    count(&spec.steps)
}

// Squash unused in non-sqlite builds.
#[allow(dead_code)]
fn _silence_retry_policy<'a>(_r: &'a RetryPolicy, _s: &'a StepPolicy) {}
#[allow(dead_code)]
fn _silence_backoff(_b: BackoffKind) {}
#[allow(dead_code)]
fn _silence_scope(_s: IdempotencyScope) {}
#[allow(dead_code)]
fn _silence_fileref(_f: &FileRef) {}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callers::ToolCaller;
    use crate::callers::test_support::{MockRemoteCaller, MockToolCaller};
    use crate::notifier::RecordingNotifier;
    use crate::policy::{RetryPolicy as PolicyRetryPolicy, StepPolicy as PolicyStepPolicy};
    use crate::spec::{Step, StepId, StepKind, WorkflowSpec};
    use serde_json::json;
    use std::sync::Arc;

    fn spec_with_steps(steps: Vec<Step>) -> WorkflowSpec {
        WorkflowSpec {
            name: "t".to_string(),
            description: String::new(),
            inputs: Value::Null,
            steps,
        }
    }

    fn tool_step(id: &str, tool: &str, args: Value) -> Step {
        Step {
            id: StepId(id.to_string()),
            kind: StepKind::Tool {
                tool: tool.to_string(),
                args,
            },
            policy: PolicyStepPolicy::default(),
        }
    }

    fn remote_step(id: &str, dcc: &str, tool: &str, args: Value) -> Step {
        Step {
            id: StepId(id.to_string()),
            kind: StepKind::ToolRemote {
                dcc: dcc.to_string(),
                tool: tool.to_string(),
                args,
            },
            policy: PolicyStepPolicy::default(),
        }
    }

    #[tokio::test]
    async fn tool_step_runs_and_completes() {
        let mock = Arc::new(MockToolCaller::new());
        mock.add("echo", |args| Ok(json!({"echoed": args})));
        let rec = Arc::new(RecordingNotifier::new());

        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .notifier(rec.clone())
            .build();

        let spec = spec_with_steps(vec![tool_step("s1", "echo", json!({"x": 1}))]);
        let handle = exe.run(spec, Value::Null, None).unwrap();
        let status = handle.wait().await;
        assert_eq!(status, WorkflowStatus::Completed);
        assert_eq!(mock.call_count("echo"), 1);
        assert!(
            rec.len() >= 3,
            "expected enter/exit/terminal events, got {}",
            rec.len()
        );
    }

    #[tokio::test]
    async fn tool_step_args_are_rendered_against_inputs() {
        let mock = Arc::new(MockToolCaller::new());
        let seen = Arc::new(parking_lot::Mutex::new(Value::Null));
        let seen_c = seen.clone();
        mock.add("echo", move |args| {
            *seen_c.lock() = args.clone();
            Ok(json!({"ok": true}))
        });
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let spec = spec_with_steps(vec![tool_step(
            "s1",
            "echo",
            json!({"name": "{{inputs.who}}"}),
        )]);
        let h = exe.run(spec, json!({"who": "alice"}), None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(*seen.lock(), json!({"name": "alice"}));
    }

    #[tokio::test]
    async fn step_output_is_accessible_to_next_step() {
        let mock = Arc::new(MockToolCaller::new());
        mock.add("produce", |_| Ok(json!({"value": 42})));
        let seen = Arc::new(parking_lot::Mutex::new(Value::Null));
        let seen_c = seen.clone();
        mock.add("consume", move |args| {
            *seen_c.lock() = args.clone();
            Ok(Value::Null)
        });
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let spec = spec_with_steps(vec![
            tool_step("a", "produce", Value::Null),
            tool_step("b", "consume", json!({"v": "{{steps.a.output.value}}"})),
        ]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(*seen.lock(), json!({"v": 42}));
    }

    #[tokio::test]
    async fn tool_step_failure_fails_workflow() {
        let mock = Arc::new(MockToolCaller::new());
        mock.add("boom", |_| Err("nope".to_string()));
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let spec = spec_with_steps(vec![tool_step("s", "boom", Value::Null)]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Failed);
    }

    #[tokio::test]
    async fn retry_policy_retries_on_transient() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let attempts = Arc::new(AtomicU32::new(0));
        let a_c = attempts.clone();
        let mock = Arc::new(MockToolCaller::new());
        mock.add("flaky", move |_| {
            let n = a_c.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err("transient".to_string())
            } else {
                Ok(json!({"ok": true}))
            }
        });
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let mut step = tool_step("s", "flaky", Value::Null);
        step.policy.retry = Some(PolicyRetryPolicy {
            max_attempts: 5,
            backoff: BackoffKind::Fixed,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            jitter: 0.0,
            retry_on: Some(vec!["transient".to_string()]),
        });
        let spec = spec_with_steps(vec![step]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_policy_stops_on_non_retryable() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let attempts = Arc::new(AtomicU32::new(0));
        let a_c = attempts.clone();
        let mock = Arc::new(MockToolCaller::new());
        mock.add("flaky", move |_| {
            a_c.fetch_add(1, Ordering::SeqCst);
            Err("validation".to_string())
        });
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let mut step = tool_step("s", "flaky", Value::Null);
        step.policy.retry = Some(PolicyRetryPolicy {
            max_attempts: 5,
            backoff: BackoffKind::Fixed,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            jitter: 0.0,
            retry_on: Some(vec!["transient".to_string()]),
        });
        let spec = spec_with_steps(vec![step]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Failed);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn timeout_policy_fires() {
        let mock = Arc::new(MockToolCaller::new());
        // Handler completes instantly but sleeps inside tokio task.
        struct SlowCaller;
        impl ToolCaller for SlowCaller {
            fn call<'a>(
                &'a self,
                _n: &'a str,
                _a: Value,
                cancel: CancellationToken,
            ) -> crate::callers::CallFuture<'a> {
                Box::pin(async move {
                    tokio::select! {
                        _ = cancel.cancelled() => Err("cancelled".to_string()),
                        _ = tokio::time::sleep(Duration::from_millis(500)) => Ok(Value::Null),
                    }
                })
            }
        }
        let _ = mock;
        let exe = WorkflowExecutor::builder()
            .tool_caller(Arc::new(SlowCaller))
            .build();
        let mut step = tool_step("s", "slow", Value::Null);
        step.policy.timeout = Some(Duration::from_millis(20));
        let spec = spec_with_steps(vec![step]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Failed);
    }

    #[tokio::test]
    async fn idempotency_key_short_circuits_second_call() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let mock = Arc::new(MockToolCaller::new());
        mock.add("op", move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"n": 1}))
        });
        let cache = IdempotencyCache::new();
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .idempotency(cache.clone())
            .build();
        let mut step1 = tool_step("s", "op", Value::Null);
        step1.policy.idempotency_key = Some("fixed-key".to_string());
        step1.policy.idempotency_scope = IdempotencyScope::Global;
        let spec1 = spec_with_steps(vec![step1.clone()]);
        let h1 = exe.run(spec1, Value::Null, None).unwrap();
        assert_eq!(h1.wait().await, WorkflowStatus::Completed);
        // Second workflow, same key, global scope → cached.
        let spec2 = spec_with_steps(vec![step1]);
        let h2 = exe.run(spec2, Value::Null, None).unwrap();
        assert_eq!(h2.wait().await, WorkflowStatus::Completed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancellation_aborts_workflow() {
        struct BlockingCaller;
        impl ToolCaller for BlockingCaller {
            fn call<'a>(
                &'a self,
                _n: &'a str,
                _a: Value,
                cancel: CancellationToken,
            ) -> crate::callers::CallFuture<'a> {
                Box::pin(async move {
                    cancel.cancelled().await;
                    Err("cancelled".to_string())
                })
            }
        }
        let exe = WorkflowExecutor::builder()
            .tool_caller(Arc::new(BlockingCaller))
            .build();
        let spec = spec_with_steps(vec![tool_step("s", "block", Value::Null)]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        let cancel = h.cancel_token.clone();
        let join = h.join;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel.cancel();
        });
        let status = tokio::time::timeout(Duration::from_millis(500), join)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, WorkflowStatus::Cancelled);
    }

    #[tokio::test]
    async fn foreach_iterates_over_jsonpath_items() {
        let mock = Arc::new(MockToolCaller::new());
        mock.add("per", |args| Ok(json!({"got": args})));
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let foreach = Step {
            id: StepId("loop".into()),
            kind: StepKind::Foreach {
                items: "$.inputs.items".to_string(),
                r#as: "item".to_string(),
                steps: vec![tool_step("inner", "per", json!({"v": "{{item}}"}))],
            },
            policy: PolicyStepPolicy::default(),
        };
        let spec = spec_with_steps(vec![foreach]);
        let h = exe
            .run(spec, json!({"items": ["a", "b", "c"]}), None)
            .unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(mock.call_count("per"), 3);
    }

    #[tokio::test]
    async fn parallel_runs_branches_concurrently() {
        let mock = Arc::new(MockToolCaller::new());
        mock.add("a", |_| Ok(json!({"from": "a"})));
        mock.add("b", |_| Ok(json!({"from": "b"})));
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let parallel = Step {
            id: StepId("par".into()),
            kind: StepKind::Parallel {
                steps: vec![
                    tool_step("x", "a", Value::Null),
                    tool_step("y", "b", Value::Null),
                ],
            },
            policy: PolicyStepPolicy::default(),
        };
        let spec = spec_with_steps(vec![parallel]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(mock.call_count("a"), 1);
        assert_eq!(mock.call_count("b"), 1);
    }

    #[tokio::test]
    async fn parallel_any_failure_fails_workflow() {
        let mock = Arc::new(MockToolCaller::new());
        mock.add("ok", |_| Ok(Value::Null));
        mock.add("bad", |_| Err("fail".to_string()));
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let parallel = Step {
            id: StepId("par".into()),
            kind: StepKind::Parallel {
                steps: vec![
                    tool_step("x", "ok", Value::Null),
                    tool_step("y", "bad", Value::Null),
                ],
            },
            policy: PolicyStepPolicy::default(),
        };
        let spec = spec_with_steps(vec![parallel]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Failed);
    }

    #[tokio::test]
    async fn branch_takes_then_on_truthy() {
        let mock = Arc::new(MockToolCaller::new());
        mock.add("then_path", |_| Ok(json!({"branch": "then"})));
        mock.add("else_path", |_| Ok(json!({"branch": "else"})));
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let branch = Step {
            id: StepId("gate".into()),
            kind: StepKind::Branch {
                on: "$.inputs.flag".to_string(),
                then: vec![tool_step("t", "then_path", Value::Null)],
                else_steps: vec![tool_step("e", "else_path", Value::Null)],
            },
            policy: PolicyStepPolicy::default(),
        };
        let spec = spec_with_steps(vec![branch]);
        let h = exe.run(spec, json!({"flag": true}), None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(mock.call_count("then_path"), 1);
        assert_eq!(mock.call_count("else_path"), 0);
    }

    #[tokio::test]
    async fn branch_takes_else_on_falsy() {
        let mock = Arc::new(MockToolCaller::new());
        mock.add("then_path", |_| Ok(Value::Null));
        mock.add("else_path", |_| Ok(Value::Null));
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let branch = Step {
            id: StepId("gate".into()),
            kind: StepKind::Branch {
                on: "$.inputs.flag".to_string(),
                then: vec![tool_step("t", "then_path", Value::Null)],
                else_steps: vec![tool_step("e", "else_path", Value::Null)],
            },
            policy: PolicyStepPolicy::default(),
        };
        let spec = spec_with_steps(vec![branch]);
        let h = exe.run(spec, json!({"flag": false}), None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(mock.call_count("then_path"), 0);
        assert_eq!(mock.call_count("else_path"), 1);
    }

    #[tokio::test]
    async fn approve_step_resolves_when_gate_approves() {
        let mock = Arc::new(MockToolCaller::new());
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let step = Step {
            id: StepId("ok_to_go".into()),
            kind: StepKind::Approve {
                prompt: "go?".to_string(),
            },
            policy: PolicyStepPolicy::default(),
        };
        let spec = spec_with_steps(vec![step]);
        let gate = exe.approval_gate();
        let h = exe.run(spec, Value::Null, None).unwrap();
        let wid = h.workflow_id;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            gate.resolve(
                wid,
                "ok_to_go",
                crate::approval::ApprovalResponse {
                    approved: true,
                    reason: None,
                },
            );
        });
        let status = tokio::time::timeout(Duration::from_secs(2), h.join)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, WorkflowStatus::Completed);
    }

    #[tokio::test]
    async fn approve_step_times_out_when_policy_timeout_set() {
        let mock = Arc::new(MockToolCaller::new());
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .build();
        let mut step = Step {
            id: StepId("wait".into()),
            kind: StepKind::Approve {
                prompt: "go?".to_string(),
            },
            policy: PolicyStepPolicy::default(),
        };
        step.policy.timeout = Some(Duration::from_millis(40));
        let spec = spec_with_steps(vec![step]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(2), h.join)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, WorkflowStatus::Failed);
    }

    #[tokio::test]
    async fn remote_step_invokes_remote_caller() {
        let local = Arc::new(MockToolCaller::new());
        let remote = Arc::new(MockRemoteCaller::new());
        remote.add("unreal", "ingest", |_| Ok(json!({"ok": true})));
        let exe = WorkflowExecutor::builder()
            .tool_caller(local.clone())
            .remote_caller(remote.clone())
            .build();
        let spec = spec_with_steps(vec![remote_step("r", "unreal", "ingest", Value::Null)]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        assert_eq!(remote.calls.lock().len(), 1);
    }

    #[tokio::test]
    async fn workflow_emits_terminal_notifier_event() {
        let mock = Arc::new(MockToolCaller::new());
        mock.add("echo", |_| Ok(Value::Null));
        let rec = Arc::new(RecordingNotifier::new());
        let exe = WorkflowExecutor::builder()
            .tool_caller(mock.clone())
            .notifier(rec.clone())
            .build();
        let spec = spec_with_steps(vec![tool_step("s", "echo", Value::Null)]);
        let h = exe.run(spec, Value::Null, None).unwrap();
        assert_eq!(h.wait().await, WorkflowStatus::Completed);
        let events = rec.events();
        assert!(matches!(
            events.last().unwrap().status,
            WorkflowStatus::Completed
        ));
    }

    #[test]
    fn count_steps_counts_nested() {
        let spec = spec_with_steps(vec![Step {
            id: StepId("p".into()),
            kind: StepKind::Parallel {
                steps: vec![
                    tool_step("a", "x", Value::Null),
                    tool_step("b", "y", Value::Null),
                ],
            },
            policy: PolicyStepPolicy::default(),
        }]);
        assert_eq!(count_steps(&spec), 3);
    }

    #[test]
    fn is_truthy_sanity() {
        assert!(!is_truthy(&Value::Null));
        assert!(!is_truthy(&json!(false)));
        assert!(!is_truthy(&json!(0)));
        assert!(!is_truthy(&json!("")));
        assert!(!is_truthy(&json!([])));
        assert!(!is_truthy(&json!({})));
        assert!(is_truthy(&json!(true)));
        assert!(is_truthy(&json!(1)));
        assert!(is_truthy(&json!("x")));
        assert!(is_truthy(&json!([1])));
        assert!(is_truthy(&json!({"a": 1})));
    }
}
