//! PyO3 bindings for `dcc-mcp-models`.
//!
//! Per workspace convention (#501), every `#[pymethods]` /
//! `#[pyfunction]` block in this crate lives below `src/python/`.

mod action_result;
mod skill_metadata;
mod skill_scope;
mod tool_declaration;

pub use action_result::{
    py_deserialize_result, py_error_result, py_from_exception, py_serialize_result,
    py_success_result, py_validate_action_result,
};
