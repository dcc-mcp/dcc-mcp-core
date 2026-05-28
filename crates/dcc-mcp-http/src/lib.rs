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
//! use dcc_mcp_actions::ToolRegistry;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let registry = Arc::new(ToolRegistry::new());
//! let config = McpHttpConfig::default().with_port(8765);
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
pub mod handler;
pub mod host_bridge;
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
pub mod payload;
pub mod prompts;
pub mod resource_link;
pub mod resources;
pub(crate) mod rest_providers;
pub mod server;
pub mod session;
pub mod session_events;
pub mod workspace;

#[cfg(feature = "prometheus")]
pub mod metrics;

// Re-exports
pub use bridge_registry::{BridgeContext, BridgeRegistry};
pub use config::{McpHttpConfig, ServerSpawnMode};
#[cfg(feature = "auto-gateway")]
pub use dcc_mcp_gateway::{GatewayConfig, GatewayHandle, GatewayRunner};
pub use dcc_mcp_http_types::debug_session::{
    DebugPathMapping, DebugSessionDescriptor, DebugSessionStatus,
};
pub use dcc_mcp_http_types::session_events::{
    SessionEvent, SessionEventPage, SessionEventReadOptions, SessionEventTruncation,
};
pub use dcc_mcp_skill_rest::{
    AllowLocalhostGate, AuditEvent, AuditOutcome, AuditSink, AuthGate, BearerTokenGate,
    NoopAuditSink, Principal, ReadinessProbe, ReadinessReport, ServiceError, ServiceErrorKind,
    SkillRestConfig, SkillRestService, StaticReadiness, ToolInvoker, ToolSlug, VecAuditSink,
    build_skill_rest_router,
};
pub use dynamic_tools::{DYNAMIC_TOOL_PREFIX, DynamicToolError, SessionDynamicTools, ToolSpec};
pub use error::{HttpError, HttpResult};
pub use executor::{DccExecutorHandle, DeferredExecutor, ExecutorQueueStats};
pub use job::{Job, JobEvent, JobManager, JobProgress, JobStatus, JobSubscriber};
#[cfg(feature = "job-persist-sqlite")]
pub use job_storage::SqliteStorage;
pub use job_storage::{InMemoryStorage, JobFilter, JobStorage, JobStorageError};
pub use notifications::{JobNotifier, WorkflowProgress, WorkflowUpdate};
pub use output::{OutputBuffer, OutputCapture, OutputEntry, OutputStream};
pub use payload::{SseChunkFrame, TruncationEnvelope, chunk_sse_data, format_chunked_sse};
pub use prompts::{
    PromptArgumentSpec, PromptEntry, PromptError, PromptRegistry, PromptResult, PromptSource,
    PromptSpec, PromptsSpec, WorkflowPromptRef, render_template,
};
pub use resources::{
    ProducerContent, ResourceError, ResourceProducer, ResourceRegistry, ResourceResult,
};
pub use server::{McpHttpServer, McpServerHandle};
pub use session::SessionManager;
pub use session_events::{
    DEFAULT_SESSION_EVENT_CAPACITY, DEFAULT_SESSION_EVENT_MAX_MESSAGE_BYTES,
    DEFAULT_SESSION_EVENT_READ_LIMIT, MAX_SESSION_EVENT_READ_LIMIT, SessionEventBuffer,
    SessionEventLevel, SessionEventResourceProducer,
};
pub use workspace::{WorkspaceResolveError, WorkspaceRoots};
