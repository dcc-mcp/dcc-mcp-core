//! PyO3 bindings for the MCP HTTP server.

pub mod bridge;
pub mod config;
pub mod output_dynamic;
pub mod server;
pub mod skill_server;
pub mod workspace;

pub use bridge::{
    PyBridgeContext, PyBridgeRegistry, py_create_skill_server, py_get_bridge_context,
    py_register_bridge, register_bridge_internal, register_classes,
};
pub use config::PyMcpHttpConfig;
pub use output_dynamic::{PyOutputCapture, PyToolSpec};
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
