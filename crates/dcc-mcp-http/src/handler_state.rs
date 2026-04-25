//! Shared [`AppState`] owned by every axum handler in [`crate::handler`].
//!
//! Carries every long-lived registry (action / skill / resource /
//! prompt), the session manager, executor handle, and in-process
//! bookkeeping (cancellations, in-flight requests, pending
//! elicitations). Cloning `AppState` is cheap — every field is an
//! `Arc`-backed handle.

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
}
