//! PyO3 bindings for ToolValidator and ToolDispatcher.
//!
//! Exposed Python classes:
//! - [`PyToolValidator`] — validates JSON params against a JSON Schema string.
//! - [`PyToolDispatcher`] — routes action calls to Python callables with auto-validation.
//!
//! ## Design note
//!
//! `PyToolDispatcher` maintains its own `handler_map: HashMap<String, PyObject>` in
//! addition to the Rust-level `ToolDispatcher`.  When `dispatch()` is called:
//! 1. Look up the Python handler in `handler_map`.
//! 2. Perform schema validation via the Rust `ToolDispatcher` (bypass the handler call).
//! 3. Call the Python handler directly in the Python GIL.
//!
//! This sidesteps the pyo3 0.28 restriction that `Python::with_gil` was removed;
//! the GIL is already held inside `#[pymethods]` so no re-acquisition is needed.

mod events;
pub(crate) mod versioned;

use std::collections::HashMap;
use std::time::Instant;

use pyo3::Py;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use serde_json::{Map, Value, json};

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

use crate::dispatcher::{DispatchError, ToolDispatcher, dispatch_error_kind, tool_event_payload};
use crate::events::EventBus;
use crate::registry::{ToolMeta, ToolRegistry};
use crate::validator::ToolValidator;

// ── PyToolValidator ─────────────────────────────────────────────────────────

/// Validates JSON-encoded action parameters against a JSON Schema.
///
/// Create with :meth:`from_schema_json` or :meth:`from_action_registry`.
///
/// Example::
///
///     import json
///     from dcc_mcp_core import ToolRegistry, ToolValidator
///
///     schema = json.dumps({
///         "type": "object",
///         "required": ["radius"],
///         "properties": {"radius": {"type": "number", "minimum": 0.0}}
///     })
///     v = ToolValidator.from_schema_json(schema)
///     ok, errors = v.validate('{"radius": 1.0}')
///     assert ok
///     ok, errors = v.validate("{}")
///     assert not ok
///
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "ToolValidator")]
pub struct PyToolValidator {
    inner: ToolValidator,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyToolValidator {
    /// Create a validator from a JSON Schema string.
    ///
    /// Raises:
    ///     ValueError: If ``schema_json`` is not valid JSON.
    #[staticmethod]
    pub fn from_schema_json(schema_json: &str) -> PyResult<Self> {
        let schema: Value = serde_json::from_str(schema_json)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid JSON: {e}")))?;
        Ok(Self {
            inner: ToolValidator::from_schema(schema),
        })
    }

    /// Create a validator from an action in an :class:`ToolRegistry`.
    ///
    /// Raises:
    ///     KeyError: If the action is not found in the registry.
    #[staticmethod]
    #[pyo3(signature = (registry, action_name, dcc_name = None))]
    pub fn from_action_registry(
        registry: &ToolRegistry,
        action_name: &str,
        dcc_name: Option<&str>,
    ) -> PyResult<Self> {
        let meta: Option<ToolMeta> = registry.get_action(action_name, dcc_name);
        let meta = meta.ok_or_else(|| {
            pyo3::exceptions::PyKeyError::new_err(format!(
                "action '{action_name}' not found in registry"
            ))
        })?;
        Ok(Self {
            inner: ToolValidator::new(&meta),
        })
    }

    /// Validate JSON-encoded parameters.
    ///
    /// Returns:
    ///     ``(bool, list[str])`` — True + empty list on success; False + errors on failure.
    ///
    /// Raises:
    ///     ValueError: If ``params_json`` is not valid JSON.
    #[pyo3(signature = (params_json))]
    pub fn validate(&self, params_json: &str) -> PyResult<(bool, Vec<String>)> {
        let params: Value = serde_json::from_str(params_json)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid JSON: {e}")))?;
        let result = self.inner.validate_input(&params);
        let errors: Vec<String> = result.errors.iter().map(|e| e.to_string()).collect();
        Ok((errors.is_empty(), errors))
    }

    fn __repr__(&self) -> String {
        "ToolValidator()".to_string()
    }
}

// ── PyToolDispatcher ────────────────────────────────────────────────────────

/// Routes action calls to registered Python callables with automatic validation.
///
/// Example::
///
///     import json
///     from dcc_mcp_core import ToolRegistry, ToolDispatcher
///
///     reg = ToolRegistry()
///     reg.register(
///         "create_sphere",
///         input_schema=json.dumps({
///             "type": "object",
///             "required": ["radius"],
///             "properties": {"radius": {"type": "number", "minimum": 0.0}},
///         }),
///     )
///     dispatcher = ToolDispatcher(reg)
///
///     def create_sphere(params):
///         return {"created": True, "radius": params["radius"]}
///
///     dispatcher.register_handler("create_sphere", create_sphere)
///     result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0}))
///     assert result["output"]["created"] is True
///
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "ToolDispatcher")]
pub struct PyToolDispatcher {
    /// Rust dispatcher used for schema validation; handler calls are short-circuited.
    inner: ToolDispatcher,
    /// Map from action name → Python callable.
    handler_map: HashMap<String, Py<PyAny>>,
    /// Whether to skip validation when the action schema is empty.
    pub skip_empty_schema_validation: bool,
}

impl PyToolDispatcher {
    fn emit_tool_failed(
        &self,
        action_name: &str,
        error_kind: &str,
        error_message: String,
        started: Instant,
    ) {
        self.emit_tool_event(
            "tool.failed",
            action_name,
            started,
            json!({
                "result_success": false,
                "error_kind": error_kind,
                "error_message": error_message,
            }),
        );
    }

    fn emit_tool_event(
        &self,
        event_name: &str,
        action_name: &str,
        started: Instant,
        attributes: Value,
    ) {
        let event_bus = self.inner.event_bus();
        if !event_bus.has_subscribers(event_name) {
            return;
        }

        let meta = self.inner.registry().get_action(action_name, None);
        let (source, attributes) =
            tool_event_payload(action_name, meta.as_ref(), started, attributes);

        let _ = event_bus.emit(event_name, source, Value::Object(Map::new()), attributes);
    }

    fn emit_vetoable_tool_event(
        &self,
        event_name: &str,
        action_name: &str,
        started: Instant,
        attributes: Value,
    ) -> Result<(), crate::events::EventVeto> {
        let event_bus = self.inner.event_bus();
        let meta = self.inner.registry().get_action(action_name, None);
        let (source, attributes) =
            tool_event_payload(action_name, meta.as_ref(), started, attributes);
        let event =
            event_bus.before_event(event_name, source, Value::Object(Map::new()), attributes)?;
        event_bus.publish_event(&event);
        Ok(())
    }
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyToolDispatcher {
    /// Create a new dispatcher backed by the given registry.
    #[new]
    pub fn new(registry: &ToolRegistry) -> Self {
        Self {
            inner: ToolDispatcher::new(registry.clone()),
            handler_map: HashMap::new(),
            skip_empty_schema_validation: true,
        }
    }

    /// Return the dispatcher event bus.
    #[pyo3(name = "event_bus")]
    pub fn py_event_bus(&self) -> EventBus {
        self.inner.event_bus()
    }

    /// Replace the dispatcher event bus.
    #[pyo3(name = "set_event_bus")]
    pub fn py_set_event_bus(&mut self, event_bus: EventBus) {
        self.inner = self.inner.clone().with_event_bus(event_bus);
    }

    /// Register a Python callable as the handler for ``action_name``.
    ///
    /// Raises:
    ///     TypeError: If ``handler`` is not callable.
    #[pyo3(signature = (action_name, handler))]
    pub fn register_handler(
        &mut self,
        py: Python<'_>,
        action_name: &str,
        handler: Py<PyAny>,
    ) -> PyResult<()> {
        if !handler.bind(py).is_callable() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "handler must be callable",
            ));
        }
        self.handler_map.insert(action_name.to_string(), handler);
        // Also register a stub in the Rust dispatcher for validation purposes.
        // The stub is never actually called; dispatch() calls the Python handler directly.
        self.inner
            .register_handler(action_name, |_| Ok(Value::Null));
        Ok(())
    }

    /// Remove the handler for ``action_name``.
    ///
    /// Returns ``True`` if a handler existed and was removed.
    #[pyo3(signature = (action_name))]
    pub fn remove_handler(&mut self, action_name: &str) -> bool {
        let removed = self.handler_map.remove(action_name).is_some();
        self.inner.remove_handler(action_name);
        removed
    }

    /// Return ``True`` if a handler is registered for ``action_name``.
    #[pyo3(signature = (action_name))]
    pub fn has_handler(&self, action_name: &str) -> bool {
        self.handler_map.contains_key(action_name)
    }

    /// Number of registered handlers.
    pub fn handler_count(&self) -> usize {
        self.handler_map.len()
    }

    /// Alphabetically sorted list of registered handler names.
    pub fn handler_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.handler_map.keys().cloned().collect();
        names.sort();
        names
    }

    /// Whether to skip validation when the action schema is empty.
    #[getter]
    pub fn get_skip_empty_schema_validation(&self) -> bool {
        self.skip_empty_schema_validation
    }

    #[setter]
    pub fn set_skip_empty_schema_validation(&mut self, value: bool) {
        self.skip_empty_schema_validation = value;
        self.inner.skip_empty_schema_validation = value;
    }

    /// Dispatch an action call.
    ///
    /// 1. Validates ``params_json`` against the action schema.
    /// 2. Calls the registered Python handler.
    /// 3. Returns a dict with ``"action"``, ``"output"``, ``"validation_skipped"``.
    ///
    /// Raises:
    ///     KeyError:     No handler registered for ``action_name``.
    ///     ValueError:   Invalid JSON or validation failure.
    ///     RuntimeError: Handler raised an exception.
    #[pyo3(signature = (action_name, params_json = "null", meta_json = "null"))]
    pub fn dispatch<'py>(
        &self,
        py: Python<'py>,
        action_name: &str,
        params_json: &str,
        meta_json: &str,
    ) -> PyResult<Bound<'py, PyDict>> {
        let started = Instant::now();
        // 1. Parse params
        let mut params: Value = serde_json::from_str(params_json)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid JSON: {e}")))?;
        // 1b. Parse optional meta (request-level context like agent_context)
        let meta: Option<Value> = if meta_json == "null" {
            None
        } else {
            Some(serde_json::from_str(meta_json).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("invalid meta JSON: {e}"))
            })?)
        };

        // 2. Validate via Rust dispatcher (the stub handler is registered there too)
        let validation_skipped = if !self.handler_map.contains_key(action_name) {
            self.emit_tool_failed(
                action_name,
                "handler_not_found",
                format!("no handler for '{action_name}'"),
                started,
            );
            return Err(pyo3::exceptions::PyKeyError::new_err(format!(
                "no handler for '{action_name}'"
            )));
        } else {
            // Use the Rust dispatcher only for validation
            match self
                .inner
                .dispatch_for_validation(action_name, params.clone())
            {
                Ok(r) => r.validation_skipped,
                Err(DispatchError::ValidationFailed(msg)) => {
                    self.emit_tool_failed(
                        action_name,
                        "validation_failed",
                        format!("validation failed: {msg}"),
                        started,
                    );
                    return Err(pyo3::exceptions::PyValueError::new_err(format!(
                        "validation failed: {msg}"
                    )));
                }
                Err(DispatchError::HandlerNotFound(_)) => {
                    // Stub not registered — treat as validation skipped
                    true
                }
                Err(DispatchError::MetadataNotFound(_)) => true,
                Err(DispatchError::HandlerError(_)) => {
                    // Stub always returns Ok(null), so this shouldn't happen
                    false
                }
                Err(err @ DispatchError::ActionDisabled { .. })
                | Err(err @ DispatchError::ThreadAffinityViolation { .. })
                | Err(err @ DispatchError::Vetoed { .. }) => {
                    self.emit_tool_failed(
                        action_name,
                        dispatch_error_kind(&err),
                        err.to_string(),
                        started,
                    );
                    return Err(pyo3::exceptions::PyPermissionError::new_err(
                        err.to_string(),
                    ));
                }
            }
        };

        // 2b. Inject _meta into params AFTER validation (safe for additionalProperties: false).
        if let Some(m) = meta {
            if let Value::Object(ref mut map) = params {
                map.insert("_meta".to_string(), m);
            }
        }

        // 3. Call the Python handler
        if let Err(veto) = self.emit_vetoable_tool_event(
            "tool.dispatched",
            action_name,
            started,
            json!({
                "validation_skipped": validation_skipped,
            }),
        ) {
            let message = format!(
                "EVENT_VETOED: action '{action_name}' was vetoed ({}): {}",
                veto.code, veto.reason
            );
            self.emit_tool_failed(action_name, "event_vetoed", message.clone(), started);
            return Err(pyo3::exceptions::PyPermissionError::new_err(message));
        }
        let handler = self.handler_map.get(action_name).expect("checked above");
        let py_params = value_to_py(py, &params)?;
        let raw = handler.call1(py, (py_params,)).map_err(|e| {
            self.emit_tool_failed(
                action_name,
                "handler_error",
                format!("handler error: {e}"),
                started,
            );
            pyo3::exceptions::PyRuntimeError::new_err(format!("handler error: {e}"))
        })?;

        let result_success = py_result_success(&raw, py);
        self.emit_tool_event(
            "tool.completed",
            action_name,
            started,
            json!({
                "validation_skipped": validation_skipped,
                "result_success": result_success,
            }),
        );

        // 4. Build result dict
        let d = PyDict::new(py);
        d.set_item("action", action_name)?;
        d.set_item("output", raw)?;
        d.set_item("validation_skipped", validation_skipped)?;
        Ok(d)
    }

    fn __repr__(&self) -> String {
        format!("ToolDispatcher(handlers={})", self.handler_map.len())
    }
}

// ── Internal helpers for PyToolPipeline ────────────────────────────────────

impl PyToolDispatcher {
    /// Return a clone of the underlying Rust registry (for pipeline construction).
    pub fn registry(&self) -> ToolRegistry {
        self.inner.registry().clone()
    }
    /// Return a clone of the Python handler map (for pipeline dispatch).
    ///
    /// Each `Py<PyAny>` is cloned via `clone_ref` which requires the GIL.
    pub fn handler_map_clone(&self) -> HashMap<String, Py<PyAny>> {
        Python::try_attach(|py| {
            self.handler_map
                .iter()
                .map(|(k, v)| (k.clone(), v.clone_ref(py)))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default()
    }
}

// ── Registration helper ───────────────────────────────────────────────────────

/// Register `PyToolValidator` and `PyToolDispatcher` on the given Python module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyToolValidator>()?;
    m.add_class::<PyToolDispatcher>()?;
    Ok(())
}

// ── Conversion helpers ────────────────────────────────────────────────────────

/// Convert a `serde_json::Value` to a Python object.
fn value_to_py(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => Ok((*b).into_pyobject(py)?.to_owned().into()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into())
            } else {
                Ok(n.to_string().into_pyobject(py)?.into())
            }
        }
        Value::String(s) => Ok(s.as_str().into_pyobject(py)?.into()),
        Value::Array(arr) => {
            let list = PyList::empty(py);
            for v in arr {
                list.append(value_to_py(py, v)?)?;
            }
            Ok(list.into())
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, value_to_py(py, v)?)?;
            }
            Ok(dict.into())
        }
    }
}

fn py_result_success(raw: &Py<PyAny>, py: Python<'_>) -> bool {
    raw.bind(py)
        .cast::<PyDict>()
        .ok()
        .and_then(|dict| dict.get_item("success").ok().flatten())
        .and_then(|value| value.extract::<bool>().ok())
        .unwrap_or(true)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolMeta;
    use serde_json::json;

    fn make_registry_with_schema(schema: Value) -> ToolRegistry {
        let reg = ToolRegistry::new();
        reg.register_action(ToolMeta {
            name: "test_action".into(),
            dcc: "maya".into(),
            input_schema: schema,
            ..Default::default()
        });
        reg
    }

    // ── ToolValidator (Rust) ────────────────────────────────────────────────

    #[test]
    fn test_validator_valid_schema() {
        let schema = json!({ "type": "object", "required": ["radius"],
            "properties": { "radius": { "type": "number", "minimum": 0.0 } } });
        let v = ToolValidator::from_schema(schema);
        assert!(v.validate_input(&json!({"radius": 1.0})).is_valid());
    }

    #[test]
    fn test_validator_missing_required() {
        let schema = json!({ "type": "object", "required": ["radius"] });
        let v = ToolValidator::from_schema(schema);
        let result = v.validate_input(&json!({}));
        assert!(!result.is_valid());
        assert!(result.errors[0].message.contains("radius"));
    }

    #[test]
    fn test_validator_from_registry() {
        let reg = make_registry_with_schema(json!({ "type": "object" }));
        let meta: Option<ToolMeta> = reg.get_action("test_action", None);
        assert!(meta.is_some());
        let v = ToolValidator::new(&meta.unwrap());
        assert!(v.validate_input(&json!({})).is_valid());
    }

    // ── ToolDispatcher (Rust) ───────────────────────────────────────────────

    #[test]
    fn test_dispatcher_new_empty() {
        let reg = ToolRegistry::new();
        let d = ToolDispatcher::new(reg);
        assert_eq!(d.handler_count(), 0);
        assert!(!d.has_handler("x"));
    }

    #[test]
    fn test_dispatcher_register_and_dispatch() {
        let reg = make_registry_with_schema(json!({}));
        let d = ToolDispatcher::new(reg);
        d.register_handler("test_action", |_| Ok(json!({"ok": true})));
        assert!(d.has_handler("test_action"));
        let result = d.dispatch("test_action", json!({})).unwrap();
        assert_eq!(result.action, "test_action");
        assert_eq!(result.output["ok"], json!(true));
    }

    #[test]
    fn test_dispatcher_handler_not_found() {
        let reg = ToolRegistry::new();
        let d = ToolDispatcher::new(reg);
        let err = d.dispatch("missing", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::HandlerNotFound(_)));
    }

    #[test]
    fn test_dispatcher_validation_fails() {
        let reg = make_registry_with_schema(json!({ "type": "object", "required": ["x"] }));
        let d = ToolDispatcher::new(reg);
        d.register_handler("test_action", |_| Ok(json!(null)));
        let err = d.dispatch("test_action", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::ValidationFailed(_)));
    }

    #[test]
    fn test_dispatcher_remove_handler() {
        let reg = ToolRegistry::new();
        let d = ToolDispatcher::new(reg);
        d.register_handler("a", |_| Ok(json!(null)));
        assert!(d.remove_handler("a"));
        assert!(!d.has_handler("a"));
        assert!(!d.remove_handler("a"));
    }

    #[test]
    fn test_dispatcher_handler_names_sorted() {
        let reg = ToolRegistry::new();
        let d = ToolDispatcher::new(reg);
        d.register_handler("z", |_| Ok(json!(null)));
        d.register_handler("a", |_| Ok(json!(null)));
        d.register_handler("m", |_| Ok(json!(null)));
        assert_eq!(d.handler_names(), vec!["a", "m", "z"]);
    }

    #[test]
    fn test_dispatcher_handler_error() {
        let reg = ToolRegistry::new();
        let d = ToolDispatcher::new(reg);
        d.register_handler("failing", |_| Err("oops".into()));
        let err = d.dispatch("failing", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::HandlerError(_)));
    }
}
