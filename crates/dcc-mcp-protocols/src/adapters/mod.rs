//! DCC adapter traits — unified interface for all DCC application integrations.
//!
//! See [`types`] for data structures and [`traits`] for trait definitions.

pub mod traits;
pub mod types;

pub use traits::{
    DccAdapter, DccConnection, DccHierarchy, DccRenderCapture, DccSceneInfo, DccSceneManager,
    DccScriptEngine, DccSnapshot, DccTransform,
};
pub use types::{
    BoundingBox, CaptureResult, DccCapabilities, DccError, DccErrorCode, DccInfo, DccResult,
    FrameRange, ObjectTransform, RenderOutput, SceneInfo, SceneNode, SceneObject, SceneStatistics,
    ScriptLanguage, ScriptResult,
};

#[cfg(test)]
mod tests;
