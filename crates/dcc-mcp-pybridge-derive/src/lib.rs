//! Procedural derive macros for `dcc-mcp-pybridge` (issue #528).
//!
//! This crate is the procedural-macro counterpart of
//! `dcc-mcp-pybridge::python::wrapper_helpers`: where the helper module
//! collapses runtime boilerplate (`__repr__` / `to_dict` formatting),
//! this crate collapses *compile-time* boilerplate \u2014 the dozens of
//! mechanical `#[getter] fn x(&self) { self.inner.x }` blocks that
//! every wrapper currently hand-writes.
//!
//! ## Status
//!
//! M1 (skeleton). The `#[derive(PyWrapper)]` derive currently parses
//! the input but emits **no code**. Field-level attribute parsing and
//! code generation land in M2 (PR for issue #528). This skeleton lets
//! downstream crates start importing the symbol so the M2 PR is a pure
//! diff inside this crate.
//!
//! ## Usage (planned, M2)
//!
//! ```ignore
//! use dcc_mcp_pybridge::derive::PyWrapper;
//!
//! #[derive(PyWrapper)]
//! #[py_wrapper(
//!     inner = "McpHttpConfig",
//!     fields(
//!         port: u16   => [get, set, repr],
//!         host: String => [get(by_str), repr],
//!     ),
//! )]
//! #[pyclass(name = "McpHttpConfig")]
//! pub struct PyMcpHttpConfig {
//!     pub(crate) inner: McpHttpConfig,
//! }
//! ```
//!
//! See issue #528 for the full design and grammar.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

/// Procedural derive that generates `#[pymethods]` accessors from a
/// `#[py_wrapper(\u2026)]` field declaration table.
///
/// **M1 (skeleton)**: parses the input but emits no code. Useful only to
/// reserve the symbol so downstream crates can wire imports ahead of the
/// M2 codegen PR. Applying the derive on a struct compiles cleanly and
/// has zero effect on the resulting binary.
#[proc_macro_derive(PyWrapper, attributes(py_wrapper))]
pub fn derive_py_wrapper(input: TokenStream) -> TokenStream {
    let _input = parse_macro_input!(input as DeriveInput);
    // M1 stub: no-op codegen. The full implementation lands in M2.
    quote!().into()
}
