//! PyO3 bindings for the MCP HTTP server.

pub mod bridge;
pub mod config;
pub mod output_dynamic;
pub mod readiness;
pub mod server;
pub mod skill_server;
pub mod workspace;

pub use bridge::{
    PyBridgeContext, PyBridgeRegistry, py_create_skill_server, py_get_bridge_context,
    py_register_bridge, register_bridge_internal, register_classes,
};
pub use config::PyMcpHttpConfig;
pub use output_dynamic::{PyOutputCapture, PyToolSpec};
pub use readiness::PyReadinessProbe;
pub use server::PyServerHandle;
pub use skill_server::PyMcpHttpServer;
pub use workspace::PyWorkspaceRoots;

use parking_lot::RwLock;
use pyo3::prelude::*;
use std::sync::Arc;
use tokio::runtime::Runtime;

use crate::{
    config::{McpHttpConfig, ServerSpawnMode},
    server::{LiveMetaInner, McpHttpServer, McpServerHandle},
};
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_skills::SkillCatalog;

/// Build the Tokio runtime used by every PyO3 entry-point in this crate.
///
/// Centralised so future tuning (worker thread count, stack size, lifetime
/// telemetry, …) only has to change here instead of every `Runtime::new()`
/// callsite spread across `bridge.rs` and `skill_server.rs`.
pub(crate) fn build_python_runtime() -> PyResult<Runtime> {
    Runtime::new().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}
