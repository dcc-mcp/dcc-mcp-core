//! PyO3 facade for the DCC MCP HTTP server (issue #852).
//!
//! This crate is the Python-binding boundary for `dcc-mcp-http`. The
//! implementation still lives in `dcc-mcp-http::python` for this first
//! extraction step; downstream crates should depend on this crate for Python
//! registration so the implementation can move here module-by-module without
//! changing the root `_core` module again.
//!
//! Dependency direction:
//!
//! ```text
//! dcc-mcp-core (_core extension) → dcc-mcp-http-py → dcc-mcp-http
//! ```

#![forbid(unsafe_code)]

#[cfg(feature = "python-bindings")]
pub use dcc_mcp_http::python::*;

/// Register HTTP Python classes and functions on the root `_core` module.
#[cfg(feature = "python-bindings")]
pub fn register_classes(m: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
    dcc_mcp_http::python::register_classes(m)
}
