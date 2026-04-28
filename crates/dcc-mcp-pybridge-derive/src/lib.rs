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
//! M2: full codegen. `#[derive(PyWrapper)]` reads the
//! `#[py_wrapper(inner = "Foo", fields(...))]` attribute and emits a
//! `#[pyo3::pymethods]` impl block containing one accessor per requested
//! mode plus aggregated `__repr__` / `to_dict` if any field opts in.
//!
//! ## Usage
//!
//! ```ignore
//! use dcc_mcp_pybridge::derive::PyWrapper;
//!
//! #[derive(PyWrapper)]
//! #[py_wrapper(
//!     inner = "McpHttpConfig",
//!     fields(
//!         port: u16    => [get, set, repr],
//!         host: String => [get(by_str), repr],
//!         tags: Vec<String> => [get(clone), set],
//!     ),
//! )]
//! #[pyclass(name = "McpHttpConfig")]
//! pub struct PyMcpHttpConfig {
//!     pub(crate) inner: McpHttpConfig,
//! }
//! ```
//!
//! See issue #528 for the full design and grammar.

mod codegen;
mod parse;

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use crate::parse::PyWrapperAttr;

/// Procedural derive that generates `#[pyo3::pymethods]` accessors from
/// a `#[py_wrapper(...)]` field declaration table.
///
/// Returns a compile error if the input struct lacks a `#[py_wrapper(...)]`
/// attribute, or if the attribute fails to parse.
#[proc_macro_derive(PyWrapper, attributes(py_wrapper))]
pub fn derive_py_wrapper(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_ident = input.ident.clone();

    // Collect the (one and only) `#[py_wrapper(...)]` attribute.
    let attrs: Vec<&syn::Attribute> = input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("py_wrapper"))
        .collect();
    let attr = match attrs.as_slice() {
        [] => {
            return syn::Error::new_spanned(
                &input.ident,
                "PyWrapper: missing required `#[py_wrapper(...)]` attribute",
            )
            .to_compile_error()
            .into();
        }
        [a] => *a,
        [first, second, ..] => {
            let mut err = syn::Error::new_spanned(
                second,
                "PyWrapper: only one `#[py_wrapper(...)]` attribute permitted",
            );
            err.combine(syn::Error::new_spanned(first, "first attribute is here"));
            return err.to_compile_error().into();
        }
    };

    let parsed: PyWrapperAttr = match attr.parse_args() {
        Ok(p) => p,
        Err(e) => return e.to_compile_error().into(),
    };

    let generated = codegen::generate(&struct_ident, &parsed);
    quote! { #generated }.into()
}
