use super::*;

impl WorkflowExecutor {
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
