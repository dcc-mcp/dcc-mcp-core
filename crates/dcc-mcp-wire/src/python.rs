//! Python bindings for canonical MCP wire normalization.

use dcc_mcp_pybridge::py_json::{json_value_to_pyobject, py_any_to_json_value};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyAny;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyfunction;

type PyObj = Py<PyAny>;

fn wire_error_to_py(err: crate::WireError) -> PyErr {
    PyValueError::new_err(format!("{}: {}", err.kind(), err))
}

/// Normalize Python ``arguments`` for MCP ``tools/call`` and REST ``/v1/call``.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "normalize_tool_arguments", signature = (arguments=None))]
pub fn py_normalize_tool_arguments(
    py: Python<'_>,
    arguments: Option<&Bound<'_, PyAny>>,
) -> PyResult<PyObj> {
    let value = arguments.map(py_any_to_json_value).transpose()?;
    let normalized = crate::normalize_arguments(value).map_err(wire_error_to_py)?;
    json_value_to_pyobject(py, &normalized)
}

/// Normalize Python ``_meta`` to an object or ``None``.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "normalize_tool_meta", signature = (meta=None))]
pub fn py_normalize_tool_meta(py: Python<'_>, meta: Option<&Bound<'_, PyAny>>) -> PyResult<PyObj> {
    let value = meta.map(py_any_to_json_value).transpose()?;
    match crate::normalize_meta(value).map_err(wire_error_to_py)? {
        Some(map) => json_value_to_pyobject(py, &serde_json::Value::Object(map)),
        None => Ok(py.None()),
    }
}

/// Register wire-normalization functions on the top-level Python extension.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py_normalize_tool_arguments, m)?)?;
    m.add_function(wrap_pyfunction!(py_normalize_tool_meta, m)?)?;
    Ok(())
}
