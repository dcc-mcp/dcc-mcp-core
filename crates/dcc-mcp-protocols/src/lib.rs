//! dcc-mcp-protocols: MCP protocol type definitions and DCC adapter traits.

pub mod adapters;
pub mod adapters_python;

#[cfg(test)]
pub mod mock;
mod types;

pub use adapters::{
    CaptureResult, DccAdapter, DccCapabilities, DccConnection, DccError, DccErrorCode, DccInfo,
    DccResult, DccSceneInfo, DccScriptEngine, DccSnapshot, SceneInfo, SceneStatistics,
    ScriptLanguage, ScriptResult,
};
pub use types::{
    PromptArgument, PromptDefinition, ResourceAnnotations, ResourceDefinition,
    ResourceTemplateDefinition, ToolAnnotations, ToolDefinition,
};

#[cfg(feature = "python-bindings")]
pub use adapters_python::{
    PyCaptureResult, PyDccCapabilities, PyDccError, PyDccErrorCode, PyDccInfo, PySceneInfo,
    PySceneStatistics, PyScriptLanguage, PyScriptResult,
};
