//! PyO3 `#[pyfunction]` exports for fast YAML `loads` / `dumps`.

use pyo3::prelude::*;

use crate::py_json::{json_value_to_pyobject, py_any_to_json_value};
use crate::py_yaml::{json_value_to_yaml, yaml_value_to_json};

/// Deserialize a YAML string to a Python object using Rust's serde_yaml_ng.
#[pyfunction]
pub fn yaml_loads(py: Python, s: &str) -> PyResult<Py<PyAny>> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(s)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    let json_value: serde_json::Value = yaml_value_to_json(value);
    json_value_to_pyobject(py, &json_value)
}

/// Serialize a Python object to a YAML string using Rust's serde_yaml_ng.
#[pyfunction]
pub fn yaml_dumps(_py: Python, obj: &Bound<'_, PyAny>) -> PyResult<String> {
    let json_value = py_any_to_json_value(obj)?;
    let yaml_value = json_value_to_yaml(&json_value);
    serde_yaml_ng::to_string(&yaml_value)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
}
