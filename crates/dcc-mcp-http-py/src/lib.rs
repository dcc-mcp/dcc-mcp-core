//! PyO3 facade for the DCC MCP HTTP server (issue #852).
//!
//! This crate is the Python-binding boundary for `dcc-mcp-http`. Modules move
//! here incrementally; during the transition, unmoved bindings are still
//! delegated to `dcc-mcp-http::python`.
//!
//! Dependency direction:
//!
//! ```text
//! dcc-mcp-core (_core extension) → dcc-mcp-http-py → dcc-mcp-http
//! ```

#![forbid(unsafe_code)]

#[cfg(feature = "python-bindings")]
use pyo3::types::PyModuleMethods;

#[cfg(feature = "python-bindings")]
pub mod workspace;

#[cfg(feature = "python-bindings")]
pub use dcc_mcp_http::python::*;
#[cfg(feature = "python-bindings")]
pub use workspace::PyWorkspaceRoots;

/// Register HTTP Python classes and functions on the root `_core` module.
#[cfg(feature = "python-bindings")]
pub fn register_classes(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    m.add_class::<PyWorkspaceRoots>()?;
    dcc_mcp_http::python::register_classes(m)
}
