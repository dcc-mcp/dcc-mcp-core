//! PyO3 bindings for `BooleanWrapper` / `IntWrapper` / `FloatWrapper` /
//! `StringWrapper`.

use pyo3::prelude::*;

use crate::type_wrappers::{
    BooleanWrapper, FLOAT_RELATIVE_TOLERANCE, FloatWrapper, IntWrapper, StringWrapper,
};

#[pymethods]
impl BooleanWrapper {
    #[new]
    fn new(value: bool) -> Self {
        Self { value }
    }
    #[getter]
    fn get_value(&self) -> bool {
        self.value
    }
    fn __bool__(&self) -> bool {
        self.value
    }
    fn __repr__(&self) -> String {
        format!(
            "BooleanWrapper({})",
            if self.value { "True" } else { "False" }
        )
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<bool>()
            .map(|b| b == self.value)
            .unwrap_or(false)
    }
    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.value.hash(&mut hasher);
        hasher.finish()
    }
}

#[pymethods]
impl IntWrapper {
    #[new]
    fn new(value: i64) -> Self {
        Self { value }
    }
    #[getter]
    fn get_value(&self) -> i64 {
        self.value
    }
    fn __int__(&self) -> i64 {
        self.value
    }
    fn __index__(&self) -> i64 {
        self.value
    }
    fn __repr__(&self) -> String {
        format!("IntWrapper({})", self.value)
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<i64>()
            .map(|i| i == self.value)
            .unwrap_or(false)
    }
    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.value.hash(&mut hasher);
        hasher.finish()
    }
}

#[pymethods]
impl FloatWrapper {
    #[new]
    fn new(value: f64) -> Self {
        Self { value }
    }
    #[getter]
    fn get_value(&self) -> f64 {
        self.value
    }
    fn __float__(&self) -> f64 {
        self.value
    }
    fn __repr__(&self) -> String {
        format!("FloatWrapper({})", self.value)
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<f64>()
            .map(|f| {
                let abs_diff = (f - self.value).abs();
                let max_abs = f.abs().max(self.value.abs());
                if max_abs == 0.0 {
                    abs_diff == 0.0
                } else {
                    abs_diff / max_abs < FLOAT_RELATIVE_TOLERANCE
                }
            })
            .unwrap_or(false)
    }
    fn __hash__(&self) -> PyResult<u64> {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "unhashable type: 'FloatWrapper'",
        ))
    }
}

#[pymethods]
impl StringWrapper {
    #[new]
    fn new(value: String) -> Self {
        Self { value }
    }
    #[getter]
    fn get_value(&self) -> &str {
        &self.value
    }
    fn __str__(&self) -> &str {
        &self.value
    }
    fn __repr__(&self) -> String {
        format!("StringWrapper({:?})", self.value)
    }
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<String>()
            .map(|s| s == self.value)
            .unwrap_or(false)
    }
    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.value.hash(&mut hasher);
        hasher.finish()
    }
}

/// Unwrap a type-wrapper (`BooleanWrapper`, `IntWrapper`, etc.) back to its
/// native Python value.  Non-wrapper values pass through unchanged.
#[pyfunction]
#[pyo3(name = "unwrap_value")]
pub fn py_unwrap_value(py: Python, value: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    if let Ok(w) = value.extract::<BooleanWrapper>() {
        let obj = pyo3::types::PyBool::new(py, w.value);
        return Ok(obj.to_owned().into_any().unbind());
    }
    if let Ok(w) = value.extract::<IntWrapper>() {
        return Ok(w.value.into_pyobject(py)?.clone().into_any().unbind());
    }
    if let Ok(w) = value.extract::<FloatWrapper>() {
        return Ok(w.value.into_pyobject(py)?.clone().into_any().unbind());
    }
    if let Ok(w) = value.extract::<StringWrapper>() {
        return Ok(w.value.into_pyobject(py)?.clone().into_any().unbind());
    }
    Ok(value.clone().unbind())
}

/// Unwrap all type-wrapper values in a dict, returning a new dict with native
/// Python values.  Non-wrapper values pass through unchanged.
#[pyfunction]
#[pyo3(name = "unwrap_parameters")]
pub fn py_unwrap_parameters(
    py: Python,
    params: &Bound<'_, pyo3::types::PyDict>,
) -> PyResult<Py<PyAny>> {
    let result = pyo3::types::PyDict::new(py);
    for (k, v) in params.iter() {
        let unwrapped = py_unwrap_value(py, &v)?;
        result.set_item(k, unwrapped)?;
    }
    Ok(result.unbind().into_any())
}

/// Wrap a native Python value (`bool`, `int`, `float`, `str`) into the
/// corresponding type-wrapper.  Unsupported types pass through unchanged.
///
/// Extraction order: bool → int → float → string (Python `bool` is a
/// subclass of `int`, and `int` can be extracted as `f64`).
#[pyfunction]
#[pyo3(name = "wrap_value")]
pub fn py_wrap_value(py: Python, value: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    if let Ok(b) = value.extract::<bool>() {
        return Ok(BooleanWrapper { value: b }
            .into_pyobject(py)?
            .into_any()
            .unbind());
    }
    if let Ok(i) = value.extract::<i64>() {
        return Ok(IntWrapper { value: i }
            .into_pyobject(py)?
            .into_any()
            .unbind());
    }
    if let Ok(f) = value.extract::<f64>() {
        return Ok(FloatWrapper { value: f }
            .into_pyobject(py)?
            .into_any()
            .unbind());
    }
    if let Ok(s) = value.extract::<String>() {
        return Ok(StringWrapper { value: s }
            .into_pyobject(py)?
            .into_any()
            .unbind());
    }
    Ok(value.clone().unbind())
}
