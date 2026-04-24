//! Shared helpers for the pipeline PyO3 bindings.
//!
//! - [`value_to_py`] — recursive conversion from [`serde_json::Value`] to a
//!   Python object.
//! - [`PyCallableHook`] — struct carrying optional `before_fn` / `after_fn`
//!   Python callables attached via `PyActionPipeline::add_callable()`.

use pyo3::Py;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

/// Convert a [`serde_json::Value`] into a Python object, recursively.
pub(super) fn value_to_py(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => Ok((*b).into_pyobject(py)?.to_owned().into()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into())
            } else {
                Ok(n.as_f64().unwrap_or(f64::NAN).into_pyobject(py)?.into())
            }
        }
        Value::String(s) => Ok(s.as_str().into_pyobject(py)?.into()),
        Value::Array(arr) => {
            let list = pyo3::types::PyList::empty(py);
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

/// Python callable pair that acts as middleware.
///
/// Unlike the Rust `ActionMiddleware` trait, these are called directly from
/// `PyActionPipeline::dispatch()` because they need the GIL, which is already
/// held inside `#[pymethods]`.
pub(super) struct PyCallableHook {
    pub(super) before_fn: Option<Py<PyAny>>,
    pub(super) after_fn: Option<Py<PyAny>>,
}
