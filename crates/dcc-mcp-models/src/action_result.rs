//! ToolResult — unified result type for all tool executions.
//!
//! Rust struct exposed to Python via PyO3.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pyclass_enum, gen_stub_pymethods};

#[cfg(feature = "python-bindings")]
use dcc_mcp_pybridge::py_json::{
    json_value_to_bound_py, py_any_to_json_value, py_dict_to_json_map,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── ActionResult-related constants (moved from dcc-mcp-utils in #485) ──

/// Default error type when the error message doesn't follow the `Type: details` pattern.
#[cfg(feature = "python-bindings")]
const DEFAULT_ERROR_TYPE: &str = "Exception";
/// Default user-facing prompt for exception-based results.
#[cfg(feature = "python-bindings")]
const DEFAULT_ERROR_PROMPT: &str = "Please check error details and retry";
/// Default success message for wrapped non-dict results.
#[cfg(feature = "python-bindings")]
const DEFAULT_SUCCESS_MESSAGE: &str = "Successfully processed result";
/// Context key for the error type string.
#[cfg(feature = "python-bindings")]
const CTX_KEY_ERROR_TYPE: &str = "error_type";
/// Context key for the traceback string.
#[cfg(feature = "python-bindings")]
const CTX_KEY_TRACEBACK: &str = "traceback";
/// Context key for the wrapped value.
#[cfg(feature = "python-bindings")]
const CTX_KEY_VALUE: &str = "value";
/// Context key for possible solutions list.
#[cfg(feature = "python-bindings")]
const CTX_KEY_POSSIBLE_SOLUTIONS: &str = "possible_solutions";
/// Standard keys extracted from a dict during `validate_action_result`.
#[cfg(feature = "python-bindings")]
const ACTION_RESULT_KNOWN_KEYS: &[&str] = &["success", "message", "prompt", "error"];

// RTK-inspired: limit context depth and array size to reduce token consumption
fn compact_json_value(
    value: &serde_json::Value,
    depth: usize,
    max_depth: usize,
) -> serde_json::Value {
    if depth >= max_depth {
        return serde_json::Value::String("...".to_string());
    }
    match value {
        serde_json::Value::Array(arr) => {
            // Limit array to first 10 elements
            let limited = arr
                .iter()
                .take(10)
                .map(|v| compact_json_value(v, depth + 1, max_depth))
                .collect();
            serde_json::Value::Array(limited)
        }
        serde_json::Value::Object(obj) => {
            // Limit object depth to 3 levels
            let limited = obj
                .iter()
                .take(10)
                .map(|(k, v)| (k.clone(), compact_json_value(v, depth + 1, max_depth)))
                .collect();
            serde_json::Value::Object(limited)
        }
        other => other.clone(),
    }
}

// ── Serialization format ─────────────────────────────────────────────────────

/// Supported serialization formats for `ToolResult`.
///
/// The default is [`SerializeFormat::Json`] (UTF-8 text, human-readable).
/// [`SerializeFormat::MsgPack`] produces compact binary (MessagePack via `rmp-serde`)
/// and is suitable for high-throughput or binary transport scenarios.
///
/// # Future extensibility
/// Additional formats (e.g. CBOR, Bincode) can be added as new variants without
/// breaking the existing API.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass_enum)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "SerializeFormat", eq, eq_int, from_py_object)
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SerializeFormat {
    /// JSON (default): UTF-8 text, human-readable, widely compatible.
    #[default]
    Json,
    /// MessagePack: compact binary encoding via `rmp-serde`.
    MsgPack,
}

#[cfg(feature = "python-bindings")]
#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl SerializeFormat {
    fn __repr__(&self) -> &'static str {
        match self {
            SerializeFormat::Json => "SerializeFormat.Json",
            SerializeFormat::MsgPack => "SerializeFormat.MsgPack",
        }
    }
}

/// Internal Rust data representation (serde-friendly).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionResultModelData {
    /// Whether the action completed successfully.
    pub success: bool,
    /// Human-readable result or error summary.
    pub message: String,
    /// Optional prompt/hint for the next user action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Optional error message when `success` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Arbitrary key-value context data (e.g. traceback, error_type).
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

// Manual impl: `success` defaults to `true` (unlike `bool::default()` which is `false`),
// matching the Python `ToolResult.__new__` signature.
impl Default for ActionResultModelData {
    fn default() -> Self {
        Self {
            success: true,
            message: String::new(),
            prompt: None,
            error: None,
            context: HashMap::new(),
        }
    }
}

impl ActionResultModelData {
    /// Create a success result with context.
    #[must_use]
    pub fn success(
        message: String,
        prompt: Option<String>,
        context: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            success: true,
            message,
            prompt,
            error: None,
            context,
        }
    }

    /// Create a failure result with context.
    #[must_use]
    pub fn failure(
        message: String,
        error: Option<String>,
        prompt: Option<String>,
        context: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            success: false,
            message,
            prompt,
            error,
            context,
        }
    }

    /// Serialize to bytes using the specified format.
    ///
    /// Returns `Err(String)` if serialization fails (should never happen for
    /// well-formed data).
    pub fn to_bytes(&self, fmt: SerializeFormat) -> Result<Vec<u8>, String> {
        match fmt {
            SerializeFormat::Json => serde_json::to_vec(self).map_err(|e| e.to_string()),
            SerializeFormat::MsgPack => rmp_serde::to_vec_named(self).map_err(|e| e.to_string()),
        }
    }

    /// Deserialize from bytes using the specified format.
    pub fn from_bytes(data: &[u8], fmt: SerializeFormat) -> Result<Self, String> {
        match fmt {
            SerializeFormat::Json => serde_json::from_slice(data).map_err(|e| e.to_string()),
            SerializeFormat::MsgPack => rmp_serde::from_slice(data).map_err(|e| e.to_string()),
        }
    }

    /// Convenience: serialize to a JSON string.
    /// Convenience: serialize to a JSON string.
    pub fn to_json_string(&self) -> Result<String, String> {
        // RTK-inspired: compact context to reduce token consumption
        let mut compacted = self.clone();
        compacted.context = compacted
            .context
            .iter()
            .map(|(k, v)| (k.clone(), compact_json_value(v, 0, 3)))
            .collect();
        serde_json::to_string(&compacted).map_err(|e| e.to_string())
    }

    /// Convenience: deserialize from a JSON string.
    pub fn from_json_str(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}

/// Python-facing ToolResult.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ToolResult", eq, from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionResultModel {
    pub(crate) inner: ActionResultModelData,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ActionResultModel {
    #[new]
    #[pyo3(signature = (success=true, message="".to_string(), prompt=None, error=None, context=None))]
    fn new(
        success: bool,
        message: String,
        prompt: Option<String>,
        error: Option<String>,
        context: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let ctx = if let Some(dict) = context {
            py_dict_to_json_map(dict)?
        } else {
            HashMap::new()
        };
        Ok(Self {
            inner: ActionResultModelData {
                success,
                message,
                prompt,
                error,
                context: ctx,
            },
        })
    }

    #[getter]
    fn success(&self) -> bool {
        self.inner.success
    }

    #[getter]
    fn message(&self) -> &str {
        &self.inner.message
    }

    #[setter]
    fn set_message(&mut self, value: String) {
        self.inner.message = value;
    }

    #[getter]
    fn prompt(&self) -> Option<&str> {
        self.inner.prompt.as_deref()
    }

    #[getter]
    fn error(&self) -> Option<&str> {
        self.inner.error.as_deref()
    }

    #[getter]
    fn context<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (k, v) in &self.inner.context {
            dict.set_item(k, json_value_to_bound_py(py, v)?)?;
        }
        Ok(dict)
    }

    /// Create a new instance with error information.
    #[allow(clippy::double_must_use)]
    #[must_use]
    fn with_error(&self, error: String) -> Self {
        let mut data = self.inner.clone();
        data.success = false;
        data.error = Some(error);
        Self { inner: data }
    }

    /// Create a new instance with updated context.
    #[allow(clippy::double_must_use)]
    #[must_use]
    #[pyo3(signature = (**kwargs))]
    fn with_context(&self, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let mut data = self.inner.clone();
        if let Some(kw) = kwargs {
            for (k, v) in kw.iter() {
                let key: String = k.extract()?;
                let val = py_any_to_json_value(&v)?;
                data.context.insert(key, val);
            }
        }
        Ok(Self { inner: data })
    }

    /// Convert to dictionary.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("success", self.inner.success)?;
        dict.set_item("message", &self.inner.message)?;
        dict.set_item("prompt", self.inner.prompt.as_deref())?;
        dict.set_item("error", self.inner.error.as_deref())?;
        dict.set_item("context", self.context(py)?)?;
        Ok(dict)
    }

    /// Serialize to a JSON string.
    ///
    /// This is the recommended way to convert a `ToolResult` to a JSON string.
    /// `json.dumps(result)` will **not** work directly — use this method instead:
    ///
    /// ```python
    /// import json
    /// result = success_result("done")
    /// json_str = result.to_json()          # preferred
    /// d = result.to_dict()
    /// json_str = json.dumps(d)             # also works
    /// ```
    fn to_json(&self) -> PyResult<String> {
        self.inner
            .to_json_string()
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Iterate over key-value pairs (mapping protocol).
    ///
    /// This enables ``dict(result)`` to work, which in turn enables
    /// ``json.dumps(dict(result))``.
    ///
    /// ```python
    /// import json
    /// result = success_result("done")
    /// # Preferred — zero allocation:
    /// json_str = result.to_json()
    /// # Also works:
    /// json_str = json.dumps(result.to_dict())
    /// # Works via mapping protocol:
    /// json_str = json.dumps(dict(result))
    /// ```
    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyIterator>> {
        let dict = self.to_dict(py)?;
        pyo3::types::PyIterator::from_object(&dict.into_any())
    }

    /// Return the list of field names (part of the mapping protocol).
    fn keys<'py>(&self, py: Python<'py>) -> PyResult<Vec<String>> {
        let _ = py;
        Ok(vec![
            "success".to_string(),
            "message".to_string(),
            "prompt".to_string(),
            "error".to_string(),
            "context".to_string(),
        ])
    }

    fn __repr__(&self) -> String {
        format!(
            "ToolResult(success={}, message={:?})",
            self.inner.success, self.inner.message
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }
}

impl ActionResultModel {
    /// Create a `ToolResult` from raw data.
    #[must_use]
    pub fn from_data(data: ActionResultModelData) -> Self {
        Self { inner: data }
    }

    /// Access the underlying data.
    #[must_use]
    pub fn data(&self) -> &ActionResultModelData {
        &self.inner
    }
}

impl std::fmt::Display for ActionResultModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.inner.success {
            write!(f, "Success: {}", self.inner.message)
        } else {
            write!(
                f,
                "Error: {}",
                self.inner.error.as_deref().unwrap_or(&self.inner.message)
            )
        }
    }
}

// ── Factory functions & helpers (Python-only) ──

#[cfg(feature = "python-bindings")]
mod py_factories {
    use super::*;
    use pyo3::types::PyDict;

    use dcc_mcp_pybridge::py_json::{py_any_to_json_value, py_dict_to_json_map};

    pub(super) fn extract_context(
        context: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<HashMap<String, serde_json::Value>> {
        match context {
            Some(dict) => py_dict_to_json_map(dict),
            None => Ok(HashMap::new()),
        }
    }

    fn insert_possible_solutions(
        ctx: &mut HashMap<String, serde_json::Value>,
        solutions: Option<Vec<String>>,
    ) {
        if let Some(solutions) = solutions {
            ctx.insert(
                CTX_KEY_POSSIBLE_SOLUTIONS.to_string(),
                serde_json::Value::Array(
                    solutions
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }
    }

    /// Extract a required-type field from a Python dict with a type error on mismatch.
    fn extract_bool_field(dict: &Bound<'_, PyDict>, key: &str, default: bool) -> PyResult<bool> {
        dict.get_item(key)?
            .map(|v| {
                v.extract::<bool>().map_err(|_| {
                    pyo3::exceptions::PyTypeError::new_err(format!("'{key}' field must be a bool"))
                })
            })
            .transpose()
            .map(|opt| opt.unwrap_or(default))
    }

    /// Extract a string field from a Python dict, returning the default on absence.
    fn extract_string_field(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<String> {
        dict.get_item(key)?
            .map(|v| {
                v.extract::<String>().map_err(|_| {
                    pyo3::exceptions::PyTypeError::new_err(format!(
                        "'{key}' field must be a string"
                    ))
                })
            })
            .transpose()
            .map(|opt| opt.unwrap_or_default())
    }

    /// Extract an optional nullable string field from a Python dict.
    fn extract_optional_string_field(
        dict: &Bound<'_, PyDict>,
        key: &str,
    ) -> PyResult<Option<String>> {
        dict.get_item(key)?
            .map(|v| {
                if v.is_none() {
                    Ok(None)
                } else {
                    v.extract::<String>().map(Some).map_err(|_| {
                        pyo3::exceptions::PyTypeError::new_err(format!(
                            "'{key}' field must be a string"
                        ))
                    })
                }
            })
            .transpose()
            .map(|opt| opt.flatten())
    }

    /// Extract dict fields into a `ToolResult`, filtering out standard keys.
    fn validate_from_dict(dict: &Bound<'_, PyDict>) -> PyResult<ActionResultModel> {
        let success = extract_bool_field(dict, "success", true)?;
        let message = extract_string_field(dict, "message")?;
        let prompt = extract_optional_string_field(dict, "prompt")?;
        let error = extract_optional_string_field(dict, "error")?;

        // Build context directly as a Rust HashMap, skipping the intermediate PyDict.
        let mut ctx = HashMap::new();
        for (k, v) in dict.iter() {
            if let Ok(key) = k.extract::<String>() {
                if !ACTION_RESULT_KNOWN_KEYS.contains(&key.as_str()) {
                    ctx.insert(key, py_any_to_json_value(&v)?);
                }
            }
        }

        Ok(ActionResultModel {
            inner: ActionResultModelData {
                success,
                message,
                prompt,
                error,
                context: ctx,
            },
        })
    }

    #[pyfunction]
    #[pyo3(name = "success_result")]
    #[pyo3(signature = (message, prompt=None, **context))]
    pub fn py_success_result(
        message: String,
        prompt: Option<String>,
        context: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<ActionResultModel> {
        let ctx = extract_context(context)?;
        Ok(ActionResultModel {
            inner: ActionResultModelData::success(message, prompt, ctx),
        })
    }

    #[pyfunction]
    #[pyo3(name = "error_result")]
    #[pyo3(signature = (message, error, prompt=None, possible_solutions=None, **context))]
    pub fn py_error_result(
        message: String,
        error: String,
        prompt: Option<String>,
        possible_solutions: Option<Vec<String>>,
        context: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<ActionResultModel> {
        let mut ctx = extract_context(context)?;
        insert_possible_solutions(&mut ctx, possible_solutions);
        Ok(ActionResultModel {
            inner: ActionResultModelData::failure(message, Some(error), prompt, ctx),
        })
    }

    #[pyfunction]
    #[pyo3(name = "from_exception")]
    #[pyo3(signature = (error_message, message=None, prompt=None, include_traceback=true, possible_solutions=None, **context))]
    pub fn py_from_exception(
        error_message: String,
        message: Option<String>,
        prompt: Option<String>,
        include_traceback: bool,
        possible_solutions: Option<Vec<String>>,
        context: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<ActionResultModel> {
        let mut ctx = extract_context(context)?;
        // Extract error type from the error_message pattern "ErrorType: details"
        let error_type = error_message
            .split_once(':')
            .map(|(t, _)| t.trim().to_string())
            .unwrap_or_else(|| DEFAULT_ERROR_TYPE.to_string());
        ctx.insert(
            CTX_KEY_ERROR_TYPE.to_string(),
            serde_json::Value::String(error_type),
        );
        // Build the user-facing message before moving error_message.
        let msg = message.unwrap_or_else(|| format!("Error: {error_message}"));
        if include_traceback {
            // RTK-inspired: limit traceback to 1KB to reduce token consumption
            let truncated_traceback = if error_message.len() > 1024 {
                let trace_id = format!("err-{}", uuid::Uuid::new_v4());
                format!(
                    "{}... (truncated, see trace_id: {})",
                    &error_message[..1000.min(error_message.len())],
                    trace_id
                )
            } else {
                error_message.clone()
            };
            ctx.insert(
                CTX_KEY_TRACEBACK.to_string(),
                serde_json::Value::String(truncated_traceback),
            );
        }
        insert_possible_solutions(&mut ctx, possible_solutions);
        Ok(ActionResultModel {
            inner: ActionResultModelData::failure(
                msg,
                Some(error_message),
                Some(prompt.unwrap_or_else(|| DEFAULT_ERROR_PROMPT.to_string())),
                ctx,
            ),
        })
    }

    #[pyfunction]
    #[pyo3(name = "validate_action_result")]
    pub fn py_validate_action_result(result: &Bound<'_, PyAny>) -> PyResult<ActionResultModel> {
        // If already ActionResultModel, clone it
        if let Ok(arm) = result.extract::<ActionResultModel>() {
            return Ok(arm);
        }
        // If dict, try to convert
        if let Ok(dict) = result.cast::<PyDict>() {
            return validate_from_dict(dict);
        }
        // Wrap as success
        let msg = result.to_string();
        Ok(ActionResultModel {
            inner: ActionResultModelData::success(
                DEFAULT_SUCCESS_MESSAGE.to_string(),
                None,
                HashMap::from([(CTX_KEY_VALUE.to_string(), serde_json::Value::String(msg))]),
            ),
        })
    }

    /// Serialize a `ToolResult` to a string (JSON) or bytes (MsgPack).
    ///
    /// Returns:
    /// - `str` for `SerializeFormat::Json`
    /// - `bytes` for `SerializeFormat::MsgPack`
    #[pyfunction]
    #[pyo3(name = "serialize_result")]
    #[pyo3(signature = (result, format = SerializeFormat::Json))]
    pub fn py_serialize_result(
        py: Python<'_>,
        result: &ActionResultModel,
        format: SerializeFormat,
    ) -> PyResult<Py<PyAny>> {
        let bytes = result
            .inner
            .to_bytes(format)
            .map_err(pyo3::exceptions::PyValueError::new_err)?;
        match format {
            SerializeFormat::Json => {
                let s = String::from_utf8(bytes)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                Ok(s.into_pyobject(py)?.into_any().unbind())
            }
            SerializeFormat::MsgPack => {
                Ok(pyo3::types::PyBytes::new(py, &bytes).into_any().unbind())
            }
        }
    }

    /// Deserialize a `str` (JSON) or `bytes` (MsgPack) into a `ToolResult`.
    ///
    /// The format must match what was used during serialization.
    #[pyfunction]
    #[pyo3(name = "deserialize_result")]
    #[pyo3(signature = (data, format = SerializeFormat::Json))]
    pub fn py_deserialize_result(
        data: &Bound<'_, PyAny>,
        format: SerializeFormat,
    ) -> PyResult<ActionResultModel> {
        let raw: Vec<u8> = if let Ok(s) = data.extract::<String>() {
            s.into_bytes()
        } else if let Ok(b) = data.extract::<Vec<u8>>() {
            b
        } else {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "data must be str (JSON) or bytes (MsgPack)",
            ));
        };
        let data = ActionResultModelData::from_bytes(&raw, format)
            .map_err(pyo3::exceptions::PyValueError::new_err)?;
        Ok(ActionResultModel { inner: data })
    }
}

#[cfg(feature = "python-bindings")]
pub use py_factories::{
    py_deserialize_result, py_error_result, py_from_exception, py_serialize_result,
    py_success_result, py_validate_action_result,
};

#[cfg(test)]
#[path = "action_result_tests.rs"]
mod tests;
