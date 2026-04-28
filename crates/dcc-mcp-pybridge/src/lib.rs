//! dcc-mcp-pybridge: Python<->Rust bridge helpers.
//!
//! Hosts the conversion utilities and type wrappers that used to live in
//! `dcc-mcp-utils` (see [issue #497](https://github.com/loonghao/dcc-mcp-core/issues/497)).
//! Keeping these out of `dcc-mcp-utils` allows pure data crates to opt out of
//! `pyo3` when they only need filesystem helpers and constants.
//!
//! - [`py_json`]: `PyAny <-> serde_json::Value` conversion + `json_dumps`/`json_loads` pyfunctions.
//! - [`py_yaml`]: `yaml_loads`/`yaml_dumps` pyfunctions backed by `serde_yaml_ng`.
//! - [`type_wrappers`]: `BooleanWrapper`/`IntWrapper`/`FloatWrapper`/`StringWrapper` and their
//!   `py_wrap_value`/`py_unwrap_value`/`py_unwrap_parameters` helpers.

#[cfg(feature = "python-bindings")]
pub mod py_json;
#[cfg(feature = "python-bindings")]
pub mod py_yaml;
pub mod type_wrappers;

#[cfg(feature = "python-bindings")]
pub mod python;

#[cfg(feature = "python-bindings")]
pub use python::{json_dumps, json_loads, yaml_dumps, yaml_loads};

/// Shared helpers for PyO3 wrapper boilerplate (issue #490).
///
/// Exposes [`python::wrapper_helpers::build_repr`] and
/// [`python::wrapper_helpers::build_dict`] so other crates can import them as:
/// ```rust,ignore
/// use dcc_mcp_pybridge::python::wrapper_helpers::{build_repr, build_dict};
/// ```
#[cfg(feature = "python-bindings")]
pub use python::wrapper_helpers;
