//! ActionResultModel — unified result type for all Action executions.
//!
//! Replaces the Pydantic-based ActionResultModel with a Rust struct exposed via PyO3.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Internal Rust data representation (serde-friendly).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionResultModelData {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

/// Python-facing ActionResultModel.
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ActionResultModel")]
#[derive(Debug, Clone)]
pub struct ActionResultModel {
    inner: ActionResultModelData,
}

#[cfg(not(feature = "python-bindings"))]
#[derive(Debug, Clone)]
pub struct ActionResultModel {
    pub inner: ActionResultModelData,
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
            dict_to_hashmap(dict)?
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
            dict.set_item(k, json_value_to_py(py, v)?)?;
        }
        Ok(dict)
    }

    /// Create a new instance with error information.
    fn with_error(&self, error: String) -> Self {
        let mut data = self.inner.clone();
        data.success = false;
        data.error = Some(error);
        Self { inner: data }
    }

    /// Create a new instance with updated context.
    #[pyo3(signature = (**kwargs))]
    fn with_context(&self, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let mut data = self.inner.clone();
        if let Some(kw) = kwargs {
            for (k, v) in kw.iter() {
                let key: String = k.extract()?;
                let val = py_any_to_json(&v)?;
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
        let ctx = PyDict::new(py);
        for (k, v) in &self.inner.context {
            ctx.set_item(k, json_value_to_py(py, v)?)?;
        }
        dict.set_item("context", ctx)?;
        Ok(dict)
    }

    fn __repr__(&self) -> String {
        format!(
            "ActionResultModel(success={}, message={:?})",
            self.inner.success, self.inner.message
        )
    }

    fn __str__(&self) -> String {
        if self.inner.success {
            format!("Success: {}", self.inner.message)
        } else {
            format!(
                "Error: {}",
                self.inner.error.as_deref().unwrap_or(&self.inner.message)
            )
        }
    }
}

impl ActionResultModel {
    pub fn from_data(data: ActionResultModelData) -> Self {
        Self { inner: data }
    }

    pub fn data(&self) -> &ActionResultModelData {
        &self.inner
    }
}

// ── Factory functions ──

#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "success_result")]
#[pyo3(signature = (message, prompt=None, **context))]
pub fn py_success_result(
    message: String,
    prompt: Option<String>,
    context: Option<&Bound<'_, PyDict>>,
) -> PyResult<ActionResultModel> {
    let ctx = if let Some(dict) = context {
        dict_to_hashmap(dict)?
    } else {
        HashMap::new()
    };
    Ok(ActionResultModel {
        inner: ActionResultModelData {
            success: true,
            message,
            prompt,
            error: None,
            context: ctx,
        },
    })
}

#[cfg(feature = "python-bindings")]
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
    let mut ctx = if let Some(dict) = context {
        dict_to_hashmap(dict)?
    } else {
        HashMap::new()
    };
    if let Some(solutions) = possible_solutions {
        ctx.insert(
            "possible_solutions".to_string(),
            serde_json::Value::Array(
                solutions
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
    Ok(ActionResultModel {
        inner: ActionResultModelData {
            success: false,
            message,
            prompt,
            error: Some(error),
            context: ctx,
        },
    })
}

#[cfg(feature = "python-bindings")]
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
    let mut ctx = if let Some(dict) = context {
        dict_to_hashmap(dict)?
    } else {
        HashMap::new()
    };
    ctx.insert(
        "error_type".to_string(),
        serde_json::Value::String("Exception".to_string()),
    );
    if include_traceback {
        ctx.insert(
            "traceback".to_string(),
            serde_json::Value::String("(traceback from Rust)".to_string()),
        );
    }
    if let Some(solutions) = possible_solutions {
        ctx.insert(
            "possible_solutions".to_string(),
            serde_json::Value::Array(
                solutions
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
    let msg = message.unwrap_or_else(|| format!("Error: {}", error_message));
    let default_prompt = "Please check error details and retry".to_string();
    Ok(ActionResultModel {
        inner: ActionResultModelData {
            success: false,
            message: msg,
            prompt: Some(prompt.unwrap_or(default_prompt)),
            error: Some(error_message),
            context: ctx,
        },
    })
}

#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "validate_action_result")]
pub fn py_validate_action_result(
    py: Python,
    result: &Bound<'_, PyAny>,
) -> PyResult<ActionResultModel> {
    // If already ActionResultModel, clone it
    if let Ok(arm) = result.extract::<ActionResultModel>() {
        return Ok(arm);
    }
    // If dict, try to convert
    if let Ok(dict) = result.downcast::<PyDict>() {
        let success = dict
            .get_item("success")?
            .map(|v| v.extract::<bool>().unwrap_or(true))
            .unwrap_or(true);
        let message = dict
            .get_item("message")?
            .map(|v| v.extract::<String>().unwrap_or_default())
            .unwrap_or_default();
        let prompt = dict
            .get_item("prompt")?
            .and_then(|v| v.extract::<String>().ok());
        let error = dict
            .get_item("error")?
            .and_then(|v| v.extract::<String>().ok());
        return ActionResultModel::new(success, message, prompt, error, Some(dict));
    }
    // Wrap as success
    let msg = format!("{}", result);
    Ok(ActionResultModel {
        inner: ActionResultModelData {
            success: true,
            message: "Successfully processed result".to_string(),
            prompt: None,
            error: None,
            context: {
                let mut m = HashMap::new();
                m.insert("value".to_string(), serde_json::Value::String(msg));
                m
            },
        },
    })
}

// ── Helper functions ──

#[cfg(feature = "python-bindings")]
fn dict_to_hashmap(dict: &Bound<'_, PyDict>) -> PyResult<HashMap<String, serde_json::Value>> {
    let mut map = HashMap::new();
    for (k, v) in dict.iter() {
        let key: String = k.extract()?;
        let val = py_any_to_json(&v)?;
        map.insert(key, val);
    }
    Ok(map)
}

#[cfg(feature = "python-bindings")]
fn py_any_to_json(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if obj.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(serde_json::Value::Number(i.into()));
    }
    if let Ok(f) = obj.extract::<f64>() {
        return Ok(serde_json::json!(f));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(list) = obj.downcast::<pyo3::types::PyList>() {
        let arr: Vec<serde_json::Value> = list
            .iter()
            .map(|item| py_any_to_json(&item))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(serde_json::Value::Array(arr));
    }
    if let Ok(dict) = obj.downcast::<PyDict>() {
        let map = dict_to_hashmap(dict)?;
        return Ok(serde_json::Value::Object(map.into_iter().collect()));
    }
    // Fallback: convert to string
    Ok(serde_json::Value::String(obj.str()?.to_string()))
}

#[cfg(feature = "python-bindings")]
fn json_value_to_py<'py>(py: Python<'py>, val: &serde_json::Value) -> PyResult<Bound<'py, PyAny>> {
    match val {
        serde_json::Value::Null => Ok(py.None().into_bound(py)),
        serde_json::Value::Bool(b) => {
            let obj = pyo3::types::PyBool::new(py, *b);
            Ok(obj.to_owned().into_any())
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.clone().into_any())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.clone().into_any())
            } else {
                Ok(py.None().into_bound(py))
            }
        }
        serde_json::Value::String(s) => Ok(s.into_pyobject(py)?.into_any()),
        serde_json::Value::Array(arr) => {
            let list = pyo3::types::PyList::empty(py);
            for item in arr {
                list.append(json_value_to_py(py, item)?)?;
            }
            Ok(list.into_any())
        }
        serde_json::Value::Object(obj) => {
            let dict = PyDict::new(py);
            for (k, v) in obj {
                dict.set_item(k, json_value_to_py(py, v)?)?;
            }
            Ok(dict.into_any())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_result_default() {
        let data = ActionResultModelData::default();
        assert!(!data.success);
        assert!(data.message.is_empty());
    }

    #[test]
    fn test_action_result_serialization() {
        let data = ActionResultModelData {
            success: true,
            message: "test".to_string(),
            prompt: Some("next step".to_string()),
            error: None,
            context: HashMap::new(),
        };
        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"message\":\"test\""));
    }
}
