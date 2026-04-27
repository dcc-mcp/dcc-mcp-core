//! Type wrappers for safe Python interop via PyO3.
//!
//! PyO3 `#[pymethods]` impls live in `crate::python::type_wrappers`.

/// Relative tolerance for floating-point equality comparison.
///
/// Used by `FloatWrapper.__eq__` (Python bindings) and available to
/// pure-Rust consumers for consistent float comparison.
#[cfg_attr(not(feature = "python-bindings"), allow(dead_code))]
pub(crate) const FLOAT_RELATIVE_TOLERANCE: f64 = 1e-9;

/// Boolean wrapper for safe Python interop via PyO3.
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "BooleanWrapper", from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct BooleanWrapper {
    pub value: bool,
}

/// Integer wrapper for safe Python interop via PyO3.
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "IntWrapper", from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct IntWrapper {
    pub value: i64,
}

/// Float wrapper for safe Python interop via PyO3.
///
/// Note: `FloatWrapper` intentionally does **not** implement `Eq` or `Hash`
/// because `f64` is not `Eq`/`Hash` (NaN != NaN). Python users cannot put
/// `FloatWrapper` instances into `set` or use them as `dict` keys.
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "FloatWrapper", from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FloatWrapper {
    pub value: f64,
}

/// String wrapper for safe Python interop via PyO3.
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "StringWrapper", from_py_object)
)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StringWrapper {
    pub value: String,
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

// `unwrap_value` / `unwrap_parameters` / `wrap_value` `#[pyfunction]` exports
// live in `crate::python::type_wrappers`.
