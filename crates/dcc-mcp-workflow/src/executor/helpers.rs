use super::*;

/// Step outcome used internally by the drivers.
#[derive(Debug)]
pub enum StepOutcome {
    Ok,
    Cancelled,
    Failed(String),
}

pub fn eval_jsonpath(expr: &str, root: &Value) -> Result<Value, String> {
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

pub fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

pub fn classify_error(e: &str) -> String {
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

pub fn count_steps(spec: &WorkflowSpec) -> u32 {
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
pub fn _silence_retry_policy<'a>(_r: &'a RetryPolicy, _s: &'a StepPolicy) {}
#[allow(dead_code)]
pub fn _silence_backoff(_b: BackoffKind) {}
#[allow(dead_code)]
pub fn _silence_scope(_s: IdempotencyScope) {}
#[allow(dead_code)]
pub fn _silence_fileref(_f: &FileRef) {}
