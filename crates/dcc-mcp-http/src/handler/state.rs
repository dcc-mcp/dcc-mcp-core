//! Shared [`AppState`] owned by every axum handler in [`crate::handler`].
//!
//! Carries every long-lived registry (action / skill / resource /
//! prompt), the session manager, executor handle, and in-process
//! bookkeeping (cancellations, in-flight requests, pending
//! elicitations). Cloning `AppState` is cheap — every field is an
//! `Arc`-backed handle.
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
//!        (DashMap — sharded internally; never wrapped in a higher-level
//!        lock)
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
//!    invalidation (issue #438). Bump it via [`AppState::bump_registry_generation`]
//!    after any mutation that changes the visible tool surface; do
//!    *not* hold a registry lock across the bump, because
//!    `invalidate_all_tool_list_snapshots` re-enters `sessions`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::oneshot;

use crate::{
    bridge_registry::BridgeRegistry, executor::DccExecutorHandle, inflight::InFlightRequests,
    prompts::PromptRegistry, protocol::ElicitationCreateResult, resources::ResourceRegistry,
    session::SessionManager,
};
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_skills::SkillCatalog;

/// How long a cancellation record is kept before being garbage-collected.
///
/// If a client sends `notifications/cancelled` for a request that has already
/// completed (common race condition), the entry would never be consumed by the
/// check in `handle_tools_call`.  This TTL bounds memory growth from such entries.
pub(crate) const CANCELLED_REQUEST_TTL: Duration = Duration::from_secs(30);
pub(crate) const ROOTS_REFRESH_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const ELICITATION_TIMEOUT: Duration = Duration::from_secs(60);

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<ActionRegistry>,
    pub dispatcher: Arc<ActionDispatcher>,
    pub catalog: Arc<SkillCatalog>,
    pub sessions: SessionManager,
    pub executor: Option<DccExecutorHandle>,
    pub bridge_registry: BridgeRegistry,
    pub server_name: String,
    pub server_version: String,
    /// Tracks request IDs that have been cancelled by the client via
    /// `notifications/cancelled`.
    ///
    /// Value is the `Instant` when the cancellation was recorded, used to
    /// garbage-collect entries that are never consumed (e.g. because the tool
    /// call already completed before the cancellation arrived).  A background
    /// task in `McpHttpServer::start()` runs `purge_expired_cancellations()`
    /// every 60 seconds to keep this map bounded.
    pub cancelled_requests: Arc<DashMap<String, Instant>>,
    pub in_flight: InFlightRequests,
    /// Pending `elicitation/create` requests keyed by the elicitation request id.
    pub pending_elicitations: Arc<DashMap<String, oneshot::Sender<ElicitationCreateResult>>>,
    /// When `true`, `tools/list` surfaces the three lazy-action meta-tools
    /// (`list_actions`, `describe_action`, `call_action`) and the dispatcher
    /// accepts them. See [`crate::McpHttpConfig::lazy_actions`] (#254).
    pub lazy_actions: bool,
    /// When `true` (default), `tools/list` emits bare action names whenever
    /// they are unique within the instance. See
    /// [`crate::McpHttpConfig::bare_tool_names`] (#307).
    pub bare_tool_names: bool,
    /// DCC capabilities advertised by the hosting adapter (issue #354).
    ///
    /// Per-tool `required_capabilities` are checked against this set at
    /// `tools/call` time. Tools with missing capabilities surface
    /// `_meta.dcc.missing_capabilities` in `tools/list` and fail the call
    /// with JSON-RPC error `-32001 capability_missing`.
    pub declared_capabilities: Arc<Vec<String>>,
    /// Registry of async jobs tracked by this server instance (#316).
    ///
    /// Actual dispatch-side wiring lands in #318; #316 only establishes the
    /// field so downstream changes can attach to it without touching
    /// `AppState` again.
    pub jobs: Arc<crate::job::JobManager>,
    /// Job / workflow lifecycle notifier (#326).
    ///
    /// Bridges `JobManager` transitions onto SSE. Also exposes
    /// [`JobNotifier::emit_workflow_update`](crate::notifications::JobNotifier::emit_workflow_update)
    /// for the #348 workflow executor to call when workflow-level
    /// transitions occur.
    pub job_notifier: crate::notifications::JobNotifier,
    /// MCP Resources primitive registry (issue #350).
    ///
    /// Populated regardless of `enable_resources` so producers can be
    /// added before the server starts; the capability is only advertised
    /// (and the JSON-RPC methods dispatched) when the flag is set.
    pub resources: ResourceRegistry,
    /// Whether the `resources/*` methods are dispatched and the
    /// `resources` capability is advertised in `initialize`.
    pub enable_resources: bool,
    /// MCP Prompts primitive registry (issues #351, #355).
    ///
    /// Always populated but only queried when `enable_prompts` is set.
    pub prompts: PromptRegistry,
    /// Whether the `prompts/*` methods are dispatched and the
    /// `prompts` capability is advertised in `initialize`.
    pub enable_prompts: bool,
    /// Monotonically increasing generation counter for the action registry
    /// (issue #438). Incremented whenever the registry changes (skill
    /// load/unload, group activation/deactivation). The per-session
    /// [`ToolListSnapshot`](crate::session::ToolListSnapshot) records the
    /// generation at which it was built; a mismatch means the cache is
    /// stale and must be rebuilt.
    pub registry_generation: Arc<AtomicU64>,
    /// Whether the connection-scoped tool cache is enabled (issue #438).
    /// When `true`, `tools/list` stores a per-session snapshot and
    /// returns it directly on subsequent calls if the registry generation
    /// has not changed. Default: `true`.
    pub enable_tool_cache: bool,
    /// Prometheus exporter for tool-call observability (issue #331).
    ///
    /// Present only when the `prometheus` Cargo feature is enabled
    /// **and** [`McpHttpConfig::enable_prometheus`](crate::config::McpHttpConfig::enable_prometheus)
    /// is `true`. When `None`, every recording site is a cheap
    /// `Option::is_some` check so the overhead is negligible for
    /// servers that do not opt in.
    #[cfg(feature = "prometheus")]
    pub prometheus: Option<dcc_mcp_telemetry::PrometheusExporter>,
    /// Pluggable JSON-RPC method router (issue #492).
    ///
    /// Built-in MCP methods (`initialize`, `tools/*`, `resources/*`,
    /// `prompts/*`, `elicitation/create`, `ping`,
    /// `notifications/initialized`, `logging/setLevel`) are pre-registered
    /// by [`AppState::default_method_router`]. Embedders can register
    /// additional handlers via [`AppState::register_method`] before the
    /// server starts serving requests.
    pub method_router: Arc<super::router::MethodRouter>,
}

impl AppState {
    /// Remove cancellation entries older than [`CANCELLED_REQUEST_TTL`].
    ///
    /// Call this from a background task to prevent unbounded memory growth when
    /// clients cancel requests that have already completed.
    pub fn purge_expired_cancellations(&self) {
        self.cancelled_requests
            .retain(|_, recorded_at| recorded_at.elapsed() < CANCELLED_REQUEST_TTL);
    }

    /// Bump the registry generation counter and invalidate all per-session
    /// tool-list caches (issue #438).
    ///
    /// Call this after any registry mutation: skill load/unload, group
    /// activation/deactivation. The next `tools/list` call on any session
    /// will detect the generation mismatch and rebuild the snapshot.
    pub fn bump_registry_generation(&self) {
        let prev = self.registry_generation.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(prev_generation = prev, "registry generation bumped");
        self.sessions.invalidate_all_tool_list_snapshots();
    }

    /// Read the current registry generation counter (issue #438).
    pub fn current_registry_generation(&self) -> u64 {
        self.registry_generation.load(Ordering::Relaxed)
    }

    /// Build a default [`MethodRouter`](super::router::MethodRouter)
    /// pre-populated with every built-in MCP method (issue #492).
    pub fn default_method_router() -> Arc<super::router::MethodRouter> {
        Arc::new(super::router::MethodRouter::with_builtins())
    }

    /// Register a custom [`MethodHandler`](super::router::MethodHandler)
    /// for `method`. Replaces any previously-registered handler for the
    /// same method (issue #492).
    pub fn register_method(
        &self,
        method: impl Into<String>,
        handler: Arc<dyn super::router::MethodHandler>,
    ) {
        self.method_router.register(method, handler);
    }
}
