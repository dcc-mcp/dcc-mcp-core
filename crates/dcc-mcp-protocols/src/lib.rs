//! dcc-mcp-protocols: MCP protocol type definitions and DCC adapter traits.

pub mod adapters;
pub mod adapters_python;
pub mod bridge;
pub mod error_envelope;

#[cfg(test)]
pub mod mock;
mod types;

pub use adapters::{
    BoundingBox, CaptureResult, DccAdapter, DccCapabilities, DccConnection, DccError, DccErrorCode,
    DccHierarchy, DccInfo, DccRenderCapture, DccResult, DccSceneInfo, DccSceneManager,
    DccScriptEngine, DccSnapshot, DccTransform, FrameRange, ObjectTransform, RenderOutput,
    SceneInfo, SceneNode, SceneObject, SceneStatistics, ScriptLanguage, ScriptResult,
};
pub use bridge::error_codes as bridge_error_codes;
pub use bridge::{
    BridgeDisconnect, BridgeEvent, BridgeHello, BridgeHelloAck, BridgeMessage, BridgeParseError,
    BridgeRequest, BridgeResponse, RequestId, RpcError,
};
pub use error_envelope::DccMcpError;
pub use types::{
    PromptArgument, PromptDefinition, ResourceAnnotations, ResourceDefinition,
    ResourceTemplateDefinition, ToolAnnotations, ToolDefinition,
};

#[cfg(feature = "python-bindings")]
pub use adapters_python::{
    PyBoundingBox, PyCaptureResult, PyDccCapabilities, PyDccError, PyDccErrorCode, PyDccInfo,
    PyFrameRange, PyObjectTransform, PyRenderOutput, PySceneInfo, PySceneNode, PySceneObject,
    PySceneStatistics, PyScriptLanguage, PyScriptResult,
};
