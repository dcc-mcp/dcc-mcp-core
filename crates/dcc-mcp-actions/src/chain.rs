//! ActionChain — native multi-step operation orchestration.
//!
//! Provides a fluent builder API for composing sequences of action dispatches
//! where each step can receive the accumulated context from previous steps.
//!
//! # Design
//!
//! - Each step specifies an action name and either static params or a
//!   context-driven closure that computes params at runtime.
//! - `{key}` placeholders in static params are interpolated from the
//!   accumulated context (consistent with the `workflow` skill).
//! - An optional `on_error` handler can inspect failures and decide whether
//!   to abort or continue the chain.
//! - All dispatch is synchronous (matching the DCC main-thread constraint).
//!
//! # Example
//!
//! ```rust
//! use dcc_mcp_actions::chain::ActionChain;
//! use dcc_mcp_actions::dispatcher::{ActionDispatcher, DispatchError};
//! use dcc_mcp_actions::registry::{ActionMeta, ActionRegistry};
//! use serde_json::{json, Value};
//!
//! let registry = ActionRegistry::new();
//! registry.register_action(ActionMeta { name: "step_a".into(), dcc: "mock".into(), ..Default::default() });
//! registry.register_action(ActionMeta { name: "step_b".into(), dcc: "mock".into(), ..Default::default() });
//!
//! let dispatcher = ActionDispatcher::new(registry);
//! dispatcher.register_handler("step_a", |_| Ok(json!({"result_a": 42})));
//! dispatcher.register_handler("step_b", |params| Ok(json!({"got": params["result_a"]})));
//!
//! let result = ActionChain::new()
//!     .step("step_a", json!({}))
//!     .step("step_b", json!({"result_a": "{result_a}"}))
//!     .run(&dispatcher, json!({}))
//!     .unwrap();
//!
//! assert!(result.success);
//! assert_eq!(result.steps.len(), 2);
//! ```

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::dispatcher::{ActionDispatcher, DispatchError};

// ── Step param provider ───────────────────────────────────────────────────────

/// How params for a chain step are produced.
enum StepParams {
    /// Static JSON value; `{key}` placeholders are interpolated from context.
    Static(Value),
    /// Closure that receives the accumulated context and returns params.
    Dynamic(Arc<dyn Fn(&Value) -> Value + Send + Sync>),
}

// ── ChainStep ─────────────────────────────────────────────────────────────────

struct ChainStep {
    /// Action name to dispatch.
    action: String,
    /// Optional human-readable label for diagnostics.
    label: Option<String>,
    /// Whether a failure in this step aborts the remaining chain.
    stop_on_failure: bool,
    params: StepParams,
}

// ── ChainStepResult ───────────────────────────────────────────────────────────

/// Result of a single step in the chain execution.
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

/// The final outcome of running an [`ActionChain`].
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
type ErrorHandlerFn = Arc<dyn Fn(&DispatchError, &Value) -> ErrorAction + Send + Sync>;

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
/// Static params supplied via [`step`] may contain `{key}` string placeholders
/// which are replaced with values from the accumulated context before dispatch.
pub struct ActionChain {
    steps: Vec<ChainStep>,
    on_error: Option<ErrorHandlerFn>,
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

// ── Context helpers ───────────────────────────────────────────────────────────

/// Merge top-level keys from `src` into the context value.
///
/// If the context is not a JSON object, it is replaced by an object
/// containing only the merged keys.
fn merge_into_context(context: &mut Value, src: &Map<String, Value>) {
    if let Value::Object(map) = context {
        for (k, v) in src {
            map.insert(k.clone(), v.clone());
        }
    } else {
        let mut map = Map::new();
        for (k, v) in src {
            map.insert(k.clone(), v.clone());
        }
        *context = Value::Object(map);
    }
}

// ── Placeholder interpolation ─────────────────────────────────────────────────

/// Recursively replace `"{key}"` string segments with values from `context`.
///
/// Only simple `{word}` patterns are supported (consistent with the workflow
/// skill's `_interpolate` function). If the entire string matches a single
/// placeholder and the context value is not a string, the original JSON type
/// is returned (e.g. `"{count}"` → `Value::Number(42)`).
fn interpolate(value: &Value, context: &Value) -> Value {
    match value {
        Value::String(s) => interpolate_string(s, context),
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), interpolate(v, context));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| interpolate(v, context)).collect()),
        other => other.clone(),
    }
}

/// Interpolate `{key}` placeholders inside a string value.
///
/// If the entire string is a single `{key}` token, the context value is
/// returned directly (preserving its JSON type). Otherwise, string replacement
/// is performed using the `to_string()` representation of context values.
fn interpolate_string(s: &str, context: &Value) -> Value {
    // Fast path: entire string is a single placeholder
    if s.starts_with('{') && s.ends_with('}') && s.len() > 2 {
        let key = &s[1..s.len() - 1];
        // Only treat as placeholder if key has no inner braces
        if !key.contains('{') && !key.contains('}') {
            if let Some(v) = context.get(key) {
                return v.clone();
            }
        }
    }

    // General case: replace all {key} occurrences with string representations
    let mut result = String::with_capacity(s.len());
    let mut remaining = s;
    while let Some(open) = remaining.find('{') {
        result.push_str(&remaining[..open]);
        remaining = &remaining[open + 1..];
        if let Some(close) = remaining.find('}') {
            let key = &remaining[..close];
            remaining = &remaining[close + 1..];
            if !key.is_empty() && !key.contains('{') {
                if let Some(v) = context.get(key) {
                    result.push_str(&value_to_string(v));
                } else {
                    // Key not found: leave placeholder intact
                    result.push('{');
                    result.push_str(key);
                    result.push('}');
                }
            } else {
                result.push('{');
                result.push_str(key);
                result.push('}');
            }
        } else {
            // No closing brace found; emit the rest as-is
            result.push('{');
            result.push_str(remaining);
            remaining = "";
        }
    }
    result.push_str(remaining);
    Value::String(result)
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

// ── Convenience builder helpers ───────────────────────────────────────────────

/// Helper for building a context map for [`ActionChain::run`].
///
/// ```rust
/// use dcc_mcp_actions::chain::context;
/// let ctx = context([("export_path", "/tmp/out.fbx"), ("frame", "42")]);
/// ```
pub fn context<I, K, V>(entries: I) -> Value
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<Value>,
{
    let mut map = Map::new();
    for (k, v) in entries {
        map.insert(k.into(), v.into());
    }
    Value::Object(map)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::ActionDispatcher;
    use crate::registry::{ActionMeta, ActionRegistry};
    use serde_json::json;

    fn make_dispatcher() -> ActionDispatcher {
        let reg = ActionRegistry::new();
        ActionDispatcher::new(reg)
    }

    fn register(dispatcher: &ActionDispatcher, name: &'static str, output: Value) {
        dispatcher.register_handler(name, move |_| Ok(output.clone()));
    }

    // ── basic ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_single_step_success() {
        let d = make_dispatcher();
        register(&d, "ping", json!({"pong": true}));

        let result = ActionChain::new()
            .step("ping", json!({}))
            .run(&d, json!({}))
            .unwrap();

        assert!(result.success);
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].output, json!({"pong": true}));
    }

    #[test]
    fn test_two_steps_context_propagation() {
        let d = make_dispatcher();
        // step_a outputs {value: 99}; step_b receives it via interpolation
        register(&d, "step_a", json!({"value": 99}));
        d.register_handler("step_b", |params| Ok(json!({"received": params["value"]})));

        let result = ActionChain::new()
            .step("step_a", json!({}))
            .step("step_b", json!({"value": "{value}"}))
            .run(&d, json!({}))
            .unwrap();

        assert!(result.success);
        assert_eq!(result.steps[1].output, json!({"received": 99}));
    }

    #[test]
    fn test_initial_context_available() {
        let d = make_dispatcher();
        d.register_handler("use_ctx", |params| {
            Ok(json!({"path": params["export_path"]}))
        });

        let result = ActionChain::new()
            .step("use_ctx", json!({"export_path": "{export_path}"}))
            .run(&d, json!({"export_path": "/tmp/out.fbx"}))
            .unwrap();

        assert!(result.success);
        assert_eq!(result.steps[0].output["path"], json!("/tmp/out.fbx"));
    }

    // ── error handling ────────────────────────────────────────────────────────

    #[test]
    fn test_step_failure_aborts_by_default() {
        let d = make_dispatcher();
        register(&d, "ok_step", json!({}));
        // "bad_step" has no handler — will fail with HandlerNotFound

        let result = ActionChain::new()
            .step("bad_step", json!({}))
            .step("ok_step", json!({}))
            .run(&d, json!({}))
            .unwrap();

        assert!(!result.success);
        assert_eq!(result.aborted_at, Some(0));
        assert_eq!(result.steps.len(), 1); // second step never ran
    }

    #[test]
    fn test_continue_on_failure() {
        let d = make_dispatcher();
        register(&d, "ok_step", json!({"ran": true}));

        let result = ActionChain::new()
            .step("missing_action", json!({}))
            .continue_on_failure()
            .step("ok_step", json!({}))
            .run(&d, json!({}))
            .unwrap();

        // Chain didn't abort; both steps ran
        assert_eq!(result.steps.len(), 2);
        assert!(!result.steps[0].success);
        assert!(result.steps[1].success);
        // Overall success is true because abort never triggered
        assert!(result.success);
    }

    #[test]
    fn test_on_error_abort() {
        let d = make_dispatcher();
        register(&d, "ok_step", json!({}));

        let result = ActionChain::new()
            .step("missing", json!({}))
            .step("ok_step", json!({}))
            .on_error(|_, _| ErrorAction::Abort)
            .run(&d, json!({}))
            .unwrap();

        assert!(!result.success);
        assert_eq!(result.aborted_at, Some(0));
    }

    #[test]
    fn test_on_error_continue() {
        let d = make_dispatcher();
        register(&d, "ok_step", json!({"ran": true}));

        let result = ActionChain::new()
            .step("missing", json!({}))
            .step("ok_step", json!({}))
            .on_error(|_, _| ErrorAction::Continue)
            .run(&d, json!({}))
            .unwrap();

        assert_eq!(result.steps.len(), 2);
        assert!(result.steps[1].success);
        assert!(result.success);
    }

    // ── dynamic steps ─────────────────────────────────────────────────────────

    #[test]
    fn test_step_with_closure() {
        let d = make_dispatcher();
        register(&d, "step_a", json!({"items": ["a", "b", "c"]}));
        d.register_handler("step_b", |params: Value| {
            let count = params["items"].as_array().map(|a| a.len()).unwrap_or(0);
            Ok(json!({"count": count}))
        });

        let result = ActionChain::new()
            .step("step_a", json!({}))
            .step_with(
                "step_b",
                |ctx| json!({"items": ctx.get("items").cloned().unwrap_or(json!([]))}),
            )
            .run(&d, json!({}))
            .unwrap();

        assert!(result.success);
        assert_eq!(result.steps[1].output["count"], json!(3));
    }

    // ── empty chain error ─────────────────────────────────────────────────────

    #[test]
    fn test_empty_chain_returns_error() {
        let d = make_dispatcher();
        let err = ActionChain::new().run(&d, json!({})).unwrap_err();
        assert!(err.contains("no steps"));
    }

    // ── interpolation ─────────────────────────────────────────────────────────

    #[test]
    fn test_interpolate_whole_placeholder_preserves_type() {
        let ctx = json!({"count": 42});
        let v = interpolate(&json!("{count}"), &ctx);
        assert_eq!(v, json!(42));
    }

    #[test]
    fn test_interpolate_inline_becomes_string() {
        let ctx = json!({"name": "world"});
        let v = interpolate(&json!("hello {name}!"), &ctx);
        assert_eq!(v, json!("hello world!"));
    }

    #[test]
    fn test_interpolate_missing_key_unchanged() {
        let ctx = json!({});
        let v = interpolate(&json!("{missing}"), &ctx);
        assert_eq!(v, json!("{missing}"));
    }

    #[test]
    fn test_interpolate_nested_object() {
        let ctx = json!({"prefix": "char_"});
        let v = interpolate(&json!({"name": "{prefix}hero"}), &ctx);
        assert_eq!(v, json!({"name": "char_hero"}));
    }

    // ── context helper ────────────────────────────────────────────────────────

    #[test]
    fn test_context_helper() {
        let ctx = context([("key", "val"), ("num", "99")]);
        assert_eq!(ctx["key"], json!("val"));
        assert_eq!(ctx["num"], json!("99"));
    }

    // ── label / message ───────────────────────────────────────────────────────

    #[test]
    fn test_label_appears_in_result() {
        let d = make_dispatcher();
        register(&d, "my_action", json!({}));

        let result = ActionChain::new()
            .step("my_action", json!({}))
            .label("Do the thing")
            .run(&d, json!({}))
            .unwrap();

        assert_eq!(result.steps[0].label, "Do the thing");
    }

    #[test]
    fn test_message_on_success() {
        let d = make_dispatcher();
        register(&d, "a", json!({}));
        register(&d, "b", json!({}));

        let result = ActionChain::new()
            .step("a", json!({}))
            .step("b", json!({}))
            .run(&d, json!({}))
            .unwrap();

        assert!(result.message.contains("2/2"));
    }

    #[test]
    fn test_message_on_abort() {
        let d = make_dispatcher();

        let result = ActionChain::new()
            .step("missing", json!({}))
            .run(&d, json!({}))
            .unwrap();

        assert!(!result.success);
        assert!(result.message.contains("aborted"));
    }

    // ── registry integration ──────────────────────────────────────────────────

    #[test]
    fn test_with_registered_action_metadata() {
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "validated".into(),
            dcc: "mock".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["x"],
                "properties": {"x": {"type": "number"}}
            }),
            ..Default::default()
        });
        let d = ActionDispatcher::new(reg);
        d.register_handler("validated", |p| {
            Ok(json!({"doubled": p["x"].as_f64().unwrap_or(0.0) * 2.0}))
        });

        let result = ActionChain::new()
            .step("validated", json!({"x": 5.0}))
            .run(&d, json!({}))
            .unwrap();

        assert!(result.success);
        assert_eq!(result.steps[0].output["doubled"], json!(10.0));
    }
}
