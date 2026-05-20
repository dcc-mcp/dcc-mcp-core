//! PyO3 bindings for the DCC MCP HTTP server (issue #852).
//!
//! This crate is the Python-binding boundary for `dcc-mcp-http`. The Rust
//! HTTP server/application code stays in `dcc-mcp-http`; Python classes and
//! registration live here so downstream `_core` registration no longer depends
//! on `dcc-mcp-http::python`.
//!
//! Dependency direction:
//!
//! ```text
//! dcc-mcp-core (_core extension) → dcc-mcp-http-py → dcc-mcp-http
//!                                        │
//!                                        └── dcc-mcp-http-types (config/value surface)
//! ```

#![forbid(unsafe_code)]

#[cfg(feature = "python-bindings")]
pub mod bridge;
#[cfg(feature = "python-bindings")]
pub mod config;
#[cfg(feature = "python-bindings")]
pub mod output_dynamic;
#[cfg(feature = "python-bindings")]
pub mod prompts_handle;
#[cfg(feature = "python-bindings")]
pub mod readiness;
#[cfg(feature = "python-bindings")]
pub mod resources_handle;
#[cfg(feature = "python-bindings")]
pub mod server;
#[cfg(feature = "python-bindings")]
pub mod session_events;
#[cfg(feature = "python-bindings")]
pub mod skill_server;
#[cfg(feature = "python-bindings")]
pub mod workspace;

#[cfg(feature = "python-bindings")]
pub use bridge::{
    PyBridgeContext, PyBridgeRegistry, py_create_skill_server, py_get_bridge_context,
    py_register_bridge, register_bridge_internal, register_classes,
};
#[cfg(feature = "python-bindings")]
pub use config::PyMcpHttpConfig;
#[cfg(feature = "python-bindings")]
pub use output_dynamic::{PyOutputCapture, PyToolSpec};
#[cfg(feature = "python-bindings")]
pub use prompts_handle::PyPromptHandle;
#[cfg(feature = "python-bindings")]
pub use readiness::PyReadinessProbe;
#[cfg(feature = "python-bindings")]
pub use resources_handle::PyResourceHandle;
#[cfg(feature = "python-bindings")]
pub use server::PyServerHandle;
#[cfg(feature = "python-bindings")]
pub use session_events::PySessionEventBuffer;
#[cfg(feature = "python-bindings")]
pub use skill_server::PyMcpHttpServer;
#[cfg(feature = "python-bindings")]
pub use workspace::PyWorkspaceRoots;

#[cfg(feature = "python-bindings")]
use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
#[cfg(feature = "python-bindings")]
use dcc_mcp_http::server::{LiveMetaInner, McpHttpServer, McpServerHandle};
#[cfg(feature = "python-bindings")]
use dcc_mcp_http_types::config::{McpHttpConfig, ServerSpawnMode};
#[cfg(feature = "python-bindings")]
use dcc_mcp_skills::SkillCatalog;
#[cfg(feature = "python-bindings")]
use parking_lot::RwLock;
#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use std::sync::Arc;
#[cfg(feature = "python-bindings")]
use tokio::runtime::Runtime;

/// Build the Tokio runtime used by every PyO3 entry-point in this crate.
///
/// Centralised so future tuning (worker thread count, stack size, lifetime
/// telemetry, …) only has to change here instead of every `Runtime::new()`
/// callsite spread across `bridge.rs` and `skill_server.rs`.
#[cfg(feature = "python-bindings")]
pub(crate) fn build_python_runtime() -> PyResult<Runtime> {
    Runtime::new().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}
