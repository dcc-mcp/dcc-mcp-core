//! PyO3 bindings for `dcc-mcp-adapters`.
//!
//! Per workspace convention (#501), every `#[pymethods]` / `#[pyfunction]`
//! block in this crate lives below `src/python/`.
//!
//! Exposes `PyDccInfo`, `PyScriptResult`, `PyScriptLanguage`, `PySceneInfo`,
//! `PySceneStatistics`, `PyDccCapabilities`, `PyDccError`, `PyDccErrorCode`,
//! `PyCaptureResult`, `PyObjectTransform`, `PyBoundingBox`, `PySceneObject`,
//! `PyFrameRange`, `PyRenderOutput`, and `PySceneNode` as Python classes.

pub mod data;
pub mod enums;
pub mod scene_node;

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
