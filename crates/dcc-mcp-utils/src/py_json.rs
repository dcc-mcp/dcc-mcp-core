//! Unified Python ↔ serde_json::Value conversion utilities.
//!
//! Provides a single source of truth for converting between Python objects and
//! `serde_json::Value`, eliminating duplicate implementations across crates.

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyList};
use std::collections::HashMap;

/// Convert a Python object to a `serde_json::Value`.
///
/// Extraction order matters — Python `bool` is a subclass of `int`,
/// and `int` can be extracted as `f64`. So: bool → int → float → string.
pub fn py_any_to_json_value(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
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
        // JSON does not support NaN/Infinity — fall back to Null
        return if f.is_finite() {
            Ok(serde_json::json!(f))
        } else {
            Ok(serde_json::Value::Null)
        };
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(list) = obj.downcast::<PyList>() {
        let arr: Vec<serde_json::Value> = list
            .iter()
            .map(|item| py_any_to_json_value(&item))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(serde_json::Value::Array(arr));
    }
    if let Ok(dict) = obj.downcast::<PyDict>() {
        return Ok(serde_json::Value::Object(py_dict_to_json_object(dict)?));
    }
    // Fallback: convert to string
    Ok(serde_json::Value::String(obj.str()?.to_string()))
}

/// Convert a `serde_json::Value` to a Python object (bound).
pub fn json_value_to_bound_py<'py>(
    py: Python<'py>,
    val: &serde_json::Value,
) -> PyResult<Bound<'py, PyAny>> {
    match val {
        serde_json::Value::Null => Ok(py.None().into_bound(py)),
        serde_json::Value::Bool(b) => Ok(PyBool::new(py, *b).to_owned().into_any()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.clone().into_any())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.clone().into_any())
            } else {
                tracing::warn!(
                    "serde_json::Number {n} cannot be represented as i64 or f64 — returning None"
                );
                Ok(py.None().into_bound(py))
            }
        }
        serde_json::Value::String(s) => Ok(s.into_pyobject(py)?.into_any()),
        serde_json::Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_value_to_bound_py(py, item)?)?;
            }
            Ok(list.into_any())
        }
        serde_json::Value::Object(obj) => {
            let dict = PyDict::new(py);
            for (k, v) in obj {
                dict.set_item(k, json_value_to_bound_py(py, v)?)?;
            }
            Ok(dict.into_any())
        }
    }
}

/// Convert a `serde_json::Value` to an unbound `PyObject`.
///
/// Convenience wrapper around [`json_value_to_bound_py`] that calls `.unbind()`.
pub fn json_value_to_pyobject(py: Python, val: &serde_json::Value) -> PyResult<PyObject> {
    Ok(json_value_to_bound_py(py, val)?.unbind())
}

/// Convert a Python dict to a `serde_json::Map<String, serde_json::Value>`.
///
/// Used when building a `serde_json::Value::Object` directly (avoids the
/// intermediate `HashMap` allocation that [`py_dict_to_json_map`] requires).
fn py_dict_to_json_object(
    dict: &Bound<'_, PyDict>,
) -> PyResult<serde_json::Map<String, serde_json::Value>> {
    let mut map = serde_json::Map::with_capacity(dict.len());
    for (k, v) in dict.iter() {
        let key: String = k.extract()?;
        let val = py_any_to_json_value(&v)?;
        map.insert(key, val);
    }
    Ok(map)
}

/// Convert a Python dict to a `HashMap<String, serde_json::Value>`.
///
/// Delegates to [`py_dict_to_json_object`] and collects into a `HashMap`,
/// avoiding duplicate iteration logic.
pub fn py_dict_to_json_map(
    dict: &Bound<'_, PyDict>,
) -> PyResult<HashMap<String, serde_json::Value>> {
    Ok(py_dict_to_json_object(dict)?.into_iter().collect())
}
