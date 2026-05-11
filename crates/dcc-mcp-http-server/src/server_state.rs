//! Shared runtime state for HTTP handlers that is independent of axum.
//!
//! `dcc-mcp-http` owns application-layer registries such as resources,
//! prompts, readiness, and method routing. This module owns the lower-level
//! runtime fields used by many handlers: tool registries, session state,
//! in-flight request tracking, job notification, and cache generation.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
use dcc_mcp_job::job::JobManager;
use dcc_mcp_jsonrpc::ElicitationCreateResult;
use dcc_mcp_skills::SkillCatalog;
use tokio::sync::oneshot;

use crate::executor::DccExecutorHandle;
use crate::inflight::InFlightRequests;
use crate::notifications::JobNotifier;
use crate::session::SessionManager;

/// How long a cancellation record is kept before being garbage-collected.
pub const CANCELLED_REQUEST_TTL: Duration = Duration::from_secs(30);

/// Timeout for MCP roots refresh requests.
pub const ROOTS_REFRESH_TIMEOUT: Duration = Duration::from_secs(2);

/// Timeout for `elicitation/create` round-trips.
pub const ELICITATION_TIMEOUT: Duration = Duration::from_secs(60);

/// Runtime state shared by HTTP handlers but independent of axum routing.
#[derive(Clone)]
pub struct ServerState {
    /// Registered tools/actions exposed by the server.
    pub registry: Arc<ToolRegistry>,
    /// Dispatcher used to execute registered tools/actions.
    pub dispatcher: Arc<ToolDispatcher>,
    /// Skill catalogue used by discovery and progressive loading tools.
    pub catalog: Arc<SkillCatalog>,
    /// MCP session store.
    pub sessions: SessionManager,
    /// Optional DCC main-thread executor.
    pub executor: Option<DccExecutorHandle>,
    /// Server name surfaced in `initialize`.
    pub server_name: String,
    /// Server version surfaced in `initialize`.
    pub server_version: String,
    /// Request ids cancelled by clients and retained for a bounded TTL.
    pub cancelled_requests: Arc<DashMap<String, Instant>>,
    /// In-flight request map for cooperative cancellation and progress.
    pub in_flight: InFlightRequests,
    /// Pending `elicitation/create` requests keyed by request id.
    pub pending_elicitations: Arc<DashMap<String, oneshot::Sender<ElicitationCreateResult>>>,
    /// Enables lazy action meta-tools.
    pub lazy_actions: bool,
    /// Enables bare tool names when unique.
    pub bare_tool_names: bool,
    /// Capabilities declared by the hosting adapter.
    pub declared_capabilities: Arc<Vec<String>>,
    /// Async job manager.
    pub jobs: Arc<JobManager>,
    /// Job / workflow lifecycle notifier.
    pub job_notifier: JobNotifier,
    /// Whether resources are enabled.
    pub enable_resources: bool,
    /// Whether prompts are enabled.
    pub enable_prompts: bool,
    /// Registry generation used for per-session tool-list cache invalidation.
    pub registry_generation: Arc<AtomicU64>,
    /// Whether per-session tool-list cache is enabled.
    pub enable_tool_cache: bool,
    /// Optional Prometheus exporter.
    #[cfg(feature = "prometheus")]
    pub prometheus: Option<dcc_mcp_telemetry::PrometheusExporter>,
}

impl ServerState {
    /// Remove cancellation entries older than [`CANCELLED_REQUEST_TTL`].
    pub fn purge_expired_cancellations(&self) {
        self.cancelled_requests
            .retain(|_, recorded_at| recorded_at.elapsed() < CANCELLED_REQUEST_TTL);
    }

    /// Bump the registry generation counter and invalidate per-session caches.
    pub fn bump_registry_generation(&self) {
        let prev = self.registry_generation.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(prev_generation = prev, "registry generation bumped");
        self.sessions.invalidate_all_tool_list_snapshots();
    }

    /// Read the current registry generation counter.
    pub fn current_registry_generation(&self) -> u64 {
        self.registry_generation.load(Ordering::Relaxed)
    }
}
