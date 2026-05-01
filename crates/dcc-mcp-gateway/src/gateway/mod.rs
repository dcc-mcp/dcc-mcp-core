//! Gateway module — first-wins port competition, instance registry, and HTTP routing.
//!
//! When `McpHttpConfig::gateway_port > 0`, `McpHttpServer::start()` will attempt to
//! become the gateway by binding the well-known gateway port. The first process to
//! bind wins; subsequent processes register themselves as plain DCC instances.
//!
//! # Quick start (Rust)
//!
//! The gateway is started transparently by the embedded MCP HTTP server:
//!
//! ```rust,ignore
//! // This example runs in the context of the `dcc-mcp-http` crate,
//! // which owns `McpHttpConfig` / `McpHttpServer`. `dcc-mcp-gateway`
//! // itself only exposes the low-level `GatewayConfig` / `GatewayRunner`
//! // primitives — see `dcc_mcp_gateway::gateway::{GatewayConfig,
//! // GatewayRunner}` for the direct API.
//! use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
//! use dcc_mcp_actions::ActionRegistry;
//! use std::sync::Arc;
//!
//! # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let registry = Arc::new(ActionRegistry::new());
//! let config = McpHttpConfig::new(0)
//!     .with_name("maya")
//!     .with_dcc_type("maya")
//!     .with_gateway(9765);
//!
//! let handle = McpHttpServer::new(registry, config).start().await?;
//! println!("is_gateway = {}", handle.is_gateway);
//! # Ok(())
//! # }
//! ```

pub mod aggregator;
pub mod backend_client;
pub mod capability;
pub mod capability_service;
pub mod handlers;
pub mod namespace;
pub mod proxy;
pub mod router;
pub mod sse_subscriber;
pub mod state;
pub mod tools;

#[cfg(feature = "prometheus")]
pub mod metrics;

pub use router::build_gateway_router;
pub use state::{GatewayState, entry_to_json};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Snapshot of live DCC instance metadata returned by [`MetadataProvider`].
///
/// Supports both single-document DCCs (Maya, Blender — only `scene` changes)
/// and multi-document DCCs (Photoshop, After Effects — also `documents` list).
#[derive(Debug, Clone, Default)]
pub struct LiveSnapshot {
    /// Currently active/focused scene or document path.
    pub scene: Option<String>,
    /// DCC application version string.
    pub version: Option<String>,
    /// All open documents.  Non-empty → `update_documents` is called;
    /// empty → only `scene`/`version` are updated via `update_metadata`.
    pub documents: Vec<String>,
    /// Human-readable instance label (e.g. `"PS-Marketing"`).
    pub display_name: Option<String>,
}

/// Closure type for supplying live instance metadata to the heartbeat task.
///
/// Called on every heartbeat tick; the returned [`LiveSnapshot`] is written
/// to `FileRegistry` via `update_documents` (when `documents` is non-empty)
/// or `update_metadata` (single-document DCCs).
pub type MetadataProvider = Arc<dyn Fn() -> LiveSnapshot + Send + Sync>;

use tokio::sync::{RwLock, broadcast, watch};
use tokio::task::AbortHandle;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceKey};

mod bind;
mod config;
mod handle;
mod runner;
mod sentinel;
mod tasks;
mod version;

pub(crate) use bind::try_bind_port_opt;
pub use config::{GatewayConfig, GatewayToolExposure, ParseGatewayToolExposureError};
pub(crate) use handle::ElectionOutcome;
pub use handle::GatewayHandle;
pub use runner::GatewayRunner;
pub(crate) use sentinel::{has_newer_sentinel, is_own_instance};
pub(crate) use tasks::start_gateway_tasks;
pub(crate) use version::{ElectionInfo, is_newer_election, is_newer_version};

#[cfg(test)]
pub(crate) use tasks::self_probe_listener;
#[cfg(test)]
pub(crate) use version::parse_semver;

#[cfg(test)]
mod tests;
