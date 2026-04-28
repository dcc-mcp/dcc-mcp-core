//! Shared helpers for PyO3 wrapper boilerplate (issue #490).
//!
//! Provides [`build_repr`] for `__repr__` implementations and
//! [`build_dict`] for `to_dict` methods, eliminating repeated
//! `format!(…)` and `PyDict::new` / `set_item` chains across crates.

use std::fmt;

use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Build a `__repr__` string from a type name and an iterator of
/// `(field_name, &dyn Debug)` pairs.
///
/// Each value is formatted with `{:?}` (the `Debug` trait). Because Rust
/// numeric types produce identical output under both `{}` and `{:?}`, this is
/// a drop-in replacement for most multi-arg `format!("Foo(a={}, b={:?})"…)`
/// patterns.
///
/// # Example
///
/// ```rust,ignore
/// fn __repr__(&self) -> String {
///     build_repr("Foo", [
///         ("a", &self.a as &dyn Debug),
///         ("b", &self.b as &dyn Debug),
///     ])
/// }
/// ```
///
/// Produces `Foo(a=42, b="hello")`.
pub fn build_repr<'a>(
    type_name: &str,
    pairs: impl IntoIterator<Item = (&'static str, &'a dyn fmt::Debug)>,
) -> String {
    let parts: Vec<String> = pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v:?}"))
        .collect();
    format!("{}({})", type_name, parts.join(", "))
}

/// Build a [`PyDict`] from an iterator of `(key, value)` pairs where
/// values are already boxed as [`PyObject`].
///
/// Eliminates the boilerplate:
/// ```rust,ignore
/// let dict = PyDict::new(py);
/// dict.set_item("a", val_a)?;
/// dict.set_item("b", val_b)?;
/// Ok(dict)
/// ```
///
/// # Example
///
/// ```rust,ignore
/// fn to_dict(&self, py: Python) -> PyResult<Bound<'_, PyDict>> {
///     build_dict(py, [
///         ("name",  self.name.clone().into_pyobject(py)?.into_any().unbind()),
///         ("value", self.value.into_pyobject(py)?.into_any().unbind()),
///     ])
/// }
/// ```
///
/// Note: `Py<PyAny>` is `PyObject`. Use `.into_pyobject(py)?.into_any().unbind()`
/// to convert a Rust value to `Py<PyAny>`.
pub fn build_dict<'py>(
    py: Python<'py>,
    pairs: impl IntoIterator<Item = (&'static str, Py<PyAny>)>,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, val) in pairs {
        dict.set_item(key, val)?;
    }
    Ok(dict)
}

/// Expand a list of `("key", value)` pairs into a populated [`PyDict`]
/// returned as `PyResult<Py<PyAny>>`.
///
/// Eliminates the three-line boilerplate that every `to_dict` method
/// repeats:
/// ```rust,ignore
/// let dict = PyDict::new(py);
/// dict.set_item("k", v)?;
/// Ok(dict.unbind().into_any())
/// ```
///
/// # Example
///
/// ```rust,ignore
/// fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
///     to_dict_pairs!(py, [
///         ("success", self.data().success),
///         ("message", &self.data().message),
///     ])
/// }
/// ```
#[macro_export]
macro_rules! to_dict_pairs {
    ($py:expr, [ $( ($key:literal, $val:expr) ),* $(,)? ]) => {{
        let _dict = pyo3::types::PyDict::new($py);
        $( _dict.set_item($key, $val)?; )*
        Ok(_dict.unbind().into_any())
    }};
}
pub use to_dict_pairs;

/// Build a `__repr__` string from a type-name literal and a list of
/// `("key", value)` pairs, formatting each value with `{:?}`.
///
/// Unlike [`build_repr`] this macro accepts heterogeneous value types
/// without requiring explicit `as &dyn fmt::Debug` casts.
///
/// # Example
///
/// ```rust,ignore
/// fn __repr__(&self) -> String {
///     dcc_mcp_pybridge::repr_pairs!("Foo", [
///         ("a", self.a),
///         ("b", self.b),
///     ])
/// }
/// ```
///
/// Produces `Foo(a=42, b="hello")`.
#[macro_export]
macro_rules! repr_pairs {
    ($type_name:literal, [ $( ($key:literal, $val:expr) ),* $(,)? ]) => {{
        let mut _parts: Vec<String> = Vec::new();
        $( _parts.push(format!("{}={:?}", $key, $val)); )*
        format!("{}({})", $type_name, _parts.join(", "))
    }};
}
pub use repr_pairs;
