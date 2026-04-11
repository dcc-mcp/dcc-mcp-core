//! Python bindings for DCC adapter data types via PyO3.
//!
//! Exposes `PyDccInfo`, `PyScriptResult`, `PyScriptLanguage`, `PySceneInfo`,
//! `PySceneStatistics`, `PyDccCapabilities`, `PyDccError`, `PyDccErrorCode`,
//! `PyCaptureResult`, `PyObjectTransform`, `PyBoundingBox`, `PySceneObject`,
//! `PyFrameRange`, `PyRenderOutput`, and `PySceneNode` as Python classes.

mod data;
mod enums;
mod scene_node;

#[cfg(feature = "python-bindings")]
pub use data::{
    PyCaptureResult, PyDccCapabilities, PyDccError, PyDccInfo, PyFrameRange, PyObjectTransform,
    PyRenderOutput, PySceneInfo, PySceneObject, PySceneStatistics, PyScriptResult,
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
