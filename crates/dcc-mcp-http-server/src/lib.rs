//! Runtime support layer for the DCC MCP HTTP server (issue #852).
//!
//! This crate contains reusable server-support components that do not depend
//! on `axum`, `tower`, Python bindings, or the top-level `dcc-mcp-http` crate:
//!
//! - main-thread execution bridge,
//! - host dispatcher adapter,
//! - session-scoped dynamic tools,
//! - MCP session state,
//! - in-flight request cancellation / progress routing,
//! - job and workflow notifications,
//! - workspace root resolution,
//! - low-coupling handler helpers.
//!
//! The public `dcc-mcp-http` crate re-exports this surface from its historical
//! module paths for source compatibility.

#![forbid(unsafe_code)]
#![allow(clippy::must_use_candidate)]

pub mod dynamic_tools;
pub mod executor;
pub mod handlers;
pub mod host_bridge;
pub mod inflight;
pub mod notifications;
pub mod server_state;
pub mod session;
pub mod workspace;

pub use dynamic_tools::{
    DYNAMIC_TOOL_PREFIX, DynamicToolEntry, DynamicToolError, SessionDynamicTools, ToolSpec,
    build_deregister_tool_descriptor, build_list_dynamic_tools_descriptor,
    build_register_tool_descriptor, handle_deregister_tool, handle_list_dynamic_tools,
    handle_register_tool,
};
pub use executor::{DccExecutorHandle, DeferredExecutor, ExecutorQueueStats};
pub use handlers::{build_core_tools, build_core_tools_inner};
pub use host_bridge::{
    DEFAULT_BRIDGE_QUEUE_DEPTH, dispatcher_to_executor_handle,
    dispatcher_to_executor_handle_with_capacity,
};
pub use inflight::{
    CANCEL_GRACE_PERIOD, CancelToken, InFlightEntry, InFlightRequests, ProgressReporter,
};
pub use notifications::{JobNotifier, WorkflowProgress, WorkflowUpdate};
pub use server_state::{
    CANCELLED_REQUEST_TTL, ELICITATION_TIMEOUT, ROOTS_REFRESH_TIMEOUT, ServerState,
};
pub use session::{
    McpSession, SessionLogLevel, SessionLogMessage, SessionManager, ToolListSnapshot,
};
pub use workspace::{WorkspaceResolveError, WorkspaceRoots};
