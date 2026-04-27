//! Type wrappers for safe Python interop via PyO3.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

/// Relative tolerance for floating-point equality comparison.
///
/// Used by `FloatWrapper.__eq__` (Python bindings) and available to
/// pure-Rust consumers for consistent float comparison.
#[cfg_attr(not(feature = "python-bindings"), allow(dead_code))]
const FLOAT_RELATIVE_TOLERANCE: f64 = 1e-9;

/// Boolean wrapper for safe Python interop via PyO3.
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "BooleanWrapper", from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct BooleanWrapper {
    pub value: bool,
}

#[cfg(feature = "python-bindings")]
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

/// Integer wrapper for safe Python interop via PyO3.
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "IntWrapper", from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct IntWrapper {
    pub value: i64,
}

#[cfg(feature = "python-bindings")]
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

/// Float wrapper for safe Python interop via PyO3.
///
/// Note: `FloatWrapper` intentionally does **not** implement `Eq` or `Hash`
/// because `f64` is not `Eq`/`Hash` (NaN != NaN). Python users cannot put
/// `FloatWrapper` instances into `set` or use them as `dict` keys.
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "FloatWrapper", from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FloatWrapper {
    pub value: f64,
}

#[cfg(feature = "python-bindings")]
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
                // Use relative tolerance for large values, absolute for near-zero.
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

/// String wrapper for safe Python interop via PyO3.
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "StringWrapper", from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StringWrapper {
    pub value: String,
}

#[cfg(feature = "python-bindings")]
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

// ── Utility functions ──

#[cfg(test)]
mod tests {
    use super::*;

    // ── BooleanWrapper ──────────────────────────────────────────────────────────

    #[test]
    fn test_boolean_wrapper_true() {
        let w = BooleanWrapper { value: true };
        assert!(w.value);
    }

    #[test]
    fn test_boolean_wrapper_false() {
        let w = BooleanWrapper { value: false };
        assert!(!w.value);
    }

    #[test]
    fn test_boolean_wrapper_default_is_false() {
        let w = BooleanWrapper::default();
        assert!(!w.value);
    }

    #[test]
    fn test_boolean_wrapper_clone_eq() {
        let a = BooleanWrapper { value: true };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn test_boolean_wrapper_debug() {
        let w = BooleanWrapper { value: true };
        let s = format!("{w:?}");
        assert!(s.contains("BooleanWrapper"));
    }

    // ── IntWrapper ──────────────────────────────────────────────────────────────

    #[test]
    fn test_int_wrapper_positive() {
        let w = IntWrapper { value: 42 };
        assert_eq!(w.value, 42);
    }

    #[test]
    fn test_int_wrapper_negative() {
        let w = IntWrapper { value: -100 };
        assert_eq!(w.value, -100);
    }

    #[test]
    fn test_int_wrapper_zero() {
        let w = IntWrapper::default();
        assert_eq!(w.value, 0);
    }

    #[test]
    fn test_int_wrapper_clone_eq() {
        let a = IntWrapper { value: 99 };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn test_int_wrapper_neq() {
        let a = IntWrapper { value: 1 };
        let b = IntWrapper { value: 2 };
        assert_ne!(a, b);
    }

    #[test]
    fn test_int_wrapper_max_min() {
        let max_w = IntWrapper { value: i64::MAX };
        let min_w = IntWrapper { value: i64::MIN };
        assert_eq!(max_w.value, i64::MAX);
        assert_eq!(min_w.value, i64::MIN);
    }

    // ── FloatWrapper ────────────────────────────────────────────────────────────

    #[test]
    fn test_float_wrapper_basic() {
        let w = FloatWrapper { value: 1.5 };
        assert!((w.value - 1.5).abs() < 1e-10);
    }

    #[test]
    fn test_float_wrapper_default_is_zero() {
        let w = FloatWrapper::default();
        assert_eq!(w.value, 0.0);
    }

    #[test]
    fn test_float_wrapper_clone() {
        let a = FloatWrapper { value: 1.618 };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn test_float_wrapper_partial_eq() {
        let a = FloatWrapper { value: 1.0 };
        let b = FloatWrapper { value: 1.0 };
        let c = FloatWrapper { value: 2.0 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_float_wrapper_nan_neq_nan() {
        // NaN != NaN — FloatWrapper is PartialEq but not Eq
        let a = FloatWrapper { value: f64::NAN };
        let b = FloatWrapper { value: f64::NAN };
        assert_ne!(a, b);
    }

    #[test]
    fn test_float_wrapper_infinity() {
        let w = FloatWrapper {
            value: f64::INFINITY,
        };
        assert!(w.value.is_infinite());
    }

    // ── StringWrapper ───────────────────────────────────────────────────────────

    #[test]
    fn test_string_wrapper_basic() {
        let w = StringWrapper {
            value: "hello".to_string(),
        };
        assert_eq!(w.value, "hello");
    }

    #[test]
    fn test_string_wrapper_empty() {
        let w = StringWrapper::default();
        assert!(w.value.is_empty());
    }

    #[test]
    fn test_string_wrapper_clone_eq() {
        let a = StringWrapper {
            value: "world".to_string(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn test_string_wrapper_neq() {
        let a = StringWrapper {
            value: "foo".to_string(),
        };
        let b = StringWrapper {
            value: "bar".to_string(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn test_string_wrapper_unicode() {
        let w = StringWrapper {
            value: "日本語テスト".to_string(),
        };
        assert_eq!(w.value, "日本語テスト");
    }

    #[test]
    fn test_string_wrapper_debug() {
        let w = StringWrapper {
            value: "debug_test".to_string(),
        };
        let s = format!("{w:?}");
        assert!(s.contains("StringWrapper"));
    }

    // ── Hash consistency ─────────────────────────────────────────────────────────

    #[test]
    fn test_wrappers_hash_consistent() {
        use std::collections::HashSet;

        let mut bool_set = HashSet::new();
        bool_set.insert(BooleanWrapper { value: true });
        bool_set.insert(BooleanWrapper { value: false });
        assert_eq!(bool_set.len(), 2);

        let mut int_set = HashSet::new();
        int_set.insert(IntWrapper { value: 1 });
        int_set.insert(IntWrapper { value: 2 });
        int_set.insert(IntWrapper { value: 1 }); // duplicate
        assert_eq!(int_set.len(), 2);

        let mut str_set = HashSet::new();
        str_set.insert(StringWrapper {
            value: "a".to_string(),
        });
        str_set.insert(StringWrapper {
            value: "b".to_string(),
        });
        str_set.insert(StringWrapper {
            value: "a".to_string(),
        }); // duplicate
        assert_eq!(str_set.len(), 2);
    }
}

/// Unwrap a type-wrapper (`BooleanWrapper`, `IntWrapper`, etc.) back to its
/// native Python value.  Non-wrapper values pass through unchanged.
#[cfg(feature = "python-bindings")]
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
#[cfg(feature = "python-bindings")]
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
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "wrap_value")]
pub fn py_wrap_value(py: Python, value: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    // IMPORTANT: Extraction order matters — Python `bool` is a subclass of `int`,
    // and `int` can be extracted as `f64`. So: bool → int → float → string.
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
