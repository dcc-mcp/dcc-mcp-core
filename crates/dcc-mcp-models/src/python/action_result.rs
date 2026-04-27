//! PyO3 bindings for `ActionResultModel` / `SerializeFormat` and the
//! `success_result` / `error_result` / `from_exception` /
//! `validate_action_result` / `serialize_result` / `deserialize_result`
//! factory functions.

use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::PyDict;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;

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
const ACTION_RESULT_KNOWN_KEYS: &[&str] = &["success", "message", "prompt", "error"];

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
        Ok(Self::from_data(ActionResultModelData {
            success,
            message,
            prompt,
            error,
            context: ctx,
        }))
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

    /// Create a new instance with error information.
    #[allow(clippy::double_must_use)]
    #[must_use]
    fn with_error(&self, error: String) -> Self {
        let mut data = self.data().clone();
        data.success = false;
        data.error = Some(error);
        Self::from_data(data)
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
        Ok(Self::from_data(data))
    }

    /// Convert to dictionary.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("success", self.data().success)?;
        dict.set_item("message", &self.data().message)?;
        dict.set_item("prompt", self.data().prompt.as_deref())?;
        dict.set_item("error", self.data().error.as_deref())?;
        dict.set_item("context", self.context(py)?)?;
        Ok(dict)
    }

    /// Serialize to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        self.data()
            .to_json_string()
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Iterate over key-value pairs (mapping protocol).
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
            self.data().success,
            self.data().message
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

fn validate_from_dict(dict: &Bound<'_, PyDict>) -> PyResult<ActionResultModel> {
    let success = extract_bool_field(dict, "success", true)?;
    let message = extract_string_field(dict, "message")?;
    let prompt = extract_optional_string_field(dict, "prompt")?;
    let error = extract_optional_string_field(dict, "error")?;

    let mut ctx = HashMap::new();
    for (k, v) in dict.iter() {
        if let Ok(key) = k.extract::<String>() {
            if !ACTION_RESULT_KNOWN_KEYS.contains(&key.as_str()) {
                ctx.insert(key, py_any_to_json_value(&v)?);
            }
        }
    }

    Ok(ActionResultModel::from_data(ActionResultModelData {
        success,
        message,
        prompt,
        error,
        context: ctx,
    }))
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
    Ok(ActionResultModel::from_data(
        ActionResultModelData::success(message, prompt, ctx),
    ))
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
    Ok(ActionResultModel::from_data(
        ActionResultModelData::failure(message, Some(error), prompt, ctx),
    ))
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
    let error_type = error_message
        .split_once(':')
        .map(|(t, _)| t.trim().to_string())
        .unwrap_or_else(|| DEFAULT_ERROR_TYPE.to_string());
    ctx.insert(
        CTX_KEY_ERROR_TYPE.to_string(),
        serde_json::Value::String(error_type),
    );
    let msg = message.unwrap_or_else(|| format!("Error: {error_message}"));
    if include_traceback {
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
    Ok(ActionResultModel::from_data(
        ActionResultModelData::failure(
            msg,
            Some(error_message),
            Some(prompt.unwrap_or_else(|| DEFAULT_ERROR_PROMPT.to_string())),
            ctx,
        ),
    ))
}

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
#[pyfunction]
#[pyo3(name = "serialize_result")]
#[pyo3(signature = (result, format = SerializeFormat::Json))]
pub fn py_serialize_result(
    py: Python<'_>,
    result: &ActionResultModel,
    format: SerializeFormat,
) -> PyResult<Py<PyAny>> {
    let bytes = result
        .data()
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
    Ok(ActionResultModel::from_data(data))
}
