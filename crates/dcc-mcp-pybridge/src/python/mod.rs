//! PyO3 bindings for `dcc-mcp-pybridge`.
//!
//! Per workspace convention (#501), every `#[pymethods]` /
//! `#[pyfunction]` block in this crate lives below `src/python/`.
//! Conversion helpers (`py_any_to_json_value`, `yaml_value_to_json`, …)
//! remain in `crate::py_json` / `crate::py_yaml` because they're shared
//! with other crates that build their own bindings on top.

mod py_json;
mod py_yaml;
mod type_wrappers;

pub use py_json::{json_dumps, json_loads};
pub use py_yaml::{yaml_dumps, yaml_loads};
pub use type_wrappers::{py_unwrap_parameters, py_unwrap_value, py_wrap_value};
