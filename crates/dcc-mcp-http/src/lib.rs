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
pub mod dynamic_tools;
pub mod error;
pub mod executor;
/// Re-export of [`dcc_mcp_gateway`] under the historical
/// `dcc_mcp_http::gateway` path.
///
/// The multi-DCC gateway lives in its own crate now (it is the
/// largest module by far — ~11k LoC across 53 files). Touching
/// gateway internals no longer triggers a full rebuild of the
/// embedded MCP HTTP server, and downstream binaries that don't
/// participate in gateway election can avoid pulling it in.
pub use dcc_mcp_gateway as gateway;
pub mod handler;
pub(crate) mod handlers;
pub mod inflight;
/// Re-export of [`dcc_mcp_job::job`] under the historical
/// `dcc_mcp_http::job` path. The job tracker now lives in its own
/// crate so embedders can use it without pulling in axum.
pub use dcc_mcp_job::job;
/// Re-export of [`dcc_mcp_job::job_storage`] under the historical
/// `dcc_mcp_http::job_storage` path. The optional SQLite backend is
/// gated by the `job-persist-sqlite` feature, which is forwarded
/// through this crate.
pub use dcc_mcp_job::job_storage;
pub mod notifications;
pub mod output;
/// Re-export of [`dcc_mcp_jsonrpc`] under the historical
/// `dcc_mcp_http::protocol` path.
///
/// The MCP wire types live in their own crate now; this alias keeps
/// every existing `use crate::protocol::*` and downstream
/// `dcc_mcp_http::protocol::*` import working without a code change.
pub use dcc_mcp_jsonrpc as protocol;
pub mod prompts;
pub mod resource_link;
pub mod resources;
pub mod server;
pub mod session;
/// Re-export of [`dcc_mcp_skill_rest`] under the historical
/// `dcc_mcp_http::skill_rest` path.
///
/// The per-DCC RESTful skill surface (#658, #660) lives in its own
/// crate now; this alias keeps every existing import working without
/// a code change.
pub use dcc_mcp_skill_rest as skill_rest;
pub mod workspace;

#[cfg(feature = "prometheus")]
pub mod metrics;

#[cfg(feature = "python-bindings")]
pub mod python;

// Re-exports
pub use bridge_registry::{BridgeContext, BridgeRegistry};
pub use config::{McpHttpConfig, ServerSpawnMode};
pub use dynamic_tools::{DYNAMIC_TOOL_PREFIX, DynamicToolError, SessionDynamicTools, ToolSpec};
pub use error::{HttpError, HttpResult};
pub use executor::{DccExecutorHandle, DeferredExecutor};
pub use gateway::{GatewayConfig, GatewayHandle, GatewayRunner};
pub use job::{Job, JobEvent, JobManager, JobProgress, JobStatus, JobSubscriber};
#[cfg(feature = "job-persist-sqlite")]
pub use job_storage::SqliteStorage;
pub use job_storage::{InMemoryStorage, JobFilter, JobStorage, JobStorageError};
pub use notifications::{JobNotifier, WorkflowProgress, WorkflowUpdate};
pub use output::{OutputBuffer, OutputCapture, OutputEntry, OutputStream};
pub use prompts::{
    PromptArgumentSpec, PromptEntry, PromptError, PromptRegistry, PromptResult, PromptSource,
    PromptSpec, PromptsSpec, WorkflowPromptRef, render_template,
};
pub use resources::{
    ProducerContent, ResourceError, ResourceProducer, ResourceRegistry, ResourceResult,
};
pub use server::{McpHttpServer, McpServerHandle};
pub use session::SessionManager;
pub use skill_rest::{
    AllowLocalhostGate, AuditEvent, AuditOutcome, AuditSink, AuthGate, BearerTokenGate,
    NoopAuditSink, Principal, ReadinessProbe, ReadinessReport, ServiceError, ServiceErrorKind,
    SkillRestConfig, SkillRestService, StaticReadiness, ToolInvoker, ToolSlug, VecAuditSink,
    build_skill_rest_router,
};
pub use workspace::{WorkspaceResolveError, WorkspaceRoots};

#[cfg(feature = "python-bindings")]
pub use python::{PyMcpHttpConfig, PyMcpHttpServer, PyServerHandle, PyWorkspaceRoots};

#[cfg(test)]
mod tests;
