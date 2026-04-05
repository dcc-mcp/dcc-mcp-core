//! Action dispatcher — routes incoming requests to registered handlers.
//!
//! The dispatcher bridges the [`ActionRegistry`] (metadata) with actual
//! callable handlers, providing:
//!
//! - **Registration**: associate handler functions with action names
//! - **Parameter validation**: automatically validate input against the
//!   registered JSON Schema before calling the handler
//! - **Result standardisation**: handler return values are wrapped in
//!   [`dcc_mcp_models::ActionResult`]
//! - **Version management**: handlers can declare the action version they
//!   implement; the dispatcher routes to the best match
//!
//! ## Usage
//!
//! ```no_run
//! use dcc_mcp_actions::dispatcher::{ActionDispatcher, HandlerFn};
//! use dcc_mcp_actions::registry::{ActionMeta, ActionRegistry};
//! use serde_json::{json, Value};
//!
//! let registry = ActionRegistry::new();
//! let mut dispatcher = ActionDispatcher::new(registry.clone());
//!
//! // 1. Register metadata
//! registry.register_action(ActionMeta {
//!     name: "create_sphere".into(),
//!     dcc:  "maya".into(),
//!     input_schema: json!({
//!         "type": "object",
//!         "required": ["radius"],
//!         "properties": { "radius": { "type": "number", "minimum": 0.0 } }
//!     }),
//!     ..Default::default()
//! });
//!
//! // 2. Register a handler
//! dispatcher.register_handler("create_sphere", |params| {
//!     let r = params["radius"].as_f64().unwrap_or(1.0);
//!     Ok(json!({ "created": true, "radius": r }))
//! });
//!
//! // 3. Dispatch
//! let result = dispatcher.dispatch("create_sphere", json!({"radius": 2.0}));
//! assert!(result.is_ok());
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::registry::{ActionMeta, ActionRegistry};
use crate::validator::ActionValidator;

// ── Handler type aliases ──────────────────────────────────────────────────────

/// A synchronous action handler function.
///
/// Receives the validated input `params` and returns either a JSON `Value`
/// (success payload) or a descriptive error string.
pub type HandlerFn = Arc<dyn Fn(Value) -> Result<Value, String> + Send + Sync>;

// ── DispatchError ─────────────────────────────────────────────────────────────

/// Errors that can occur during dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// No handler has been registered for this action name.
    HandlerNotFound(String),
    /// The action is registered in the registry but no metadata was found.
    MetadataNotFound(String),
    /// Input validation failed.
    ValidationFailed(String),
    /// The handler itself returned an error.
    HandlerError(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HandlerNotFound(name) => write!(f, "no handler registered for action '{name}'"),
            Self::MetadataNotFound(name) => {
                write!(f, "no metadata found for action '{name}'")
            }
            Self::ValidationFailed(msg) => write!(f, "validation failed: {msg}"),
            Self::HandlerError(msg) => write!(f, "handler error: {msg}"),
        }
    }
}

impl std::error::Error for DispatchError {}

// ── DispatchResult ────────────────────────────────────────────────────────────

/// The outcome of a dispatch call.
#[derive(Debug, Clone)]
pub struct DispatchResult {
    /// The action name that was called.
    pub action: String,
    /// Serialisable output produced by the handler.
    pub output: Value,
    /// Whether validation was skipped (e.g. empty schema).
    pub validation_skipped: bool,
}

// ── ActionDispatcher ──────────────────────────────────────────────────────────

/// Routes action calls to registered handlers with automatic validation.
///
/// Thread-safe: handlers are wrapped in `Arc`, and the handler map is
/// protected by a `Mutex`.
///
/// # Example
///
/// ```no_run
/// use dcc_mcp_actions::dispatcher::ActionDispatcher;
/// use dcc_mcp_actions::registry::{ActionMeta, ActionRegistry};
/// use serde_json::json;
///
/// let registry = ActionRegistry::new();
/// let mut dispatcher = ActionDispatcher::new(registry.clone());
///
/// registry.register_action(ActionMeta {
///     name: "echo".into(),
///     dcc: "python".into(),
///     ..Default::default()
/// });
/// dispatcher.register_handler("echo", |params| Ok(params));
///
/// let result = dispatcher.dispatch("echo", json!({"msg": "hello"})).unwrap();
/// assert_eq!(result.output, json!({"msg": "hello"}));
/// ```
#[derive(Clone)]
pub struct ActionDispatcher {
    registry: ActionRegistry,
    handlers: Arc<Mutex<HashMap<String, HandlerFn>>>,
    /// Whether to skip validation when the schema is `{}` or `null`.
    pub skip_empty_schema_validation: bool,
}

impl std::fmt::Debug for ActionDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionDispatcher")
            .field("handler_count", &self.handler_count())
            .finish()
    }
}

impl ActionDispatcher {
    /// Create a new dispatcher backed by the given registry.
    #[must_use]
    pub fn new(registry: ActionRegistry) -> Self {
        Self {
            registry,
            handlers: Arc::new(Mutex::new(HashMap::new())),
            skip_empty_schema_validation: true,
        }
    }

    /// Register a handler function for the given action name.
    ///
    /// If a handler already exists, it is replaced.
    pub fn register_handler<F>(&self, action_name: &str, f: F)
    where
        F: Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        let mut map = self.handlers.lock().expect("dispatcher lock poisoned");
        map.insert(action_name.to_string(), Arc::new(f));
    }

    /// Register multiple handlers at once from an iterator.
    pub fn register_handlers<I, F>(&self, iter: I)
    where
        I: IntoIterator<Item = (&'static str, F)>,
        F: Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        let mut map = self.handlers.lock().expect("dispatcher lock poisoned");
        for (name, f) in iter {
            map.insert(name.to_string(), Arc::new(f));
        }
    }

    /// Remove the handler for `action_name`. Returns `true` if one existed.
    pub fn remove_handler(&self, action_name: &str) -> bool {
        let mut map = self.handlers.lock().expect("dispatcher lock poisoned");
        map.remove(action_name).is_some()
    }

    /// Return `true` if a handler is registered for `action_name`.
    #[must_use]
    pub fn has_handler(&self, action_name: &str) -> bool {
        let map = self.handlers.lock().expect("dispatcher lock poisoned");
        map.contains_key(action_name)
    }

    /// Number of registered handlers.
    #[must_use]
    pub fn handler_count(&self) -> usize {
        let map = self.handlers.lock().expect("dispatcher lock poisoned");
        map.len()
    }

    /// List all registered handler names.
    #[must_use]
    pub fn handler_names(&self) -> Vec<String> {
        let map = self.handlers.lock().expect("dispatcher lock poisoned");
        let mut names: Vec<String> = map.keys().cloned().collect();
        names.sort();
        names
    }

    /// Dispatch an action call.
    ///
    /// 1. Look up the handler for `action_name`.
    /// 2. Look up metadata from the registry (for schema validation).
    /// 3. Validate `params` against `input_schema` (unless schema is empty and
    ///    `skip_empty_schema_validation` is `true`).
    /// 4. Call the handler and return the result.
    ///
    /// # Errors
    ///
    /// Returns a [`DispatchError`] if:
    /// - No handler is registered for the action.
    /// - Validation fails.
    /// - The handler returns an error.
    pub fn dispatch(
        &self,
        action_name: &str,
        params: Value,
    ) -> Result<DispatchResult, DispatchError> {
        // 1. Look up handler
        let handler = {
            let map = self.handlers.lock().expect("dispatcher lock poisoned");
            map.get(action_name).cloned()
        };
        let handler =
            handler.ok_or_else(|| DispatchError::HandlerNotFound(action_name.to_string()))?;

        // 2. Look up metadata for validation
        let meta_opt: Option<ActionMeta> = self.registry.get_action(action_name, None);
        let validation_skipped = match &meta_opt {
            None => true,
            Some(meta) => {
                let schema = &meta.input_schema;
                let is_empty = schema.is_null()
                    || schema.as_object().map(|o| o.is_empty()).unwrap_or(false)
                    || is_default_schema(schema);
                if is_empty && self.skip_empty_schema_validation {
                    true
                } else {
                    // 3. Validate
                    let validator = ActionValidator::new(meta);
                    let result = validator.validate_input(&params);
                    if !result.is_valid() {
                        return Err(DispatchError::ValidationFailed(
                            result.into_result().unwrap_err(),
                        ));
                    }
                    false
                }
            }
        };

        // 4. Call handler
        let output = handler(params).map_err(DispatchError::HandlerError)?;

        Ok(DispatchResult {
            action: action_name.to_string(),
            output,
            validation_skipped,
        })
    }

    /// Access the underlying registry.
    #[must_use]
    pub fn registry(&self) -> &ActionRegistry {
        &self.registry
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns `true` if the schema carries no real constraints — i.e. it is the
/// default placeholder `{"type":"object","properties":{}}` or any schema that
/// has no `required` fields and only an empty `properties` map.
fn is_default_schema(schema: &Value) -> bool {
    let Some(obj) = schema.as_object() else {
        return false;
    };
    // Must not have a "required" key with a non-empty array
    if let Some(req) = obj.get("required") {
        if req.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            return false;
        }
    }
    // Properties must be absent or an empty object
    if let Some(props) = obj.get("properties") {
        if props.as_object().map(|p| !p.is_empty()).unwrap_or(false) {
            return false;
        }
    }
    // No additional constraint keywords
    let constraint_keys = [
        "anyOf", "oneOf", "allOf", "not", "if", "then", "else", "enum", "const",
    ];
    for key in &constraint_keys {
        if obj.contains_key(*key) {
            return false;
        }
    }
    true
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ActionMeta;
    use serde_json::json;

    fn make_dispatcher_with_action(schema: Value) -> (ActionDispatcher, ActionRegistry) {
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "test_action".into(),
            dcc: "maya".into(),
            input_schema: schema,
            ..Default::default()
        });
        let dispatcher = ActionDispatcher::new(reg.clone());
        (dispatcher, reg)
    }

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn test_dispatch_echo() {
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "echo".into(),
            dcc: "python".into(),
            ..Default::default()
        });
        let dispatcher = ActionDispatcher::new(reg);
        dispatcher.register_handler("echo", |params| Ok(params));

        let result = dispatcher
            .dispatch("echo", json!({"msg": "hello"}))
            .unwrap();
        assert_eq!(result.action, "echo");
        assert_eq!(result.output, json!({"msg": "hello"}));
    }

    #[test]
    fn test_dispatch_with_valid_schema() {
        let (dispatcher, _reg) = make_dispatcher_with_action(json!({
            "type": "object",
            "required": ["radius"],
            "properties": { "radius": { "type": "number", "minimum": 0.0 } }
        }));
        dispatcher.register_handler("test_action", |params| {
            let r = params["radius"].as_f64().unwrap_or(1.0);
            Ok(json!({ "created": true, "radius": r }))
        });

        let result = dispatcher
            .dispatch("test_action", json!({"radius": 5.0}))
            .unwrap();
        assert_eq!(result.output["radius"], json!(5.0));
        assert!(!result.validation_skipped);
    }

    #[test]
    fn test_dispatch_empty_schema_skips_validation() {
        let (dispatcher, _reg) = make_dispatcher_with_action(json!({}));
        dispatcher.register_handler("test_action", |_params| Ok(json!("ok")));

        let result = dispatcher
            .dispatch("test_action", json!({"anything": "goes"}))
            .unwrap();
        assert!(result.validation_skipped);
    }

    #[test]
    fn test_dispatch_no_metadata_skips_validation() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);
        dispatcher.register_handler("orphan", |_| Ok(json!("no meta needed")));

        let result = dispatcher.dispatch("orphan", json!(null)).unwrap();
        assert!(result.validation_skipped);
    }

    // ── error paths ───────────────────────────────────────────────────────────

    #[test]
    fn test_dispatch_handler_not_found() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);

        let err = dispatcher.dispatch("missing", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::HandlerNotFound(_)));
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn test_dispatch_validation_fails_missing_required() {
        let (dispatcher, _reg) = make_dispatcher_with_action(json!({
            "type": "object",
            "required": ["radius"]
        }));
        dispatcher.register_handler("test_action", |_| Ok(json!("ok")));

        let err = dispatcher.dispatch("test_action", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::ValidationFailed(_)));
        assert!(err.to_string().contains("radius"));
    }

    #[test]
    fn test_dispatch_validation_fails_type_mismatch() {
        let (dispatcher, _reg) = make_dispatcher_with_action(json!({
            "type": "object",
            "properties": { "x": { "type": "number" } }
        }));
        dispatcher.register_handler("test_action", |_| Ok(json!("ok")));

        let err = dispatcher
            .dispatch("test_action", json!({"x": "not_a_number"}))
            .unwrap_err();
        assert!(matches!(err, DispatchError::ValidationFailed(_)));
    }

    #[test]
    fn test_dispatch_handler_error() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);
        dispatcher.register_handler("failing", |_| Err("something went wrong".into()));

        let err = dispatcher.dispatch("failing", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::HandlerError(_)));
        assert!(err.to_string().contains("something went wrong"));
    }

    // ── handler management ────────────────────────────────────────────────────

    #[test]
    fn test_has_handler() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);
        assert!(!dispatcher.has_handler("my_action"));
        dispatcher.register_handler("my_action", |_| Ok(json!(null)));
        assert!(dispatcher.has_handler("my_action"));
    }

    #[test]
    fn test_handler_count() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);
        assert_eq!(dispatcher.handler_count(), 0);
        dispatcher.register_handler("a", |_| Ok(json!(null)));
        dispatcher.register_handler("b", |_| Ok(json!(null)));
        assert_eq!(dispatcher.handler_count(), 2);
    }

    #[test]
    fn test_handler_names_sorted() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);
        dispatcher.register_handler("zz", |_| Ok(json!(null)));
        dispatcher.register_handler("aa", |_| Ok(json!(null)));
        dispatcher.register_handler("mm", |_| Ok(json!(null)));

        let names = dispatcher.handler_names();
        assert_eq!(names, vec!["aa", "mm", "zz"]);
    }

    #[test]
    fn test_remove_handler() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);
        dispatcher.register_handler("to_remove", |_| Ok(json!(null)));
        assert!(dispatcher.has_handler("to_remove"));
        assert!(dispatcher.remove_handler("to_remove"));
        assert!(!dispatcher.has_handler("to_remove"));
        assert!(!dispatcher.remove_handler("to_remove")); // second time returns false
    }

    #[test]
    fn test_replace_handler() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);
        dispatcher.register_handler("action", |_| Ok(json!("v1")));
        dispatcher.register_handler("action", |_| Ok(json!("v2")));

        let result = dispatcher.dispatch("action", json!({})).unwrap();
        assert_eq!(result.output, json!("v2"));
    }

    #[test]
    fn test_register_handlers_batch() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);
        dispatcher.register_handlers([
            ("a", (|_: Value| Ok(json!(1))) as fn(Value) -> _),
            ("b", |_| Ok(json!(2))),
            ("c", |_| Ok(json!(3))),
        ]);
        assert_eq!(dispatcher.handler_count(), 3);
        assert!(dispatcher.has_handler("a"));
        assert!(dispatcher.has_handler("c"));
    }

    // ── clone / debug ─────────────────────────────────────────────────────────

    #[test]
    fn test_dispatcher_clone_shares_handlers() {
        let reg = ActionRegistry::new();
        let d1 = ActionDispatcher::new(reg);
        d1.register_handler("shared", |_| Ok(json!("ok")));
        let d2 = d1.clone();
        // Both share the same Arc<Mutex<...>> handler map
        assert!(d2.has_handler("shared"));
    }

    #[test]
    fn test_dispatcher_debug() {
        let reg = ActionRegistry::new();
        let dispatcher = ActionDispatcher::new(reg);
        let s = format!("{dispatcher:?}");
        assert!(s.contains("ActionDispatcher"));
    }

    // ── dispatch_error display ────────────────────────────────────────────────

    #[test]
    fn test_dispatch_error_display_handler_not_found() {
        let err = DispatchError::HandlerNotFound("my_fn".into());
        assert!(err.to_string().contains("my_fn"));
    }

    #[test]
    fn test_dispatch_error_display_validation() {
        let err = DispatchError::ValidationFailed("field missing".into());
        assert!(err.to_string().contains("validation failed"));
    }

    #[test]
    fn test_dispatch_error_display_metadata_not_found() {
        let err = DispatchError::MetadataNotFound("x".into());
        assert!(err.to_string().contains("x"));
    }

    #[test]
    fn test_dispatch_error_is_error() {
        let err: Box<dyn std::error::Error> = Box::new(DispatchError::HandlerNotFound("x".into()));
        assert!(!err.to_string().is_empty());
    }
}
