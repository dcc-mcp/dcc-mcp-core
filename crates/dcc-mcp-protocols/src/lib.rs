//! dcc-mcp-protocols: MCP protocol type definitions.
//!
//! This crate owns the *protocol* types — MCP tool, resource, and prompt
//! definitions plus the JSON-RPC 2.0 bridge vocabulary. The DCC application
//! integration surface (adapter traits, scene/scription data models, and
//! their PyO3 projections) moved to `dcc-mcp-adapters` in ADR-037 Cut 3 and
//! is re-exported here so existing `dcc_mcp_protocols::*` paths keep
//! compiling.

pub mod bridge;
pub mod error_envelope;
pub mod python;

mod types;

pub use bridge::error_codes as bridge_error_codes;
pub use bridge::{
    BridgeDisconnect, BridgeEvent, BridgeHello, BridgeHelloAck, BridgeMessage, BridgeParseError,
    BridgeRequest, BridgeResponse, RequestId, RpcError,
};
#[allow(deprecated)]
pub use error_envelope::{DccMcpError, ToolCallErrorEnvelope};
pub use types::{
    DEFAULT_MIME_TYPE, PromptArgument, PromptDefinition, ResourceAnnotations, ResourceDefinition,
    ResourceTemplateDefinition, ToolAnnotations, ToolDefinition,
};

// Re-export the DCC adapter surface from its new home (ADR-037 Cut 3).
pub use dcc_mcp_adapters::{
    BoundingBox, BridgeKind, CaptureResult, DccAdapter, DccCapabilities, DccConnection, DccError,
    DccErrorCode, DccFileIO, DccHierarchy, DccInfo, DccRenderCapture, DccResult, DccSceneInfo,
    DccSceneManager, DccSceneQuery, DccScriptEngine, DccSelection, DccSnapshot, DccTransform,
    FrameRange, ObjectTransform, RenderOutput, SceneInfo, SceneNode, SceneObject, SceneStatistics,
    ScriptLanguage, ScriptResult,
};

#[cfg(feature = "python-bindings")]
pub use dcc_mcp_adapters::{
    PyBoundingBox, PyCaptureResult, PyDccCapabilities, PyDccError, PyDccErrorCode, PyDccInfo,
    PyFrameRange, PyObjectTransform, PyRenderOutput, PySceneInfo, PySceneNode, PySceneObject,
    PySceneStatistics, PyScriptLanguage, PyScriptResult,
};
