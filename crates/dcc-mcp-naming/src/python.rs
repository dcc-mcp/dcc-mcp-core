//! PyO3 bindings for the naming validators.
//!
//! Exposes:
//!
//! * `validate_tool_name(name: str) -> None`
//! * `validate_action_id(name: str) -> None`
//! * `TOOL_NAME_RE: str`
//! * `ACTION_ID_RE: str`
//! * `MAX_TOOL_NAME_LEN: int`
//!
//! The two `validate_*` functions raise `ValueError` with a human-readable
//! message on failure; on success they return `None`. This matches the
//! convention used by the other validators in `dcc_mcp_core` (e.g. the
//! SandboxPolicy input validator).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pyfunction;

use crate::{ACTION_ID_RE, MAX_TOOL_NAME_LEN, NamingError, TOOL_NAME_RE};

fn to_py_err(e: NamingError) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Validate an MCP wire-visible tool name.
///
/// Raises ``ValueError`` on any violation; returns ``None`` on success.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "validate_tool_name", text_signature = "(name, /)")]
pub fn py_validate_tool_name(name: &str) -> PyResult<()> {
    crate::validate_tool_name(name).map_err(to_py_err)
}

/// Validate an internal action id.
///
/// Raises ``ValueError`` on any violation; returns ``None`` on success.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "validate_action_id", text_signature = "(name, /)")]
pub fn py_validate_action_id(name: &str) -> PyResult<()> {
    crate::validate_action_id(name).map_err(to_py_err)
}

/// Register naming symbols on a Python module.
///
/// Called from the top-level `_core` PyO3 module entrypoint.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py_validate_tool_name, m)?)?;
    m.add_function(wrap_pyfunction!(py_validate_action_id, m)?)?;
    m.add("TOOL_NAME_RE", TOOL_NAME_RE)?;
    m.add("ACTION_ID_RE", ACTION_ID_RE)?;
    m.add("MAX_TOOL_NAME_LEN", MAX_TOOL_NAME_LEN)?;
    Ok(())
}
