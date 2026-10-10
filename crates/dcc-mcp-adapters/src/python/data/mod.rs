//! Python-facing data structs for DCC adapter types.
//!
//! This module re-exports all types from the two sub-modules:
//! - [`data_dcc`]: DCC core types (`PyDccInfo`, `PyScriptResult`, `PySceneStatistics`,
//!   `PySceneInfo`, `PyDccCapabilities`, `PyDccError`, `PyCaptureResult`)
//! - [`data_scene`]: Scene geometry types (`PyObjectTransform`, `PyBoundingBox`,
//!   `PySceneObject`, `PyFrameRange`, `PyRenderOutput`)

pub mod data_dcc;
pub mod data_scene;

#[cfg(feature = "python-bindings")]
pub use data_dcc::{
    PyCaptureResult, PyDccCapabilities, PyDccError, PyDccInfo, PySceneInfo, PySceneStatistics,
    PyScriptResult,
};

#[cfg(feature = "python-bindings")]
pub use data_scene::{
    PyBoundingBox, PyFrameRange, PyObjectTransform, PyRenderOutput, PySceneObject,
};
