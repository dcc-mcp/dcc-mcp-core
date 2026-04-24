//! Fluent builder and executor for [`ActionChain`].

use std::sync::Arc;

use serde_json::Value;

use super::interpolate_impl::{interpolate, merge_into_context};
use super::types::{
    ChainResult, ChainStep, ChainStepResult, ErrorAction, ErrorHandlerFn, StepParams,
};
use crate::dispatcher::{ActionDispatcher, DispatchError};

// ── ActionChain ───────────────────────────────────────────────────────────────

/// Fluent builder for multi-step action chains.
///
/// # Context propagation
///
/// After each step succeeds, the output value is merged into the shared
/// context if it is a JSON object. Nested keys are **not** recursively merged;
/// top-level keys from the output overwrite existing context entries.
///
/// # Placeholder interpolation
///
/// Static params supplied via [`step`](Self::step) may contain `{key}` string
/// placeholders which are replaced with values from the accumulated context
/// before dispatch.
pub struct ActionChain {
    pub(crate) steps: Vec<ChainStep>,
    pub(crate) on_error: Option<ErrorHandlerFn>,
}

impl Default for ActionChain {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionChain {
    /// Create an empty chain.
    #[must_use]
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            on_error: None,
        }
    }

    /// Add a step with static params (supports `{key}` interpolation).
    ///
    /// This is the most common form: the params JSON value is interpolated
    /// against the accumulated context immediately before dispatch.
    #[must_use]
    pub fn step(mut self, action: impl Into<String>, params: Value) -> Self {
        self.steps.push(ChainStep {
            action: action.into(),
            label: None,
            stop_on_failure: true,
            params: StepParams::Static(params),
        });
        self
    }

    /// Add a step with a dynamic params closure.
    ///
    /// The closure receives the current context and returns the params value.
    /// No placeholder interpolation is applied (the closure handles it).
    #[must_use]
    pub fn step_with<F>(mut self, action: impl Into<String>, f: F) -> Self
    where
        F: Fn(&Value) -> Value + Send + Sync + 'static,
    {
        self.steps.push(ChainStep {
            action: action.into(),
            label: None,
            stop_on_failure: true,
            params: StepParams::Dynamic(Arc::new(f)),
        });
        self
    }

    /// Override the label of the last step added.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        if let Some(step) = self.steps.last_mut() {
            step.label = Some(label.into());
        }
        self
    }

    /// Mark the last step as non-fatal: a failure will be recorded but the
    /// chain will continue rather than aborting.
    #[must_use]
    pub fn continue_on_failure(mut self) -> Self {
        if let Some(step) = self.steps.last_mut() {
            step.stop_on_failure = false;
        }
        self
    }

    /// Register a global error handler that runs whenever any step fails.
    ///
    /// The closure receives the [`DispatchError`] and the current context,
    /// and returns an [`ErrorAction`] to either abort or continue the chain.
    /// If no handler is registered the chain uses the per-step `stop_on_failure`
    /// flag to decide.
    #[must_use]
    pub fn on_error<F>(mut self, f: F) -> Self
    where
        F: Fn(&DispatchError, &Value) -> ErrorAction + Send + Sync + 'static,
    {
        self.on_error = Some(Arc::new(f));
        self
    }

    /// Execute the chain against the given dispatcher.
    ///
    /// # Arguments
    ///
    /// - `dispatcher` — the dispatcher to route each step through.
    /// - `initial_context` — a JSON object with seed values available to all
    ///   steps via `{key}` interpolation or dynamic closures.
    ///
    /// # Errors
    ///
    /// Returns `Err` only if the chain has no steps. Individual step failures
    /// are recorded in [`ChainResult::steps`] and reflected in
    /// [`ChainResult::success`].
    pub fn run(
        self,
        dispatcher: &ActionDispatcher,
        initial_context: Value,
    ) -> Result<ChainResult, String> {
        if self.steps.is_empty() {
            return Err("ActionChain has no steps".into());
        }

        let mut context = initial_context;
        let mut step_results: Vec<ChainStepResult> = Vec::with_capacity(self.steps.len());
        let mut chain_success = true;
        let mut aborted_at: Option<usize> = None;

        for (idx, step) in self.steps.iter().enumerate() {
            let label = step.label.clone().unwrap_or_else(|| step.action.clone());

            // Resolve params
            let params = match &step.params {
                StepParams::Static(v) => interpolate(v, &context),
                StepParams::Dynamic(f) => f(&context),
            };

            // Dispatch
            let dispatch_result = dispatcher.dispatch(&step.action, params);

            match dispatch_result {
                Ok(res) => {
                    // Merge object output into context
                    if let Value::Object(ref map) = res.output {
                        merge_into_context(&mut context, map);
                    }
                    step_results.push(ChainStepResult {
                        index: idx,
                        action: step.action.clone(),
                        label,
                        success: true,
                        output: res.output,
                        error: None,
                    });
                }
                Err(ref err) => {
                    step_results.push(ChainStepResult {
                        index: idx,
                        action: step.action.clone(),
                        label: label.clone(),
                        success: false,
                        output: Value::Null,
                        error: Some(err.to_string()),
                    });

                    // Decide whether to abort
                    let should_abort = if let Some(handler) = &self.on_error {
                        handler(err, &context) == ErrorAction::Abort
                    } else {
                        step.stop_on_failure
                    };

                    if should_abort {
                        chain_success = false;
                        aborted_at = Some(idx);
                        break;
                    }
                }
            }
        }

        let completed = step_results.len();
        let total = self.steps.len();
        let message = if chain_success {
            format!("Chain completed: {completed}/{total} steps succeeded.")
        } else if let Some(at) = aborted_at {
            let err_msg = step_results[at].error.as_deref().unwrap_or("unknown error");
            format!(
                "Chain aborted at step {at} ('{}'): {err_msg}",
                step_results[at].label
            )
        } else {
            format!("Chain finished with failures: {completed}/{total} steps executed.")
        };

        Ok(ChainResult {
            success: chain_success,
            steps: step_results,
            context,
            aborted_at,
            message,
        })
    }
}
