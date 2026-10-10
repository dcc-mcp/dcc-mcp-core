//! dcc-mcp-adapters: DCC adapter traits, data models, and their PyO3 projections.
//!
//! ADR-037 Cut 3. These types describe a *DCC application integration*
//! (scene contents, script execution, capture results, hierarchy), which is a
//! different concern from the MCP protocol types in `dcc-mcp-protocols`.
//! Keeping them in the protocol crate raised its dependency ceiling for every
//! consumer that only needed `ToolDefinition` or `ToolAnnotations`.
//!
//! `dcc-mcp-protocols` re-exports the whole surface so existing
//! `dcc_mcp_protocols::*` paths keep compiling.

pub mod adapters;

#[cfg(test)]
pub mod mock;

#[cfg(feature = "python-bindings")]
pub mod python;

pub use adapters::{
    BoundingBox, BridgeKind, CaptureResult, DccAdapter, DccCapabilities, DccConnection, DccError,
    DccErrorCode, DccFileIO, DccHierarchy, DccInfo, DccRenderCapture, DccResult, DccSceneInfo,
    DccSceneManager, DccSceneQuery, DccScriptEngine, DccSelection, DccSnapshot, DccTransform,
    FrameRange, ObjectTransform, RenderOutput, SceneInfo, SceneNode, SceneObject, SceneStatistics,
    ScriptLanguage, ScriptResult,
};

#[cfg(feature = "python-bindings")]
pub use python::{
    PyBoundingBox, PyCaptureResult, PyDccCapabilities, PyDccError, PyDccErrorCode, PyDccInfo,
    PyFrameRange, PyObjectTransform, PyRenderOutput, PySceneInfo, PySceneNode, PySceneObject,
    PySceneStatistics, PyScriptLanguage, PyScriptResult,
};
