//! PyO3 bindings for `dcc-mcp-protocols`.
//!
//! Per workspace convention (#501), every `#[pymethods]` /
//! `#[pyfunction]` block in this crate lives below `src/python/`.
//! The previous `adapters_python/` directory has been renamed to
//! `python/adapters/` for naming consistency.
//!
//! Exposes `PyDccInfo`, `PyScriptResult`, `PyScriptLanguage`, `PySceneInfo`,
//! `PySceneStatistics`, `PyDccCapabilities`, `PyDccError`, `PyDccErrorCode`,
//! `PyCaptureResult`, `PyObjectTransform`, `PyBoundingBox`, `PySceneObject`,
//! `PyFrameRange`, `PyRenderOutput`, and `PySceneNode` as Python classes.

mod data;
mod enums;
mod scene_node;

#[cfg(feature = "python-bindings")]
mod types_prompts;
#[cfg(feature = "python-bindings")]
mod types_resources;
#[cfg(feature = "python-bindings")]
mod types_tools;

#[cfg(feature = "python-bindings")]
pub use data::{
    PyBoundingBox, PyCaptureResult, PyDccCapabilities, PyDccError, PyDccInfo, PyFrameRange,
    PyObjectTransform, PyRenderOutput, PySceneInfo, PySceneObject, PySceneStatistics,
    PyScriptResult,
};
#[cfg(feature = "python-bindings")]
pub use enums::{PyDccErrorCode, PyScriptLanguage};
#[cfg(feature = "python-bindings")]
pub use scene_node::PySceneNode;

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_compiles() {
        // Compilation test — the Python bindings are gated behind the feature flag,
        // so we only verify the module compiles in default (non-binding) mode.
        let _ = 1 + 1;
    }
}
