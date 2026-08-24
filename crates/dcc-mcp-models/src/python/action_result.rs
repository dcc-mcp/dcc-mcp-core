//! PyO3 bindings for `ActionResultModel` / `SerializeFormat` and the
//! `success_result` / `error_result` / `from_exception` /
//! `validate_action_result` / `serialize_result` / `deserialize_result`
//! factory functions.

use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::PyDict;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyfunction, gen_stub_pymethods};

use dcc_mcp_pybridge::py_json::{
    json_value_to_bound_py, py_any_to_json_value, py_dict_to_json_map,
};

use crate::action_result::{ActionResultModel, ActionResultModelData, SerializeFormat};

// ── ActionResult-related constants (Python-only) ──

const DEFAULT_ERROR_TYPE: &str = "Exception";
const DEFAULT_ERROR_PROMPT: &str = "Please check error details and retry";
const DEFAULT_SUCCESS_MESSAGE: &str = "Successfully processed result";
const CTX_KEY_ERROR_TYPE: &str = "error_type";
const CTX_KEY_TRACEBACK: &str = "traceback";
const CTX_KEY_VALUE: &str = "value";
const CTX_KEY_POSSIBLE_SOLUTIONS: &str = "possible_solutions";
const META_KEY_DCC_ERROR: &str = "dcc.error";
const ACTION_RESULT_KNOWN_KEYS: &[&str] = &[
    "success",
    "message",
    "prompt",
    "error",
    "context",
    "postcondition",
    "_meta",
];

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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl ActionResultModel {
    #[new]
    #[pyo3(signature = (success=true, message="".to_string(), prompt=None, error=None, context=None, *, postcondition=None, _meta=None))]
    fn new(
        success: bool,
        message: String,
        prompt: Option<String>,
        error: Option<String>,
        context: Option<&Bound<'_, PyDict>>,
        postcondition: Option<&Bound<'_, PyDict>>,
        _meta: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let ctx = extract_context(context)?;
        let postcondition = postcondition.map(py_dict_to_json_map).transpose()?;
        let meta = extract_context(_meta)?;
        Self::from_data_with_envelope(
            ActionResultModelData {
                success,
                message,
                prompt,
                error,
                context: ctx,
            },
            postcondition,
            meta,
        )
        .map_err(pyo3::exceptions::PyTypeError::new_err)
    }

    #[getter]
    fn success(&self) -> bool {
        self.data().success
    }

    #[getter]
    fn message(&self) -> &str {
        &self.data().message
    }

    #[setter]
    fn set_message(&mut self, value: String) {
        self.inner.message = value;
    }

    #[getter]
    fn prompt(&self) -> Option<&str> {
        self.data().prompt.as_deref()
    }

    #[getter]
    fn error(&self) -> Option<&str> {
        self.data().error.as_deref()
    }

    #[getter]
    fn context<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (k, v) in &self.data().context {
            dict.set_item(k, json_value_to_bound_py(py, v)?)?;
        }
        Ok(dict)
    }

    #[getter]
    fn _meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (k, v) in self.meta() {
            dict.set_item(k, json_value_to_bound_py(py, v)?)?;
        }
        Ok(dict)
    }

    #[getter]
    fn get_postcondition<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDict>>> {
        self.postcondition()
            .map(|postcondition| {
                let dict = PyDict::new(py);
                for (key, value) in postcondition {
                    dict.set_item(key, json_value_to_bound_py(py, value)?)?;
                }
                Ok(dict)
            })
            .transpose()
    }

    /// Create a new instance with error information.
    #[allow(clippy::double_must_use)]
    #[must_use]
    fn with_error(&self, error: String) -> Self {
        let mut data = self.data().clone();
        data.success = false;
        data.error = Some(error);
        Self {
            inner: data,
            postcondition: self.postcondition().cloned(),
            meta: self.meta().clone(),
        }
    }

    /// Create a new instance with updated context.
    #[allow(clippy::double_must_use)]
    #[must_use]
    #[pyo3(signature = (**kwargs))]
    fn with_context(&self, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let mut data = self.data().clone();
        if let Some(kw) = kwargs {
            for (k, v) in kw.iter() {
                let key: String = k.extract()?;
                let val = py_any_to_json_value(&v)?;
                data.context.insert(key, val);
            }
        }
        Self::from_data_with_envelope(data, self.postcondition().cloned(), self.meta().clone())
            .map_err(pyo3::exceptions::PyTypeError::new_err)
    }

    /// Convert to dictionary.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("success", self.data().success)?;
        dict.set_item("message", &self.data().message)?;
        dict.set_item("prompt", self.data().prompt.as_deref())?;
        dict.set_item("error", self.data().error.as_deref())?;
        dict.set_item("context", self.context(py)?)?;
        if let Some(postcondition) = self.get_postcondition(py)? {
            dict.set_item("postcondition", postcondition)?;
        }
        if !self.meta().is_empty() {
            dict.set_item("_meta", self._meta(py)?)?;
        }
        Ok(dict)
    }

    /// Serialize to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        self.to_json_string()
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Iterate over key-value pairs (mapping protocol).
    fn __iter__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = self.to_dict(py)?;
        Ok(pyo3::types::PyIterator::from_object(&dict.into_any())?
            .into_any()
            .unbind())
    }

    /// Return the list of field names (part of the mapping protocol).
    fn keys<'py>(&self, py: Python<'py>) -> PyResult<Vec<String>> {
        let _ = py;
        let mut keys = vec![
            "success".to_string(),
            "message".to_string(),
            "prompt".to_string(),
            "error".to_string(),
            "context".to_string(),
        ];
        if self.postcondition().is_some() {
            keys.push("postcondition".to_string());
        }
        if !self.meta().is_empty() {
            keys.push("_meta".to_string());
        }
        Ok(keys)
    }

    fn __repr__(&self) -> String {
        dcc_mcp_pybridge::repr_pairs!(
            "ToolResult",
            [
                ("success", self.data().success),
                ("message", self.data().message),
            ]
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }
}

// ── Factory functions ────────────────────────────────────────────────

fn extract_context(
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

fn insert_error_meta(
    meta: &mut HashMap<String, serde_json::Value>,
    error_type: &str,
    message: &str,
    traceback: Option<String>,
) {
    let mut details = serde_json::Map::from_iter([
        (
            "type".to_string(),
            serde_json::Value::String(error_type.to_string()),
        ),
        (
            "message".to_string(),
            serde_json::Value::String(message.to_string()),
        ),
    ]);
    if let Some(traceback) = traceback {
        details.insert(
            "traceback".to_string(),
            serde_json::Value::String(traceback),
        );
    }
    meta.insert(
        META_KEY_DCC_ERROR.to_string(),
        serde_json::Value::Object(details),
    );
}

fn truncate_utf8_at_byte_limit(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn is_exception_error_code(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let has_valid_syntax = (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));
    if !has_valid_syntax {
        return false;
    }

    let leaf = value.rsplit('.').next().unwrap_or(value);
    value.contains('_')
        || value.contains('-')
        || leaf.ends_with("Error")
        || leaf.ends_with("Exception")
        || matches!(
            leaf,
            "KeyboardInterrupt"
                | "SystemExit"
                | "GeneratorExit"
                | "StopIteration"
                | "StopAsyncIteration"
        )
}

fn split_exception_message(error_message: &str) -> (String, String) {
    if let Some((candidate, detail)) = error_message.split_once(':') {
        let candidate = candidate.trim();
        let detail = detail.trim();
        if is_exception_error_code(candidate) && !detail.starts_with("//") {
            return (candidate.to_string(), detail.to_string());
        }
    }

    (DEFAULT_ERROR_TYPE.to_string(), error_message.to_string())
}

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

fn extract_string_field(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<String> {
    dict.get_item(key)?
        .map(|v| {
            v.extract::<String>().map_err(|_| {
                pyo3::exceptions::PyTypeError::new_err(format!("'{key}' field must be a string"))
            })
        })
        .transpose()
        .map(|opt| opt.unwrap_or_default())
}

fn extract_optional_string_field(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<String>> {
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

fn extract_dict_field(
    dict: &Bound<'_, PyDict>,
    key: &str,
) -> PyResult<HashMap<String, serde_json::Value>> {
    dict.get_item(key)?
        .map(|v| {
            if v.is_none() {
                Ok(HashMap::new())
            } else {
                let value = v.cast::<PyDict>().map_err(|_| {
                    pyo3::exceptions::PyTypeError::new_err(format!("'{key}' field must be a dict"))
                })?;
                py_dict_to_json_map(value)
            }
        })
        .transpose()
        .map(|opt| opt.unwrap_or_default())
}

fn validate_from_dict(dict: &Bound<'_, PyDict>) -> PyResult<ActionResultModel> {
    let success = extract_bool_field(dict, "success", true)?;
    let message = extract_string_field(dict, "message")?;
    let prompt = extract_optional_string_field(dict, "prompt")?;
    let error = extract_optional_string_field(dict, "error")?;

    let mut ctx = extract_dict_field(dict, "context")?;
    let postcondition = dict
        .get_item("postcondition")?
        .map(|value| {
            if value.is_none() {
                Ok(None)
            } else {
                let value = value.cast::<PyDict>().map_err(|_| {
                    pyo3::exceptions::PyTypeError::new_err(
                        "'postcondition' field must be a dict or None",
                    )
                })?;
                py_dict_to_json_map(value).map(Some)
            }
        })
        .transpose()?
        .flatten();
    let meta = extract_dict_field(dict, "_meta")?;
    for (k, v) in dict.iter() {
        if let Ok(key) = k.extract::<String>()
            && !ACTION_RESULT_KNOWN_KEYS.contains(&key.as_str())
        {
            ctx.insert(key, py_any_to_json_value(&v)?);
        }
    }

    ActionResultModel::from_data_with_envelope(
        ActionResultModelData {
            success,
            message,
            prompt,
            error,
            context: ctx,
        },
        postcondition,
        meta,
    )
    .map_err(pyo3::exceptions::PyTypeError::new_err)
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "success_result")]
#[pyo3(signature = (message, prompt=None, *, _meta=None, **context))]
pub fn py_success_result(
    message: String,
    prompt: Option<String>,
    _meta: Option<&Bound<'_, PyDict>>,
    context: Option<&Bound<'_, PyDict>>,
) -> PyResult<ActionResultModel> {
    let ctx = extract_context(context)?;
    let data = ActionResultModelData::success(message, prompt, ctx);
    Ok(ActionResultModel::from_data_with_meta(
        data,
        extract_context(_meta)?,
    ))
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "error_result")]
#[pyo3(signature = (message, error, prompt=None, possible_solutions=None, *, _meta=None, **context))]
pub fn py_error_result(
    message: String,
    error: String,
    prompt: Option<String>,
    possible_solutions: Option<Vec<String>>,
    _meta: Option<&Bound<'_, PyDict>>,
    context: Option<&Bound<'_, PyDict>>,
) -> PyResult<ActionResultModel> {
    let mut ctx = extract_context(context)?;
    insert_possible_solutions(&mut ctx, possible_solutions);
    let data = ActionResultModelData::failure(message, Some(error), prompt, ctx);
    Ok(ActionResultModel::from_data_with_meta(
        data,
        extract_context(_meta)?,
    ))
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "from_exception")]
#[pyo3(signature = (error_message, message=None, prompt=None, include_traceback=true, possible_solutions=None, *, _meta=None, **context))]
pub fn py_from_exception(
    error_message: String,
    message: Option<String>,
    prompt: Option<String>,
    include_traceback: bool,
    possible_solutions: Option<Vec<String>>,
    _meta: Option<&Bound<'_, PyDict>>,
    context: Option<&Bound<'_, PyDict>>,
) -> PyResult<ActionResultModel> {
    let mut ctx = extract_context(context)?;
    let (error_type, error_detail) = split_exception_message(&error_message);
    let msg = message.unwrap_or_else(|| format!("Error: {error_message}"));
    let traceback = include_traceback.then(|| {
        if error_message.len() > 1024 {
            let trace_id = format!("err-{}", uuid::Uuid::new_v4());
            format!(
                "{}... (truncated, see trace_id: {})",
                truncate_utf8_at_byte_limit(&error_message, 1000),
                trace_id
            )
        } else {
            error_message.clone()
        }
    });
    ctx.insert(
        CTX_KEY_ERROR_TYPE.to_string(),
        serde_json::Value::String(error_type.clone()),
    );
    if let Some(traceback) = &traceback {
        ctx.insert(
            CTX_KEY_TRACEBACK.to_string(),
            serde_json::Value::String(traceback.clone()),
        );
    }
    insert_possible_solutions(&mut ctx, possible_solutions);
    let data = ActionResultModelData::failure(
        msg,
        Some(error_type.clone()),
        Some(prompt.unwrap_or_else(|| DEFAULT_ERROR_PROMPT.to_string())),
        ctx,
    );
    let mut meta = extract_context(_meta)?;
    insert_error_meta(&mut meta, &error_type, &error_detail, traceback);
    Ok(ActionResultModel::from_data_with_meta(data, meta))
}

#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "validate_action_result")]
pub fn py_validate_action_result(result: &Bound<'_, PyAny>) -> PyResult<ActionResultModel> {
    if let Ok(arm) = result.extract::<ActionResultModel>() {
        return Ok(arm);
    }
    if let Ok(dict) = result.cast::<PyDict>() {
        return validate_from_dict(dict);
    }
    let msg = result.to_string();
    Ok(ActionResultModel::from_data(
        ActionResultModelData::success(
            DEFAULT_SUCCESS_MESSAGE.to_string(),
            None,
            HashMap::from([(CTX_KEY_VALUE.to_string(), serde_json::Value::String(msg))]),
        ),
    ))
}

/// Serialize a `ToolResult` to a string (JSON) or bytes (MsgPack).
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "serialize_result")]
#[pyo3(signature = (result, format = SerializeFormat::Json))]
pub fn py_serialize_result(
    py: Python<'_>,
    result: &ActionResultModel,
    format: SerializeFormat,
) -> PyResult<Py<PyAny>> {
    let bytes = result
        .to_bytes(format)
        .map_err(pyo3::exceptions::PyValueError::new_err)?;
    match format {
        SerializeFormat::Json => {
            let s = String::from_utf8(bytes)
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
            Ok(s.into_pyobject(py)?.into_any().unbind())
        }
        SerializeFormat::MsgPack => Ok(pyo3::types::PyBytes::new(py, &bytes).into_any().unbind()),
    }
}

/// Deserialize a `str` (JSON) or `bytes` (MsgPack) into a `ToolResult`.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
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
    let data = ActionResultModel::from_bytes(&raw, format)
        .map_err(pyo3::exceptions::PyValueError::new_err)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::exceptions::PyTypeError;
    use std::sync::Once;

    fn initialize_python() {
        static INITIALIZE: Once = Once::new();
        INITIALIZE.call_once(Python::initialize);
    }

    #[test]
    fn test_validate_to_dict_and_serialize_preserve_top_level_meta() {
        initialize_python();
        Python::attach(|py| -> PyResult<()> {
            let error_details = PyDict::new(py);
            error_details.set_item("type", "RuntimeError")?;
            error_details.set_item("message", "host stopped")?;

            let meta = PyDict::new(py);
            meta.set_item("dcc.error", &error_details)?;

            let context = PyDict::new(py);
            context.set_item("action_name", "create_sphere")?;

            let payload = PyDict::new(py);
            payload.set_item("success", false)?;
            payload.set_item("message", "Execution failed")?;
            payload.set_item("error", "execution_error")?;
            payload.set_item("context", &context)?;
            payload.set_item("_meta", &meta)?;
            payload.set_item("legacy_extra", 42)?;

            let result = validate_from_dict(&payload)?;
            assert_eq!(
                result.meta()["dcc.error"],
                serde_json::json!({"type": "RuntimeError", "message": "host stopped"})
            );
            assert!(!result.data().context.contains_key("_meta"));
            assert_eq!(result.data().context["legacy_extra"], serde_json::json!(42));

            let rendered = result.to_dict(py)?;
            let rendered_json = py_dict_to_json_map(&rendered)?;
            assert_eq!(
                rendered_json["_meta"],
                serde_json::json!({
                    "dcc.error": {"type": "RuntimeError", "message": "host stopped"}
                })
            );
            assert!(result.keys(py)?.contains(&"_meta".to_string()));

            let serialized = py_serialize_result(py, &result, SerializeFormat::Json)?;
            let serialized: String = serialized.bind(py).extract()?;
            let serialized: serde_json::Value = serde_json::from_str(&serialized).unwrap();
            assert_eq!(serialized["_meta"], rendered_json["_meta"]);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_constructor_and_factory_keep_meta_outside_context() {
        initialize_python();
        Python::attach(|py| -> PyResult<()> {
            let meta = PyDict::new(py);
            meta.set_item("vendor.trace", "trace-42")?;

            let constructed = ActionResultModel::new(
                true,
                "Done".to_string(),
                None,
                None,
                None,
                None,
                Some(&meta),
            )?;
            assert_eq!(
                constructed.meta()["vendor.trace"],
                serde_json::json!("trace-42")
            );
            assert!(constructed.data().context.is_empty());

            let failed = py_error_result(
                "Execution failed".to_string(),
                "execution_error".to_string(),
                None,
                None,
                Some(&meta),
                None,
            )?;
            assert_eq!(failed.meta(), constructed.meta());
            assert!(failed.data().context.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_from_exception_uses_string_code_and_namespaced_meta() {
        initialize_python();
        Python::attach(|py| -> PyResult<()> {
            let meta = PyDict::new(py);
            meta.set_item("vendor.trace", "trace-42")?;

            let result = py_from_exception(
                "RuntimeError: host stopped".to_string(),
                None,
                None,
                true,
                None,
                Some(&meta),
                None,
            )?;

            assert_eq!(result.data().error.as_deref(), Some("RuntimeError"));
            assert_eq!(
                result.data().context[CTX_KEY_ERROR_TYPE],
                serde_json::json!("RuntimeError")
            );
            assert_eq!(
                result.data().context[CTX_KEY_TRACEBACK],
                serde_json::json!("RuntimeError: host stopped")
            );
            assert_eq!(result.meta()["vendor.trace"], serde_json::json!("trace-42"));
            assert_eq!(
                result.meta()[META_KEY_DCC_ERROR],
                serde_json::json!({
                    "type": "RuntimeError",
                    "message": "host stopped",
                    "traceback": "RuntimeError: host stopped"
                })
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_from_exception_truncates_utf8_on_character_boundary() {
        initialize_python();
        for (unit, count, expected_prefix_count) in [("错", 342, 333), ("🙂", 260, 250)] {
            let result =
                py_from_exception(unit.repeat(count), None, None, true, None, None, None).unwrap();

            let traceback = result.data().context[CTX_KEY_TRACEBACK]
                .as_str()
                .expect("traceback must be a string");
            assert!(traceback.starts_with(&format!("{}...", unit.repeat(expected_prefix_count))));
            assert!(traceback.contains("truncated, see trace_id"));
        }
    }

    #[test]
    fn test_from_exception_rejects_human_text_and_windows_drive_as_error_codes() {
        initialize_python();
        for message in [
            r"C:\scene.ma: access denied",
            "C:scene.ma: access denied",
            "https://example.invalid/file: failed",
            "dcc-mcp://host/tool: failed",
            "Could not open: file",
        ] {
            let result =
                py_from_exception(message.to_string(), None, None, false, None, None, None)
                    .unwrap();

            assert_eq!(result.data().error.as_deref(), Some(DEFAULT_ERROR_TYPE));
            assert_eq!(
                result.meta()[META_KEY_DCC_ERROR]["message"],
                serde_json::json!(message)
            );
        }
    }

    #[test]
    fn test_validate_rejects_non_dict_meta() {
        initialize_python();
        Python::attach(|py| -> PyResult<()> {
            let payload = PyDict::new(py);
            payload.set_item("success", false)?;
            payload.set_item("_meta", "not-a-dict")?;

            let error = validate_from_dict(&payload).unwrap_err();
            assert!(error.is_instance_of::<PyTypeError>(py));
            assert!(error.to_string().contains("'_meta' field must be a dict"));
            Ok(())
        })
        .unwrap();
    }
}
