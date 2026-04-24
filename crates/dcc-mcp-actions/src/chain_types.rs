//! Step and result types used by [`ActionChain`](super::ActionChain).

use std::sync::Arc;

use serde_json::Value;

use crate::dispatcher::DispatchError;

// ── Step param provider ───────────────────────────────────────────────────────

/// How params for a chain step are produced.
pub(crate) enum StepParams {
    /// Static JSON value; `{key}` placeholders are interpolated from context.
    Static(Value),
    /// Closure that receives the accumulated context and returns params.
    Dynamic(Arc<dyn Fn(&Value) -> Value + Send + Sync>),
}

// ── ChainStep ─────────────────────────────────────────────────────────────────

/// One step in an [`ActionChain`](super::ActionChain).
pub(crate) struct ChainStep {
    /// Action name to dispatch.
    pub(crate) action: String,
    /// Optional human-readable label for diagnostics.
    pub(crate) label: Option<String>,
    /// Whether a failure in this step aborts the remaining chain.
    pub(crate) stop_on_failure: bool,
    pub(crate) params: StepParams,
}

// ── ChainStepResult ───────────────────────────────────────────────────────────

/// Result of a single step in the chain execution.
///
/// Produced by [`ActionChain::run`](super::ActionChain::run).
#[derive(Debug, Clone)]
pub struct ChainStepResult {
    /// Zero-based step index.
    pub index: usize,
    /// Action name.
    pub action: String,
    /// Human-readable label (falls back to action name).
    pub label: String,
    /// Whether this step succeeded.
    pub success: bool,
    /// The output value returned by the handler.
    pub output: Value,
    /// Error message if this step failed.
    pub error: Option<String>,
}

// ── ChainResult ───────────────────────────────────────────────────────────────

/// The final outcome of running an [`ActionChain`](super::ActionChain).
#[derive(Debug, Clone)]
pub struct ChainResult {
    /// `true` if all required steps succeeded.
    pub success: bool,
    /// Per-step results (only for steps that were executed).
    pub steps: Vec<ChainStepResult>,
    /// Accumulated context after all steps.
    pub context: Value,
    /// Index of the step that caused the chain to abort, if any.
    pub aborted_at: Option<usize>,
    /// Human-readable summary message.
    pub message: String,
}

// ── ErrorAction ───────────────────────────────────────────────────────────────

/// Decision returned by the `on_error` handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorAction {
    /// Abort the chain immediately.
    Abort,
    /// Skip this step and continue with the next.
    Continue,
}

// ── Type alias ────────────────────────────────────────────────────────────────

/// Signature for chain-level error handlers.
pub(crate) type ErrorHandlerFn = Arc<dyn Fn(&DispatchError, &Value) -> ErrorAction + Send + Sync>;
