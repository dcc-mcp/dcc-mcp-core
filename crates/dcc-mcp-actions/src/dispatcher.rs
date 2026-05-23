//! Action dispatcher — routes incoming requests to registered handlers.
//!
//! The dispatcher bridges the [`ToolRegistry`] (metadata) with actual
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
//! use dcc_mcp_actions::dispatcher::{ToolDispatcher, HandlerFn};
//! use dcc_mcp_actions::registry::{ToolMeta, ToolRegistry};
//! use serde_json::{json, Value};
//!
//! let registry = ToolRegistry::new();
//! let mut dispatcher = ToolDispatcher::new(registry.clone());
//!
//! // 1. Register metadata
//! registry.register_action(ToolMeta {
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

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde_json::{Map, Value, json};

use dcc_mcp_models::ThreadAffinity;

use crate::events::EventBus;
use crate::registry::{ToolMeta, ToolRegistry};
use crate::validation_strategy::select_strategy;

// ── Handler type aliases ──────────────────────────────────────────────────────

/// A synchronous action handler function.
///
/// Receives the validated input `params` and returns either a JSON `Value`
/// (success payload) or a descriptive error string.
pub type HandlerFn = Arc<dyn Fn(Value) -> Result<Value, String> + Send + Sync>;

thread_local! {
    static CURRENT_THREAD_AFFINITY: Cell<ThreadAffinity> = const { Cell::new(ThreadAffinity::Any) };
}

/// Return the affinity declared for the current execution context.
#[must_use]
pub fn current_thread_affinity() -> ThreadAffinity {
    CURRENT_THREAD_AFFINITY.with(Cell::get)
}

/// Run `f` while marking the current execution context with `affinity`.
pub fn with_thread_affinity<R>(affinity: ThreadAffinity, f: impl FnOnce() -> R) -> R {
    CURRENT_THREAD_AFFINITY.with(|cell| {
        let previous = cell.replace(affinity);
        let result = f();
        cell.set(previous);
        result
    })
}

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
    /// The action exists but is currently disabled (inactive tool group).
    ActionDisabled { action: String, group: String },
    /// The action opted into runtime affinity enforcement and the observed
    /// execution context does not match its declaration.
    ThreadAffinityViolation {
        action: String,
        declared: ThreadAffinity,
        actual: ThreadAffinity,
    },
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
            Self::ActionDisabled { action, group } => write!(
                f,
                "action '{action}' is disabled (group '{group}' is inactive — call activate_tool_group first)"
            ),
            Self::ThreadAffinityViolation {
                action,
                declared,
                actual,
            } => write!(
                f,
                "THREAD_AFFINITY_VIOLATION: action '{action}' declared thread_affinity={declared} but ran on {actual}"
            ),
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

// ── ToolDispatcher ──────────────────────────────────────────────────────────

/// Routes action calls to registered handlers with automatic validation.
///
/// Thread-safe: handlers are wrapped in `Arc`, and the handler map is
/// protected by a `parking_lot::Mutex`.
///
/// # Example
///
/// ```no_run
/// use dcc_mcp_actions::dispatcher::ToolDispatcher;
/// use dcc_mcp_actions::registry::{ToolMeta, ToolRegistry};
/// use serde_json::json;
///
/// let registry = ToolRegistry::new();
/// let mut dispatcher = ToolDispatcher::new(registry.clone());
///
/// registry.register_action(ToolMeta {
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
pub struct ToolDispatcher {
    registry: ToolRegistry,
    handlers: Arc<Mutex<HashMap<String, HandlerFn>>>,
    event_bus: EventBus,
    /// Whether to skip validation when the schema is `{}` or `null`.
    pub skip_empty_schema_validation: bool,
}

impl std::fmt::Debug for ToolDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDispatcher")
            .field("handler_count", &self.handler_count())
            .finish()
    }
}

impl ToolDispatcher {
    /// Create a new dispatcher backed by the given registry.
    #[must_use]
    pub fn new(registry: ToolRegistry) -> Self {
        Self {
            registry,
            handlers: Arc::new(Mutex::new(HashMap::new())),
            event_bus: EventBus::new(),
            skip_empty_schema_validation: true,
        }
    }

    /// Create a dispatcher that emits lifecycle events on the supplied bus.
    #[must_use]
    pub fn with_event_bus(mut self, event_bus: EventBus) -> Self {
        self.event_bus = event_bus;
        self
    }

    /// Return the dispatcher event bus.
    #[must_use]
    pub fn event_bus(&self) -> EventBus {
        self.event_bus.clone()
    }

    /// Register a handler function for the given action name.
    ///
    /// If a handler already exists, it is replaced.
    pub fn register_handler<F>(&self, action_name: &str, f: F)
    where
        F: Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        let mut map = self.handlers.lock();
        map.insert(action_name.to_string(), Arc::new(f));
    }

    /// Register multiple handlers at once from an iterator.
    pub fn register_handlers<I, F>(&self, iter: I)
    where
        I: IntoIterator<Item = (&'static str, F)>,
        F: Fn(Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        let mut map = self.handlers.lock();
        for (name, f) in iter {
            map.insert(name.to_string(), Arc::new(f));
        }
    }

    /// Remove the handler for `action_name`. Returns `true` if one existed.
    pub fn remove_handler(&self, action_name: &str) -> bool {
        let mut map = self.handlers.lock();
        map.remove(action_name).is_some()
    }

    /// Return `true` if a handler is registered for `action_name`.
    #[must_use]
    pub fn has_handler(&self, action_name: &str) -> bool {
        let map = self.handlers.lock();
        map.contains_key(action_name)
    }

    /// Number of registered handlers.
    #[must_use]
    pub fn handler_count(&self) -> usize {
        let map = self.handlers.lock();
        map.len()
    }

    /// List all registered handler names.
    #[must_use]
    pub fn handler_names(&self) -> Vec<String> {
        let map = self.handlers.lock();
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
        self.dispatch_inner(action_name, params, true)
    }

    #[cfg_attr(not(feature = "python-bindings"), allow(dead_code))]
    pub(crate) fn dispatch_for_validation(
        &self,
        action_name: &str,
        params: Value,
    ) -> Result<DispatchResult, DispatchError> {
        self.dispatch_inner(action_name, params, false)
    }

    fn dispatch_inner(
        &self,
        action_name: &str,
        params: Value,
        emit_events: bool,
    ) -> Result<DispatchResult, DispatchError> {
        let started = Instant::now();
        // 1. Look up handler
        let handler = {
            let map = self.handlers.lock();
            map.get(action_name).cloned()
        };
        let Some(handler) = handler else {
            let err = DispatchError::HandlerNotFound(action_name.to_string());
            if emit_events {
                self.emit_tool_failed(action_name, None, &err, started);
            }
            return Err(err);
        };

        // 2. Metadata + progressive-exposure gate.
        let meta_opt: Option<ToolMeta> = self.registry.get_action(action_name, None);
        if let Some(meta) = &meta_opt {
            if !meta.enabled {
                let err = DispatchError::ActionDisabled {
                    action: action_name.to_string(),
                    group: meta.group.clone(),
                };
                if emit_events {
                    self.emit_tool_failed(action_name, meta_opt.as_ref(), &err, started);
                }
                return Err(err);
            }
            if meta.enforce_thread_affinity {
                let actual = current_thread_affinity();
                if actual != meta.thread_affinity {
                    let err = DispatchError::ThreadAffinityViolation {
                        action: action_name.to_string(),
                        declared: meta.thread_affinity,
                        actual,
                    };
                    if emit_events {
                        self.emit_tool_failed(action_name, meta_opt.as_ref(), &err, started);
                    }
                    return Err(err);
                }
            }
        }

        // 3. Validation via pluggable strategy (#493).
        let outcome = match select_strategy(meta_opt.as_ref(), self.skip_empty_schema_validation)
            .validate(&params)
        {
            Ok(outcome) => outcome,
            Err(msg) => {
                let err = DispatchError::ValidationFailed(msg);
                if emit_events {
                    self.emit_tool_failed(action_name, meta_opt.as_ref(), &err, started);
                }
                return Err(err);
            }
        };

        // 4. Call handler.
        if emit_events {
            self.emit_tool_event(
                "tool.dispatched",
                action_name,
                meta_opt.as_ref(),
                started,
                json!({
                    "validation_skipped": outcome.skipped,
                }),
            );
        }

        let output = match handler(params) {
            Ok(output) => output,
            Err(msg) => {
                let err = DispatchError::HandlerError(msg);
                if emit_events {
                    self.emit_tool_failed(action_name, meta_opt.as_ref(), &err, started);
                }
                return Err(err);
            }
        };

        if emit_events {
            let result_success = tool_result_success(&output);
            self.emit_tool_event(
                "tool.completed",
                action_name,
                meta_opt.as_ref(),
                started,
                json!({
                    "validation_skipped": outcome.skipped,
                    "result_success": result_success,
                }),
            );
        }

        Ok(DispatchResult {
            action: action_name.to_string(),
            output,
            validation_skipped: outcome.skipped,
        })
    }

    fn emit_tool_failed(
        &self,
        action_name: &str,
        meta: Option<&ToolMeta>,
        err: &DispatchError,
        started: Instant,
    ) {
        self.emit_tool_event(
            "tool.failed",
            action_name,
            meta,
            started,
            json!({
                "result_success": false,
                "error_kind": dispatch_error_kind(err),
                "error_message": err.to_string(),
            }),
        );
    }

    fn emit_tool_event(
        &self,
        event_name: &str,
        action_name: &str,
        meta: Option<&ToolMeta>,
        started: Instant,
        attributes: Value,
    ) {
        if !self.event_bus.has_subscribers(event_name) {
            return;
        }

        let mut source = Map::new();
        let mut attrs = attributes.as_object().cloned().unwrap_or_default();
        attrs.insert("tool_slug".to_string(), json!(action_name));
        attrs.insert("tool_name".to_string(), json!(action_name));
        attrs.insert(
            "duration_ms".to_string(),
            json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
        );

        if let Some(meta) = meta {
            if !meta.dcc.is_empty() {
                source.insert("dcc_type".to_string(), json!(meta.dcc));
                attrs.insert("dcc_type".to_string(), json!(meta.dcc));
            }
            if let Some(skill_name) = &meta.skill_name {
                attrs.insert("skill_name".to_string(), json!(skill_name));
            }
            if !meta.group.is_empty() {
                attrs.insert("group".to_string(), json!(meta.group));
            }
            attrs.insert(
                "annotations".to_string(),
                serde_json::to_value(&meta.annotations).unwrap_or(Value::Null),
            );
        }

        let _ = self.event_bus.emit(
            event_name,
            Value::Object(source),
            Value::Object(Map::new()),
            Value::Object(attrs),
        );
    }

    /// Access the underlying registry.
    #[must_use]
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }
}

pub(crate) fn dispatch_error_kind(err: &DispatchError) -> &'static str {
    match err {
        DispatchError::HandlerNotFound(_) => "handler_not_found",
        DispatchError::MetadataNotFound(_) => "metadata_not_found",
        DispatchError::ValidationFailed(_) => "validation_failed",
        DispatchError::HandlerError(_) => "handler_error",
        DispatchError::ActionDisabled { .. } => "action_disabled",
        DispatchError::ThreadAffinityViolation { .. } => "thread_affinity_violation",
    }
}

fn tool_result_success(output: &Value) -> bool {
    output
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns `true` if the schema carries no real constraints — i.e. it is the
/// default placeholder `{"type":"object","properties":{}}` or any schema that
/// has no `required` fields and only an empty `properties` map.
pub(crate) fn is_default_schema(schema: &Value) -> bool {
    let Some(obj) = schema.as_object() else {
        return false;
    };
    // Must not have a "required" key with a non-empty array
    if let Some(req) = obj.get("required")
        && req.as_array().map(|a| !a.is_empty()).unwrap_or(false)
    {
        return false;
    }
    // Properties must be absent or an empty object
    if let Some(props) = obj.get("properties")
        && props.as_object().map(|p| !p.is_empty()).unwrap_or(false)
    {
        return false;
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
    use crate::registry::ToolMeta;
    use serde_json::json;

    fn make_dispatcher_with_action(schema: Value) -> (ToolDispatcher, ToolRegistry) {
        let reg = ToolRegistry::new();
        reg.register_action(ToolMeta {
            name: "test_action".into(),
            dcc: "maya".into(),
            input_schema: schema,
            ..Default::default()
        });
        let dispatcher = ToolDispatcher::new(reg.clone());
        (dispatcher, reg)
    }

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn test_dispatch_echo() {
        let reg = ToolRegistry::new();
        reg.register_action(ToolMeta {
            name: "echo".into(),
            dcc: "python".into(),
            ..Default::default()
        });
        let dispatcher = ToolDispatcher::new(reg);
        dispatcher.register_handler("echo", Ok);

        let result = dispatcher
            .dispatch("echo", json!({"msg": "hello"}))
            .unwrap();
        assert_eq!(result.action, "echo");
        assert_eq!(result.output, json!({"msg": "hello"}));
    }

    #[test]
    fn test_dispatch_enforces_thread_affinity_when_opted_in() {
        let reg = ToolRegistry::new();
        reg.register_action(ToolMeta {
            name: "main_only".into(),
            dcc: "maya".into(),
            thread_affinity: ThreadAffinity::Main,
            enforce_thread_affinity: true,
            ..Default::default()
        });
        let dispatcher = ToolDispatcher::new(reg);
        dispatcher.register_handler("main_only", |_| Ok(json!({"ok": true})));

        let err = dispatcher.dispatch("main_only", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::ThreadAffinityViolation { .. }));
        assert!(err.to_string().contains("THREAD_AFFINITY_VIOLATION"));

        let result = with_thread_affinity(ThreadAffinity::Main, || {
            dispatcher.dispatch("main_only", json!({}))
        })
        .unwrap();
        assert_eq!(result.output, json!({"ok": true}));
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

    #[cfg(not(feature = "python-bindings"))]
    #[test]
    fn test_dispatch_emits_tool_lifecycle_events() {
        let reg = ToolRegistry::new();
        reg.register_action(ToolMeta {
            name: "echo".into(),
            dcc: "maya".into(),
            skill_name: Some("maya-core".into()),
            ..Default::default()
        });
        let dispatcher = ToolDispatcher::new(reg);
        dispatcher.register_handler("echo", Ok);

        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let _id = dispatcher
            .event_bus()
            .subscribe_event("tool.*".to_string(), move |event| {
                captured
                    .lock()
                    .unwrap()
                    .push((event.name.clone(), event.attributes.clone()));
            });

        let result = dispatcher
            .dispatch("echo", json!({"msg": "hello"}))
            .unwrap();
        assert_eq!(result.output, json!({"msg": "hello"}));

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "tool.dispatched");
        assert_eq!(events[0].1["tool_slug"], "echo");
        assert_eq!(events[0].1["skill_name"], "maya-core");
        assert_eq!(events[0].1["dcc_type"], "maya");
        assert_eq!(events[1].0, "tool.completed");
        assert_eq!(events[1].1["result_success"], true);
    }

    #[cfg(not(feature = "python-bindings"))]
    #[test]
    fn test_dispatch_completed_event_respects_success_false_payload() {
        let reg = ToolRegistry::new();
        reg.register_action(ToolMeta {
            name: "soft_fail".into(),
            dcc: "maya".into(),
            ..Default::default()
        });
        let dispatcher = ToolDispatcher::new(reg);
        dispatcher.register_handler("soft_fail", |_| {
            Ok(json!({
                "success": false,
                "message": "tool reported a domain failure"
            }))
        });

        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let _id = dispatcher
            .event_bus()
            .subscribe_event("tool.*".to_string(), move |event| {
                captured
                    .lock()
                    .unwrap()
                    .push((event.name.clone(), event.attributes.clone()));
            });

        let result = dispatcher.dispatch("soft_fail", json!({})).unwrap();
        assert_eq!(result.output["success"], false);

        let events = events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["tool.dispatched", "tool.completed"]
        );
        assert_eq!(events[1].1["result_success"], false);
    }

    #[cfg(not(feature = "python-bindings"))]
    #[test]
    fn test_dispatch_validation_failure_emits_tool_failed() {
        let (dispatcher, _reg) = make_dispatcher_with_action(json!({
            "type": "object",
            "required": ["radius"]
        }));
        dispatcher.register_handler("test_action", |_| Ok(json!("ok")));

        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let _id = dispatcher
            .event_bus()
            .subscribe_event("tool.*".to_string(), move |event| {
                captured
                    .lock()
                    .unwrap()
                    .push((event.name.clone(), event.attributes.clone()));
            });

        let err = dispatcher.dispatch("test_action", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::ValidationFailed(_)));

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "tool.failed");
        assert_eq!(events[0].1["tool_slug"], "test_action");
        assert_eq!(events[0].1["error_kind"], "validation_failed");
        assert_eq!(events[0].1["result_success"], false);
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
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);
        dispatcher.register_handler("orphan", |_| Ok(json!("no meta needed")));

        let result = dispatcher.dispatch("orphan", json!(null)).unwrap();
        assert!(result.validation_skipped);
    }

    // ── error paths ───────────────────────────────────────────────────────────

    #[test]
    fn test_dispatch_handler_not_found() {
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);

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
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);
        dispatcher.register_handler("failing", |_| Err("something went wrong".into()));

        let err = dispatcher.dispatch("failing", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::HandlerError(_)));
        assert!(err.to_string().contains("something went wrong"));
    }

    // ── handler management ────────────────────────────────────────────────────

    #[test]
    fn test_has_handler() {
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);
        assert!(!dispatcher.has_handler("my_action"));
        dispatcher.register_handler("my_action", |_| Ok(json!(null)));
        assert!(dispatcher.has_handler("my_action"));
    }

    #[test]
    fn test_handler_count() {
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);
        assert_eq!(dispatcher.handler_count(), 0);
        dispatcher.register_handler("a", |_| Ok(json!(null)));
        dispatcher.register_handler("b", |_| Ok(json!(null)));
        assert_eq!(dispatcher.handler_count(), 2);
    }

    #[test]
    fn test_handler_names_sorted() {
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);
        dispatcher.register_handler("zz", |_| Ok(json!(null)));
        dispatcher.register_handler("aa", |_| Ok(json!(null)));
        dispatcher.register_handler("mm", |_| Ok(json!(null)));

        let names = dispatcher.handler_names();
        assert_eq!(names, vec!["aa", "mm", "zz"]);
    }

    #[test]
    fn test_remove_handler() {
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);
        dispatcher.register_handler("to_remove", |_| Ok(json!(null)));
        assert!(dispatcher.has_handler("to_remove"));
        assert!(dispatcher.remove_handler("to_remove"));
        assert!(!dispatcher.has_handler("to_remove"));
        assert!(!dispatcher.remove_handler("to_remove")); // second time returns false
    }

    #[test]
    fn test_replace_handler() {
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);
        dispatcher.register_handler("action", |_| Ok(json!("v1")));
        dispatcher.register_handler("action", |_| Ok(json!("v2")));

        let result = dispatcher.dispatch("action", json!({})).unwrap();
        assert_eq!(result.output, json!("v2"));
    }

    #[test]
    fn test_register_handlers_batch() {
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);
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
        let reg = ToolRegistry::new();
        let d1 = ToolDispatcher::new(reg);
        d1.register_handler("shared", |_| Ok(json!("ok")));
        let d2 = d1.clone();
        // Both share the same Arc<Mutex<...>> handler map
        assert!(d2.has_handler("shared"));
    }

    #[test]
    fn test_dispatcher_debug() {
        let reg = ToolRegistry::new();
        let dispatcher = ToolDispatcher::new(reg);
        let s = format!("{dispatcher:?}");
        assert!(s.contains("ToolDispatcher"));
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
