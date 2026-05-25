//! Shared runtime state for HTTP handlers that is independent of axum.
//!
//! `dcc-mcp-http` owns application-layer registries such as resources,
//! prompts, readiness, and method routing. This module owns the lower-level
//! runtime fields used by many handlers: tool registries, session state,
//! in-flight request tracking, job notification, and cache generation.
//!
//! ## Lock acquisition rules
//!
//! Several fields hold internal locks (`RwLock`, `Mutex`, `DashMap`).
//! Handlers MUST follow these rules to avoid deadlocks and lock-order
//! inversions:
//!
//! 1. **Acquisition order, outermost first**, when more than one lock is
//!    held simultaneously:
//!     1. `registry` / `catalog` / `prompts` / `resources` (long-lived
//!        registries — read-heavy, may be held across an entire
//!        `tools/list` build)
//!     2. `sessions` (snapshots, subscribers — per-session granularity)
//!     3. `jobs` / `bridge_registry` (per-request orchestration)
//!     4. `cancelled_requests` / `pending_elicitations` / `in_flight`
//!        (`DashMap` / sharded maps — never wrapped in a higher-level lock)
//!
//!    Do not invert this order. Acquiring `sessions` before `registry`
//!    in one path while another path does the reverse will deadlock under
//!    contention.
//!
//! 2. **Locks are released before notifying subscribers.** SSE / progress
//!    notifications must be dispatched *after* dropping every lock they
//!    do not strictly need — otherwise a slow subscriber stalls the
//!    registry. Pattern: clone the data out of the lock, drop the guard,
//!    then call `JobNotifier` / `SessionManager::publish`.
//!
//! 3. **Hot-path fields use `DashMap`** (`cancelled_requests`,
//!    `pending_elicitations`) so reads / writes never require a global
//!    lock. Do not refactor these into `Arc<RwLock<HashMap<…>>>` —
//!    contention on the inner `RwLock` would serialise every active
//!    SSE connection.
//!
//! 4. **`registry_generation`** is the canonical signal for cache
//!    invalidation (issue #438). Bump it via
//!    [`ServerState::bump_registry_generation`] after any mutation that
//!    changes the visible tool surface; do *not* hold a registry lock
//!    across the bump, because `invalidate_all_tool_list_snapshots`
//!    re-enters `sessions`.

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
    /// Explicit opt-in for standalone/batch hosts where the in-process lane is
    /// already safe for main-affinity calls even without a GUI dispatcher.
    pub standalone_main_thread_execution: bool,
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
    /// Omit ``__skill__*`` stubs from ``tools/list`` when true.
    pub exclude_skill_stubs_from_tools_list: bool,
    /// Omit ``__group__*`` stubs from ``tools/list`` when true.
    pub exclude_group_stubs_from_tools_list: bool,
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
    /// Start building a [`ServerState`] from the three required registries.
    #[must_use]
    pub fn builder(
        registry: Arc<ToolRegistry>,
        dispatcher: Arc<ToolDispatcher>,
        catalog: Arc<SkillCatalog>,
    ) -> ServerStateBuilder {
        let sessions = SessionManager::new();
        ServerStateBuilder {
            state: Self {
                registry,
                dispatcher,
                catalog,
                sessions: sessions.clone(),
                executor: None,
                standalone_main_thread_execution: false,
                server_name: "dcc-mcp-http".to_owned(),
                server_version: env!("CARGO_PKG_VERSION").to_owned(),
                cancelled_requests: Arc::new(DashMap::new()),
                in_flight: InFlightRequests::new(),
                pending_elicitations: Arc::new(DashMap::new()),
                lazy_actions: false,
                bare_tool_names: true,
                exclude_skill_stubs_from_tools_list: false,
                exclude_group_stubs_from_tools_list: false,
                declared_capabilities: Arc::new(Vec::new()),
                jobs: Arc::new(JobManager::new()),
                job_notifier: JobNotifier::new(sessions, true),
                enable_resources: true,
                enable_prompts: true,
                registry_generation: Arc::new(AtomicU64::new(0)),
                enable_tool_cache: true,
                #[cfg(feature = "prometheus")]
                prometheus: None,
            },
        }
    }

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
    #[must_use]
    pub fn current_registry_generation(&self) -> u64 {
        self.registry_generation.load(Ordering::Relaxed)
    }
}

/// Builder for [`ServerState`].
#[derive(Clone)]
pub struct ServerStateBuilder {
    state: ServerState,
}

impl ServerStateBuilder {
    /// Use an existing session manager.
    #[must_use]
    pub fn with_sessions(mut self, sessions: SessionManager) -> Self {
        self.state.sessions = sessions;
        self
    }

    /// Use an optional DCC main-thread executor.
    #[must_use]
    pub fn with_executor(mut self, executor: Option<DccExecutorHandle>) -> Self {
        self.state.executor = executor;
        self
    }

    /// Allow main-affinity tools to run without a host dispatcher when the
    /// embedding adapter has explicitly declared a standalone-safe main lane.
    #[must_use]
    pub fn with_standalone_main_thread_execution(mut self, enabled: bool) -> Self {
        self.state.standalone_main_thread_execution = enabled;
        self
    }

    /// Set the server identity surfaced in `initialize`.
    #[must_use]
    pub fn with_server_identity(
        mut self,
        server_name: impl Into<String>,
        server_version: impl Into<String>,
    ) -> Self {
        self.state.server_name = server_name.into();
        self.state.server_version = server_version.into();
        self
    }

    /// Use an existing cancelled-request map.
    #[must_use]
    pub fn with_cancelled_requests(
        mut self,
        cancelled_requests: Arc<DashMap<String, Instant>>,
    ) -> Self {
        self.state.cancelled_requests = cancelled_requests;
        self
    }

    /// Enable or disable lazy action meta-tools.
    #[must_use]
    pub fn with_lazy_actions(mut self, lazy_actions: bool) -> Self {
        self.state.lazy_actions = lazy_actions;
        self
    }

    /// Enable or disable bare tool names.
    #[must_use]
    pub fn with_bare_tool_names(mut self, bare_tool_names: bool) -> Self {
        self.state.bare_tool_names = bare_tool_names;
        self
    }

    /// Omit unloaded-skill ``__skill__*`` stubs from ``tools/list``.
    #[must_use]
    pub fn with_exclude_skill_stubs_from_tools_list(mut self, exclude: bool) -> Self {
        self.state.exclude_skill_stubs_from_tools_list = exclude;
        self
    }

    /// Omit inactive-group ``__group__*`` stubs from ``tools/list``.
    #[must_use]
    pub fn with_exclude_group_stubs_from_tools_list(mut self, exclude: bool) -> Self {
        self.state.exclude_group_stubs_from_tools_list = exclude;
        self
    }

    /// Set capabilities declared by the hosting adapter.
    #[must_use]
    pub fn with_declared_capabilities(mut self, declared_capabilities: Vec<String>) -> Self {
        self.state.declared_capabilities = Arc::new(declared_capabilities);
        self
    }

    /// Use an existing job manager.
    #[must_use]
    pub fn with_jobs(mut self, jobs: Arc<JobManager>) -> Self {
        self.state.jobs = jobs;
        self
    }

    /// Use an existing job notifier.
    #[must_use]
    pub fn with_job_notifier(mut self, job_notifier: JobNotifier) -> Self {
        self.state.job_notifier = job_notifier;
        self
    }

    /// Enable or disable resource handling.
    #[must_use]
    pub fn with_resources_enabled(mut self, enable_resources: bool) -> Self {
        self.state.enable_resources = enable_resources;
        self
    }

    /// Enable or disable prompt handling.
    #[must_use]
    pub fn with_prompts_enabled(mut self, enable_prompts: bool) -> Self {
        self.state.enable_prompts = enable_prompts;
        self
    }

    /// Enable or disable per-session tool-list caching.
    #[must_use]
    pub fn with_tool_cache_enabled(mut self, enable_tool_cache: bool) -> Self {
        self.state.enable_tool_cache = enable_tool_cache;
        self
    }

    /// Use an existing Prometheus exporter.
    #[cfg(feature = "prometheus")]
    #[must_use]
    pub fn with_prometheus(
        mut self,
        prometheus: Option<dcc_mcp_telemetry::PrometheusExporter>,
    ) -> Self {
        self.state.prometheus = prometheus;
        self
    }

    /// Finish building the state.
    #[must_use]
    pub fn build(self) -> ServerState {
        self.state
    }
}
