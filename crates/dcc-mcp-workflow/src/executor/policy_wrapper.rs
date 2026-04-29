use super::*;

/// Policy wrapper for Tool / ToolRemote steps.
pub async fn run_with_policy<F, Fut>(
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
                        step.id.as_str(),
                        step_out.output.clone(),
                        step.policy.idempotency_ttl_secs,
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
