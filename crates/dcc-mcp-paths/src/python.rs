//! PyO3 bindings for the platform-directory helpers.
//!
//! Lives under `python.rs` per the workspace convention codified in
//! issue #501 — every `#[pyfunction]` / `#[pyclass]` / `#[pymethods]`
//! lives in a `python.rs` (or `python/` sub-module) so that
//! `rg "#\[pyclass\]|#\[pyfunction\]" crates/*/src --glob "!**/python.rs"
//! --glob "!**/python/**"` returns nothing.

use super::{get_config_dir, get_data_dir, get_log_dir, get_platform_dir, get_tools_dir};
use pyo3::prelude::*;

macro_rules! py_dir_binding {
    ($py_fn:ident, $pyo3_name:literal, $rust_fn:ident) => {
        #[pyfunction]
        #[pyo3(name = $pyo3_name)]
        pub fn $py_fn() -> PyResult<String> {
            Ok($rust_fn()?)
        }
    };
    ($py_fn:ident, $pyo3_name:literal, $rust_fn:ident, $arg:ident : $ty:ty) => {
        #[pyfunction]
        #[pyo3(name = $pyo3_name)]
        pub fn $py_fn($arg: $ty) -> PyResult<String> {
            Ok($rust_fn($arg)?)
        }
    };
}

py_dir_binding!(py_get_config_dir, "get_config_dir", get_config_dir);
py_dir_binding!(py_get_data_dir, "get_data_dir", get_data_dir);
py_dir_binding!(py_get_log_dir, "get_log_dir", get_log_dir);
py_dir_binding!(py_get_platform_dir, "get_platform_dir", get_platform_dir, dir_type: &str);
py_dir_binding!(py_get_tools_dir, "get_tools_dir", get_tools_dir, dcc_name: &str);
