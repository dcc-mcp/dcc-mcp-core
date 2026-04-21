//! MCP Streamable HTTP server for DCC applications.
//!
//! Implements the MCP 2025-03-26 Streamable HTTP transport specification.
//! Designed for embedding inside DCC software (Maya, Blender, Houdini, etc.)
//! with explicit DCC-thread-safety guarantees.
//!
//! # Architecture
//!
//! ```text
//! DCC Main Thread                    Tokio Worker Thread(s)
//! ─────────────────                  ───────────────────────
//! register skills/actions            axum HTTP server
//! McpHttpServer::start()  ──────►   POST /mcp  → dispatch
//!                                    GET  /mcp  → SSE stream
//!   DeferredExecutor ◄───────────── task queue (mpsc)
//!   (executes on DCC main thread)
//!       │
//!       └─► DCC API calls (thread-safe)
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use dcc_mcp_http::{McpHttpServer, McpHttpConfig};
//! use dcc_mcp_actions::ActionRegistry;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let registry = Arc::new(ActionRegistry::new());
//! let config = McpHttpConfig::new(8765);
//!
//! let server = McpHttpServer::new(registry, config);
//! let handle = server.start().await?;
//!
//! // Later: graceful shutdown
//! handle.shutdown().await;
//! # Ok(())
//! # }
//! ```

pub mod bridge_registry;
pub mod config;
pub mod error;
pub mod executor;
pub mod gateway;
pub mod handler;
pub mod inflight;
pub mod job;
pub mod protocol;
pub mod resource_link;
pub mod resources;
pub mod server;
pub mod session;

#[cfg(feature = "python-bindings")]
pub mod python;

// Re-exports
pub use bridge_registry::{BridgeContext, BridgeRegistry};
pub use config::{McpHttpConfig, ServerSpawnMode};
pub use error::{HttpError, HttpResult};
pub use executor::{DccExecutorHandle, DeferredExecutor};
pub use gateway::{GatewayConfig, GatewayHandle, GatewayRunner};
pub use job::{Job, JobManager, JobProgress, JobStatus};
pub use resources::{
    ProducerContent, ResourceError, ResourceProducer, ResourceRegistry, ResourceResult,
};
pub use server::{McpHttpServer, McpServerHandle};
pub use session::SessionManager;

#[cfg(feature = "python-bindings")]
pub use python::{PyMcpHttpConfig, PyMcpHttpServer, PyServerHandle};

#[cfg(test)]
mod tests;
