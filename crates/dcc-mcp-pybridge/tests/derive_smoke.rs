//! Compile-time smoke test for `#[derive(PyWrapper)]` codegen (issue #528, M2).
//!
//! Exercises the generated `#[pymethods]` impl block for both the
//! delegation pattern (wrapper holds an `inner` field) and the
//! direct-pyclass pattern (the type *is* the pyclass).
//!
//! By workspace convention, runtime PyO3 behaviour is exercised from
//! Python via pytest — Rust-side tests only verify that the proc-macro
//! emits code that **compiles** against the real `pyo3::pymethods`
//! contract. If that holds, the generated symbols can be invoked from
//! Python with the same semantics as hand-written accessors.
//!
//! Gated behind `python-bindings` so non-pyo3 builds still link.

#![cfg(feature = "python-bindings")]

use dcc_mcp_pybridge::derive::PyWrapper;
use pyo3::prelude::*;

// ---------------------------------------------------------------- delegation

#[derive(Default)]
pub struct InnerCfg {
    pub port: u16,
    pub host: String,
    pub tags: Vec<String>,
}

#[pyclass(name = "WrapperCfg")]
#[derive(PyWrapper)]
#[py_wrapper(
    inner = "InnerCfg",
    fields(
        port: u16            => [get, set, repr, dict],
        host: String         => [get(by_str), repr, dict],
        tags: Vec<String>    => [get(clone), set, dict],
    ),
)]
pub struct WrapperCfg {
    pub inner: InnerCfg,
}

#[pymethods]
impl WrapperCfg {
    #[new]
    fn new() -> Self {
        Self {
            inner: InnerCfg::default(),
        }
    }
}

#[test]
fn delegation_pattern_compiles() {
    // The proc-macro must emit a `#[pyo3::pymethods] impl WrapperCfg`
    // block alongside the hand-written one above (PyO3
    // `multiple-pymethods`). If either block fails to compile against
    // the current PyO3 ABI the test crate won't build.
    let _ = std::mem::size_of::<WrapperCfg>();
}

// ------------------------------------------------------------- direct pyclass

#[pyclass(name = "DirectStyle")]
#[derive(PyWrapper)]
#[py_wrapper(
    fields(
        name: String => [get(by_str), set, repr],
    ),
)]
pub struct DirectStyle {
    pub name: String,
}

#[pymethods]
impl DirectStyle {
    #[new]
    fn new() -> Self {
        Self {
            name: String::new(),
        }
    }
}

#[test]
fn direct_pattern_compiles() {
    let _ = std::mem::size_of::<DirectStyle>();
}

// --------------------------------------------------------- get(to_string) mode

/// A type whose canonical Python serialisation is `Display`. Mimics the
/// real-world pattern in `PyMcpHttpConfig::host()` where the inner field
/// is a non-`String` type (e.g. `IpAddr`, `Url`) but Python sees a plain
/// `str` produced via `to_string()`.
#[derive(Default)]
pub struct DisplayHolder {
    pub label: u32,
}

impl std::fmt::Display for DisplayHolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "label-{}", self.label)
    }
}

#[derive(Default)]
pub struct InnerToString {
    pub label: DisplayHolder,
}

#[pyclass(name = "ToStringWrapper")]
#[derive(PyWrapper)]
#[py_wrapper(
    inner = "InnerToString",
    fields(
        label: String => [get(to_string)],
    ),
)]
pub struct ToStringWrapper {
    pub inner: InnerToString,
}

#[pymethods]
impl ToStringWrapper {
    #[new]
    fn new() -> Self {
        Self {
            inner: InnerToString::default(),
        }
    }
}

#[test]
fn to_string_mode_compiles() {
    let _ = std::mem::size_of::<ToStringWrapper>();
}
