//! PyO3 bindings for ToolValidator and ToolDispatcher.
//!
//! Exposed Python classes:
//! - [`PyActionValidator`] — validates JSON params against a JSON Schema string.
//! - [`PyActionDispatcher`] — routes action calls to Python callables with auto-validation.
//!
//! ## Design note
//!
//! `PyActionDispatcher` maintains its own `handler_map: HashMap<String, PyObject>` in
//! addition to the Rust-level `ActionDispatcher`.  When `dispatch()` is called:
//! 1. Look up the Python handler in `handler_map`.
//! 2. Perform schema validation via the Rust `ActionDispatcher` (bypass the handler call).
//! 3. Call the Python handler directly in the Python GIL.
//!
//! This sidesteps the pyo3 0.28 restriction that `Python::with_gil` was removed;
//! the GIL is already held inside `#[pymethods]` so no re-acquisition is needed.

use std::collections::HashMap;

use pyo3::Py;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use serde_json::Value;

use crate::dispatcher::{ActionDispatcher, DispatchError};
use crate::registry::{ActionMeta, ActionRegistry};
use crate::validator::ActionValidator;

// ── PyActionValidator ─────────────────────────────────────────────────────────

/// Validates JSON-encoded action parameters against a JSON Schema.
///
/// Create with :meth:`from_schema_json` or :meth:`from_action_registry`.
///
/// Example::
///
///     import json
///     from dcc_mcp_core import ActionRegistry, ActionValidator
///
///     schema = json.dumps({
///         "type": "object",
///         "required": ["radius"],
///         "properties": {"radius": {"type": "number", "minimum": 0.0}}
///     })
///     v = ActionValidator.from_schema_json(schema)
///     ok, errors = v.validate('{"radius": 1.0}')
///     assert ok
///     ok, errors = v.validate("{}")
///     assert not ok
///
#[pyclass(name = "ToolValidator")]
pub struct PyActionValidator {
    inner: ActionValidator,
}

#[pymethods]
impl PyActionValidator {
    /// Create a validator from a JSON Schema string.
    ///
    /// Raises:
    ///     ValueError: If ``schema_json`` is not valid JSON.
    #[staticmethod]
    pub fn from_schema_json(schema_json: &str) -> PyResult<Self> {
        let schema: Value = serde_json::from_str(schema_json)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid JSON: {e}")))?;
        Ok(Self {
            inner: ActionValidator::from_schema(schema),
        })
    }

    /// Create a validator from an action in an :class:`ActionRegistry`.
    ///
    /// Raises:
    ///     KeyError: If the action is not found in the registry.
    #[staticmethod]
    #[pyo3(signature = (registry, action_name, dcc_name = None))]
    pub fn from_action_registry(
        registry: &ActionRegistry,
        action_name: &str,
        dcc_name: Option<&str>,
    ) -> PyResult<Self> {
        let meta: Option<ActionMeta> = registry.get_action(action_name, dcc_name);
        let meta = meta.ok_or_else(|| {
            pyo3::exceptions::PyKeyError::new_err(format!(
                "action '{action_name}' not found in registry"
            ))
        })?;
        Ok(Self {
            inner: ActionValidator::new(&meta),
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

// ── PyActionDispatcher ────────────────────────────────────────────────────────

/// Routes action calls to registered Python callables with automatic validation.
///
/// Example::
///
///     import json
///     from dcc_mcp_core import ActionRegistry, ActionDispatcher
///
///     reg = ActionRegistry()
///     reg.register(
///         "create_sphere",
///         input_schema=json.dumps({
///             "type": "object",
///             "required": ["radius"],
///             "properties": {"radius": {"type": "number", "minimum": 0.0}},
///         }),
///     )
///     dispatcher = ActionDispatcher(reg)
///
///     def create_sphere(params):
///         return {"created": True, "radius": params["radius"]}
///
///     dispatcher.register_handler("create_sphere", create_sphere)
///     result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0}))
///     assert result["output"]["created"] is True
///
#[pyclass(name = "ToolDispatcher")]
pub struct PyActionDispatcher {
    /// Rust dispatcher used for schema validation; handler calls are short-circuited.
    inner: ActionDispatcher,
    /// Map from action name → Python callable.
    handler_map: HashMap<String, Py<PyAny>>,
    /// Whether to skip validation when the action schema is empty.
    pub skip_empty_schema_validation: bool,
}

#[pymethods]
impl PyActionDispatcher {
    /// Create a new dispatcher backed by the given registry.
    #[new]
    pub fn new(registry: &ActionRegistry) -> Self {
        Self {
            inner: ActionDispatcher::new(registry.clone()),
            handler_map: HashMap::new(),
            skip_empty_schema_validation: true,
        }
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
    #[pyo3(signature = (action_name, params_json = "null"))]
    pub fn dispatch<'py>(
        &self,
        py: Python<'py>,
        action_name: &str,
        params_json: &str,
    ) -> PyResult<Bound<'py, PyDict>> {
        // 1. Parse params
        let params: Value = serde_json::from_str(params_json)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid JSON: {e}")))?;

        // 2. Validate via Rust dispatcher (the stub handler is registered there too)
        let validation_skipped = if !self.handler_map.contains_key(action_name) {
            return Err(pyo3::exceptions::PyKeyError::new_err(format!(
                "no handler for '{action_name}'"
            )));
        } else {
            // Use the Rust dispatcher only for validation
            match self.inner.dispatch(action_name, params.clone()) {
                Ok(r) => r.validation_skipped,
                Err(DispatchError::ValidationFailed(msg)) => {
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
                Err(err @ DispatchError::ActionDisabled { .. }) => {
                    return Err(pyo3::exceptions::PyPermissionError::new_err(
                        err.to_string(),
                    ));
                }
            }
        };

        // 3. Call the Python handler
        let handler = self.handler_map.get(action_name).expect("checked above");
        let py_params = value_to_py(py, &params)?;
        let raw = handler.call1(py, (py_params,)).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("handler error: {e}"))
        })?;

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

// ── Internal helpers for PyActionPipeline ────────────────────────────────────

impl PyActionDispatcher {
    /// Return a clone of the underlying Rust registry (for pipeline construction).
    pub fn registry(&self) -> ActionRegistry {
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

/// Register `PyActionValidator` and `PyActionDispatcher` on the given Python module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyActionValidator>()?;
    m.add_class::<PyActionDispatcher>()?;
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ActionMeta;
    use serde_json::json;

    fn make_registry_with_schema(schema: Value) -> ActionRegistry {
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "test_action".into(),
            dcc: "maya".into(),
            input_schema: schema,
            ..Default::default()
        });
        reg
    }

    // ── ActionValidator (Rust) ────────────────────────────────────────────────

    #[test]
    fn test_validator_valid_schema() {
        let schema = json!({ "type": "object", "required": ["radius"],
            "properties": { "radius": { "type": "number", "minimum": 0.0 } } });
        let v = ActionValidator::from_schema(schema);
        assert!(v.validate_input(&json!({"radius": 1.0})).is_valid());
    }

    #[test]
    fn test_validator_missing_required() {
        let schema = json!({ "type": "object", "required": ["radius"] });
        let v = ActionValidator::from_schema(schema);
        let result = v.validate_input(&json!({}));
        assert!(!result.is_valid());
        assert!(result.errors[0].message.contains("radius"));
    }

    #[test]
    fn test_validator_from_registry() {
        let reg = make_registry_with_schema(json!({ "type": "object" }));
        let meta: Option<ActionMeta> = reg.get_action("test_action", None);
        assert!(meta.is_some());
        let v = ActionValidator::new(&meta.unwrap());
        assert!(v.validate_input(&json!({})).is_valid());
    }

    // ── ActionDispatcher (Rust) ───────────────────────────────────────────────

    #[test]
    fn test_dispatcher_new_empty() {
        let reg = ActionRegistry::new();
        let d = ActionDispatcher::new(reg);
        assert_eq!(d.handler_count(), 0);
        assert!(!d.has_handler("x"));
    }

    #[test]
    fn test_dispatcher_register_and_dispatch() {
        let reg = make_registry_with_schema(json!({}));
        let d = ActionDispatcher::new(reg);
        d.register_handler("test_action", |_| Ok(json!({"ok": true})));
        assert!(d.has_handler("test_action"));
        let result = d.dispatch("test_action", json!({})).unwrap();
        assert_eq!(result.action, "test_action");
        assert_eq!(result.output["ok"], json!(true));
    }

    #[test]
    fn test_dispatcher_handler_not_found() {
        let reg = ActionRegistry::new();
        let d = ActionDispatcher::new(reg);
        let err = d.dispatch("missing", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::HandlerNotFound(_)));
    }

    #[test]
    fn test_dispatcher_validation_fails() {
        let reg = make_registry_with_schema(json!({ "type": "object", "required": ["x"] }));
        let d = ActionDispatcher::new(reg);
        d.register_handler("test_action", |_| Ok(json!(null)));
        let err = d.dispatch("test_action", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::ValidationFailed(_)));
    }

    #[test]
    fn test_dispatcher_remove_handler() {
        let reg = ActionRegistry::new();
        let d = ActionDispatcher::new(reg);
        d.register_handler("a", |_| Ok(json!(null)));
        assert!(d.remove_handler("a"));
        assert!(!d.has_handler("a"));
        assert!(!d.remove_handler("a"));
    }

    #[test]
    fn test_dispatcher_handler_names_sorted() {
        let reg = ActionRegistry::new();
        let d = ActionDispatcher::new(reg);
        d.register_handler("z", |_| Ok(json!(null)));
        d.register_handler("a", |_| Ok(json!(null)));
        d.register_handler("m", |_| Ok(json!(null)));
        assert_eq!(d.handler_names(), vec!["a", "m", "z"]);
    }

    #[test]
    fn test_dispatcher_handler_error() {
        let reg = ActionRegistry::new();
        let d = ActionDispatcher::new(reg);
        d.register_handler("failing", |_| Err("oops".into()));
        let err = d.dispatch("failing", json!({})).unwrap_err();
        assert!(matches!(err, DispatchError::HandlerError(_)));
    }
}
