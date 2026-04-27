//! PyO3 `#[pyfunction]` exports for fast JSON `dumps` / `loads`.
//!
//! The conversion helpers (`py_any_to_json_value` etc.) remain in
//! `crate::py_json` because they're shared with other crates that build
//! their own bindings on top.

use pyo3::prelude::*;

use crate::py_json::{json_value_to_pyobject, py_any_to_json_value, unescape_unicode_json};

/// Serialize a Python object to a JSON string using Rust's serde_json.
///
/// This is a high-performance drop-in replacement for `json.dumps()`.
/// Equivalent to: ``json.dumps(obj, ensure_ascii=True, indent=None)``.
///
/// Parameters
/// ----------
/// obj : Any
///     The Python object to serialize.
/// ensure_ascii : bool, optional
///     If True (default), escape non-ASCII characters. If False, output
///     raw Unicode characters.
/// indent : int or None, optional
///     If given, pretty-print with the specified number of spaces.
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
/// This is a high-performance drop-in replacement for `json.loads()`.
/// Returns a Python dict, list, string, number, bool, or None.
#[pyfunction]
pub fn json_loads(py: Python, s: &str) -> PyResult<Py<PyAny>> {
    let value: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    json_value_to_pyobject(py, &value)
}
