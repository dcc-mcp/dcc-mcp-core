//! PyO3 bindings for `dcc-mcp-protocols`.
//!
//! Per workspace convention (#501), every `#[pymethods]` /
//! `#[pyfunction]` block in this crate lives below `src/python/`.
//!
//! These modules attach `#[pymethods]` directly to the protocol value types,
//! so they export no separate `Py*` classes. The DCC adapter PyO3 classes
//! (`PyDccInfo`, `PySceneInfo`, `PyCaptureResult`, …) moved to
//! `dcc-mcp-adapters` in ADR-037 Cut 3 and are re-exported from the crate
//! root, so `dcc_mcp_protocols::PyDccInfo` still resolves.

#[cfg(feature = "python-bindings")]
mod types_prompts;
#[cfg(feature = "python-bindings")]
mod types_resources;
#[cfg(feature = "python-bindings")]
mod types_tools;

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_compiles() {
        // Compilation test — the Python bindings are gated behind the feature flag,
        // so we only verify the module compiles in default (non-binding) mode.
        let _ = 1 + 1;
    }
}
