//! PyO3 `#[pyfunction]` exports for fast JSON `dumps` / `loads`.
//!
//! The conversion helpers (`py_any_to_json_value` etc.) remain in
//! `crate::py_json` because they're shared with other crates that build
//! their own bindings on top.

use pyo3::prelude::*;

use crate::py_json::{json_value_to_pyobject, py_any_to_json_value, unescape_unicode_json};

/// Serialize a Python object to a JSON string using Rust's serde_json.
///
/// This is the native backend for dcc-mcp-core's dependency-light JSON API.
/// It accepts the package's compatibility parameters but is not a complete
/// replacement for Python's ``json.dumps``.  Formatting, numeric limits,
/// non-finite floats, accepted container/object types, keyword arguments, and
/// file APIs follow the narrower PyO3/serde bridge contract.
///
/// Parameters
/// ----------
/// obj : Any
///     The Python object to serialize.
/// ensure_ascii : bool, optional
///     Compatibility parameter retained by the package API. Exact stdlib
///     Unicode escaping parity is not currently part of the contract.
/// indent : int or None, optional
///     If given, enable serde_json pretty-printing.  Exact stdlib indentation
///     width parity is not currently part of the contract.
#[pyfunction]
#[pyo3(signature = (obj, *, ensure_ascii=true, indent=None))]
pub fn json_dumps(
    _py: Python,
    obj: &Bound<'_, PyAny>,
    ensure_ascii: bool,
    indent: Option<usize>,
) -> PyResult<String> {
    let value = py_any_to_json_value(obj)?;
    let s = match indent {
        Some(_) => serde_json::to_string_pretty(&value),
        None => serde_json::to_string(&value),
    }
    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;

    if ensure_ascii {
        Ok(s)
    } else {
        Ok(unescape_unicode_json(&s))
    }
}

/// Deserialize a JSON string to a Python object using Rust's serde_json.
///
/// This is the native backend for dcc-mcp-core's dependency-light JSON API,
/// not a complete replacement for Python's ``json.loads``.  It accepts text
/// and returns the subset representable by the shared PyO3/serde bridge.
#[pyfunction]
pub fn json_loads(py: Python, s: &str) -> PyResult<Py<PyAny>> {
    let value: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    json_value_to_pyobject(py, &value)
}
