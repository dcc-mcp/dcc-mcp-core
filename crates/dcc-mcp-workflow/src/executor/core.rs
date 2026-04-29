use super::*;

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

    /// Access the configured idempotency store (trait object).
    pub fn idempotency(&self) -> SharedIdempotencyStore {
        Arc::clone(&self.idempotency)
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
            idempotency: Arc::clone(&self.idempotency),
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
    pub(crate) async fn drive(
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
}
