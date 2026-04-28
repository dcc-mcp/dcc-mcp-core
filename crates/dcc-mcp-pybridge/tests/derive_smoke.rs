//! Smoke test for the `#[derive(PyWrapper)]` re-export (issue #528, M1).
//!
//! Verifies that:
//! 1. The derive macro is reachable via the `dcc_mcp_pybridge::derive` module.
//! 2. Applying it to a struct compiles cleanly with **no** generated symbols
//!    (M1 stub semantics) \u2014 i.e. the original struct still has only the
//!    fields the user wrote, and instantiation through the standard
//!    field-init syntax still works.
//!
//! The full codegen path is exercised by `macrotest` snapshots in M2.

use dcc_mcp_pybridge::derive::PyWrapper;

/// Plain struct that opts into the derive. The `#[py_wrapper(\u2026)]`
/// attribute is parsed but ignored in M1.
#[derive(PyWrapper)]
#[py_wrapper(
    inner = "InnerCfg",
    fields(
        port: u16 => [get, set, repr],
        host: String => [get, repr],
    ),
)]
pub struct WrapperCfg {
    pub inner: InnerCfg,
}

#[derive(Default)]
pub struct InnerCfg {
    pub port: u16,
    pub host: String,
}

#[test]
fn derive_compiles_and_struct_remains_constructible() {
    // If the M1 stub started emitting code that conflicted with the
    // user's own definitions, this test would fail to compile.
    let cfg = WrapperCfg {
        inner: InnerCfg {
            port: 8765,
            host: "127.0.0.1".to_string(),
        },
    };
    assert_eq!(cfg.inner.port, 8765);
    assert_eq!(cfg.inner.host, "127.0.0.1");
}

/// Direct-pyclass pattern (no `inner` field). Should also compile under
/// the M1 stub since the macro doesn't emit anything yet.
#[derive(PyWrapper)]
#[py_wrapper(
    fields(
        name: String => [get, set, repr],
    ),
)]
pub struct DirectStyle {
    pub name: String,
}

#[test]
fn derive_compiles_for_direct_pattern() {
    let d = DirectStyle {
        name: "skill".to_string(),
    };
    assert_eq!(d.name, "skill");
}
