//! Shared gateway state and helpers.
//!
//! # SRP-compliant sub-state structs (issue #839)
//!
//! Historically [`GatewayState`] grew into a *god object* carrying 24+ fields
//! that mixed discovery, routing, eventing and identity concerns. Issue #839
//! calls for splitting these responsibilities into focused sub-structs so
//! future handlers can accept only the slice of state they actually need
//! (ISP + Dependency Inversion), tests can build just the sub-state they
//! exercise, and merge conflicts shrink.
//!
//! The refactor is intentionally **backwards-compatible**: every field that
//! existed on [`GatewayState`] prior to this change is still reachable from
//! the same `state.<field>` expression, so the 30 files that touch gateway
//! state keep compiling without a rename sweep. The new
//! [`DiscoveryState`] / [`RoutingState`] / [`EventState`] / [`ServerState`]
//! types provide a *typed view* over those fields for code that wants a
//! narrow dependency — they are cheap to construct (plain references / Arc
//! clones) and hold exactly the subset of state that their responsibility
//! implies.
//!
//! ## Responsibility map
//!
//! | Sub-state         | Responsibility                                            |
//! |-------------------|-----------------------------------------------------------|
//! | [`DiscoveryState`]| File registry + staleness / visibility policy             |
//! | [`RoutingState`]  | In-flight backend calls + timeouts + HTTP client          |
//! | [`EventState`]    | Event fan-out (broadcast, SSE, subscriptions, event log)  |
//! | [`ServerState`]   | Server identity, protocol negotiation, adapter metadata   |
//!
//! ## Migration strategy
//!
//! * Handlers that still use `state.registry`, `state.stale_timeout`, … work
//!   unchanged — those fields remain `pub` on `GatewayState`.
//! * New handlers and tests should prefer the typed accessors
//!   [`GatewayState::discovery`], [`GatewayState::routing`],
//!   [`GatewayState::events`], [`GatewayState::server`].
//! * The accessors borrow from `self`, so they are allocation-free.
//!
//! See issue #845 for the follow-on Clean-Architecture crate split that
//! builds on these sub-structs.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};

use super::event_log::EventLog;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};

use super::middleware::MiddlewareChain;

/// A call that the gateway has forwarded to a backend and is still awaiting.
///
/// Re-exported from [`dcc_mcp_gateway_core::PendingCall`] as part of the
/// Clean-Architecture split (issue #845). The domain type lives in
/// `dcc-mcp-gateway-core`; this re-export keeps the historical
/// `crate::gateway::state::PendingCall` / `crate::gateway::PendingCall`
/// path working for the ~30 files that already import it.
pub use dcc_mcp_gateway_core::PendingCall;

mod views;
pub use views::{DiscoveryState, EventState, RoutingState, ServerState};

/// Shared state passed to every gateway axum handler.
///
/// This struct owns the concrete fields; [`DiscoveryState`] / [`RoutingState`]
/// / [`EventState`] / [`ServerState`] are *views* constructed on demand via
/// [`Self::discovery`], [`Self::routing`], [`Self::events`], [`Self::server`]
/// (issue #839 — backwards-compatible SRP split).
#[derive(Clone)]
pub struct GatewayState {
    pub registry: Arc<RwLock<FileRegistry>>,
    pub stale_timeout: Duration,
    /// Per-backend request timeout for gateway fan-out calls (issue #314).
    ///
    /// Kept short by default (10s) so a single unresponsive instance does
    /// not stall aggregation, but configurable via
    /// [`McpHttpConfig::backend_timeout_ms`] for workflows with legitimately
    /// long-running backend tools.
    pub backend_timeout: Duration,
    /// Longer timeout applied when the outbound `tools/call` is async-
    /// opted-in (issue #321). Default: `60 s`.
    pub async_dispatch_timeout: Duration,
    /// Gateway wait-for-terminal passthrough timeout (issue #321).
    /// Default: `600 s` (10 minutes).
    pub wait_terminal_timeout: Duration,
    pub server_name: String,
    /// The version string of this gateway instance (e.g. `"0.12.29"`).
    pub server_version: String,
    /// Host the gateway is bound to (issue #419).
    ///
    /// Used together with [`Self::own_port`] to filter this gateway's own
    /// plain-instance row out of fan-out targets — without the filter, a
    /// DCC that wins gateway election would subscribe to its own `/mcp`
    /// endpoint via [`super::sse_subscriber::SubscriberManager`] and every
    /// `tools/list` / `tools/call` fan-out would recurse back into itself.
    pub own_host: String,
    /// Port the gateway's facade listens on (issue #419).
    pub own_port: u16,
    pub http_client: reqwest::Client,
    /// Sending side of the voluntary-yield channel.
    ///
    /// Sending `true` causes the gateway's HTTP server to perform a
    /// graceful shutdown and release the gateway port — allowing a
    /// higher-version challenger to take over.
    pub yield_tx: Arc<watch::Sender<bool>>,
    /// Broadcast channel for server-initiated MCP notifications pushed to SSE clients.
    ///
    /// The gateway's instance-watcher task sends JSON-RPC notification strings here
    /// whenever the set of live DCC instances changes.  Every connected SSE client
    /// (`GET /mcp`) subscribes and forwards the messages to the MCP client.
    ///
    /// Channel capacity 128 is intentionally generous: at a 2-second poll interval,
    /// there will never be more than a handful of pending messages.
    pub events_tx: Arc<broadcast::Sender<String>>,
    /// Protocol version negotiated during the last `initialize` handshake.
    ///
    /// `None` until the first client calls `initialize`.  Used so that the
    /// gateway can adapt its behaviour (e.g. `outputSchema` inclusion) to the
    /// client's supported protocol version.
    pub protocol_version: Arc<RwLock<Option<String>>>,
    /// Per-session resource subscriptions.
    ///
    /// Key = `Mcp-Session-Id`, value = set of subscribed URIs.
    /// Populated by `resources/subscribe` and pruned by `resources/unsubscribe`.
    pub resource_subscriptions: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// In-flight forwarded tool calls so that `notifications/cancelled` can be
    /// routed to the correct backend.
    ///
    /// Key = gateway-side JSON-RPC request `id` (serialised to string).
    /// Value = backend URL + the request id used when talking to that backend.
    pub pending_calls: Arc<RwLock<HashMap<String, PendingCall>>>,
    /// Backend SSE multiplexer (#320).
    ///
    /// Each live backend gets a long-lived SSE subscription; incoming
    /// `notifications/progress` / `$/dcc.jobUpdated` /
    /// `$/dcc.workflowUpdated` are routed to the originating client
    /// session via `progressToken` and `job_id` correlation.
    pub subscriber: super::sse_subscriber::SubscriberManager,
    /// Allow instances with `dcc_type == "unknown"` to expose their tools
    /// and be selectable via `connect_to_dcc` (issue #555).
    ///
    /// When `false` (default), `live_instances` filters them out.
    pub allow_unknown_tools: bool,
    /// Adapter package version (e.g. `dcc_mcp_maya = "0.3.0"`) advertised
    /// by this gateway on its `__gateway__` sentinel and used by the
    /// version-aware election comparison (issue maya#137).
    pub adapter_version: Option<String>,
    /// DCC type the adapter is bound to (e.g. `"maya"`) — drives the
    /// real-DCC vs `"unknown"` tiebreaker in gateway election
    /// (issue maya#137).
    pub adapter_dcc: Option<String>,
    /// Emit Cursor-safe names (`i_<id8>__<escaped>`) when fanning prompts
    /// out to backends (issue #656). Default: `true`.
    ///
    /// Tools no longer fan out to `tools/list` — the gateway exposes only
    /// its discover+dispatch primitives there — but prompts still go
    /// through the per-instance aggregator so clients that talk to
    /// multiple DCCs keep a unique address per prompt. Kept `true` by
    /// default so Cursor and other strict clients see every prompt.
    pub cursor_safe_tool_names: bool,

    /// Gateway-scoped capability index (issue #653). Populated by the
    /// refresh loop and queried by the REST / MCP dynamic-capability
    /// wrappers (#654 / #655).
    ///
    /// `Arc` so every handler can hold an owned, cheaply-cloned
    /// reference; the inner `RwLock` is held only for the
    /// milliseconds it takes to swap a single instance's slice.
    pub capability_index: Arc<super::capability::CapabilityIndex>,

    /// Contention event log (issue #766).
    ///
    /// Append-only JSONL ring buffer (bounded to [`EventLog::CAPACITY`]).
    /// Exposed as the MCP resource `resources://gateway/events`.
    /// Also drives Prometheus counters when the `prometheus` feature is on.
    pub event_log: Arc<EventLog>,

    /// Prometheus contention counters (issue #766).
    ///
    /// Compiled only when `prometheus` feature is enabled. Shared via `Arc`
    /// so both `tasks.rs` (instrumentation points) and `metrics.rs`
    /// (endpoint handler) refer to the same registered counter objects.
    #[cfg(feature = "prometheus")]
    pub gateway_metrics: Arc<super::event_log::GatewayMetrics>,

    /// Pluggable middleware chain applied to every `tools/call` dispatch
    /// (issue #770). Empty by default; operators register middlewares via
    /// [`GatewayConfig::middleware_chain`] or the builder API.
    pub middleware_chain: Arc<MiddlewareChain>,
}

impl GatewayState {
    // ── Sub-state accessors (issue #839) ───────────────────────────────────
    //
    // These return zero-cost typed views over the subset of fields each
    // responsibility needs. New handlers should prefer these accessors so
    // that their signatures advertise exactly which slice of gateway state
    // they touch.

    /// Typed discovery view (registry + staleness + visibility policy).
    pub fn discovery(&self) -> DiscoveryState<'_> {
        DiscoveryState {
            registry: &self.registry,
            stale_timeout: self.stale_timeout,
            allow_unknown_tools: self.allow_unknown_tools,
            own_host: &self.own_host,
            own_port: self.own_port,
        }
    }

    /// Typed routing view (HTTP client + timeouts + pending-call table).
    pub fn routing(&self) -> RoutingState<'_> {
        RoutingState {
            http_client: &self.http_client,
            backend_timeout: self.backend_timeout,
            async_dispatch_timeout: self.async_dispatch_timeout,
            wait_terminal_timeout: self.wait_terminal_timeout,
            pending_calls: &self.pending_calls,
            subscriber: &self.subscriber,
            middleware_chain: &self.middleware_chain,
            cursor_safe_tool_names: self.cursor_safe_tool_names,
        }
    }

    /// Typed eventing view (broadcast + subscriptions + capability index +
    /// event log).
    pub fn events(&self) -> EventState<'_> {
        EventState {
            events_tx: &self.events_tx,
            resource_subscriptions: &self.resource_subscriptions,
            capability_index: &self.capability_index,
            event_log: &self.event_log,
        }
    }

    /// Typed server-identity view (server/adapter metadata, protocol
    /// version, yield channel).
    pub fn server(&self) -> ServerState<'_> {
        ServerState {
            server_name: &self.server_name,
            server_version: &self.server_version,
            protocol_version: &self.protocol_version,
            adapter_version: self.adapter_version.as_deref(),
            adapter_dcc: self.adapter_dcc.as_deref(),
            yield_tx: &self.yield_tx,
        }
    }

    /// Return all instances that are live (not stale, not shutting down/unreachable).
    ///
    /// The `__gateway__` sentinel row is **always** excluded — it is bookkeeping
    /// for gateway election, not an addressable DCC instance.  Including it in
    /// user-facing tool output (`list_dcc_instances`, `get_dcc_instance`,
    /// `connect_to_dcc`) would confuse agents and break the `mcp_url` contract
    /// (the sentinel's host:port points at the gateway facade, not a real DCC).
    ///
    /// The gateway's **own plain-instance row** is also excluded (issue #419).
    /// When a DCC process (e.g. Maya) wins gateway election it keeps both the
    /// sentinel and a regular `"maya"` row; exposing its own row here would
    /// cause the facade to fan `tools/list` / `tools/call` back into itself.
    pub fn live_instances(&self, registry: &FileRegistry) -> Vec<ServiceEntry> {
        self.discovery().live_instances(registry)
    }

    /// Return every parseable registry row that an operator-facing tool
    /// (e.g. `list_dcc_instances`) should expose, regardless of liveness.
    ///
    /// Issue maya#138: the previous implementation reused [`live_instances`],
    /// which silently dropped stale, shutting-down, and `dcc_type == "unknown"`
    /// rows.  Operators eyeballing the registry directory then saw three
    /// sentinels but only one row in the tool output, with no signal as to
    /// why the others vanished.  The expanded view excludes only the bookkeeping
    /// `__gateway__` sentinel and the gateway's own self row, so callers can
    /// render a full picture and downgrade stale entries via the surface
    /// `status` field instead.
    pub fn all_instances(&self, registry: &FileRegistry) -> Vec<ServiceEntry> {
        self.discovery().all_instances(registry)
    }

    /// Return operator-facing registry rows with dead-PID entries pruned.
    ///
    /// Issue #719: before this method existed, `list_dcc_instances` could
    /// return rows whose owning DCC process had already exited — for up to
    /// `stale_timeout_secs` (default 30 s) after the process died, or
    /// indefinitely if no gateway process was running the periodic sweep.
    /// Agents then routed `call_tool` / `acquire_dcc_instance` to a dead
    /// backend and hit connection-refused.
    ///
    /// This is a self-healing read path: every call consults
    /// [`FileRegistry::read_alive`] which probes each row's `pid` field via
    /// `sysinfo` and evicts dead-PID rows from both the in-memory view and
    /// the on-disk `services.json` before returning. The same
    /// sentinel / self-row filters that [`Self::all_instances`] uses are
    /// applied after the prune so the caller sees the identical view
    /// minus the zombies.
    ///
    /// Fail-open contract (#227): rows with no `pid` are considered alive
    /// and survive the prune. `FileRegistry::read_alive` enforces this
    /// internally; we do not re-check it here.
    ///
    /// Returns `(alive_entries, evicted_count)`. `evicted_count` is the
    /// total number of dead-PID rows the registry dropped — callers can
    /// surface it so operators notice when a backend crashed without
    /// deregistering. Note that the count reflects every dead row the
    /// registry held, not just rows that would have passed the gateway's
    /// own filters (e.g. a zombie `__gateway__` sentinel still bumps the
    /// count even though it is filtered out of the returned slice).
    pub fn read_alive_instances(
        &self,
        registry: &FileRegistry,
    ) -> dcc_mcp_transport::TransportResult<(Vec<ServiceEntry>, usize)> {
        self.discovery().read_alive_instances(registry)
    }
}

/// Serialize a `ServiceEntry` to a JSON `Value` suitable for gateway responses.
///
/// The returned object always contains every field so that MCP clients can
/// display a rich disambiguation prompt when multiple instances of the same
/// DCC type are live at the same time.
///
/// Issue maya#138: when the entry is past `stale_timeout` the surface
/// `status` field is reported as `"stale"` so operators inspecting
/// `list_dcc_instances` can immediately tell why a registry row is no
/// longer routable.  The original `ServiceStatus` is preserved verbatim
/// when the row is still live, and the redundant `stale: bool` field is
/// kept for clients that prefer to branch on a boolean.
pub fn entry_to_json(e: &ServiceEntry, stale_timeout: Duration) -> Value {
    let stale = e.is_stale(stale_timeout) || e.status == ServiceStatus::Stale;
    let status = if stale {
        "stale".to_string()
    } else {
        e.status.to_string()
    };
    json!({
        "instance_id":     e.instance_id.to_string(),
        "dcc_type":        e.dcc_type,
        "host":            e.host,
        "port":            e.port,
        "mcp_url":         format!("http://{}:{}/mcp", e.host, e.port),
        "status":          status,
        // ── document / scene ───────────────────────────────────────────────
        // `scene` is the active / primary document (same field as before).
        // `documents` is the full list for multi-document apps (Photoshop etc.).
        "scene":           e.scene,
        "documents":       e.documents,
        // ── disambiguation helpers ─────────────────────────────────────────
        // Both fields are null when not set; agents should skip null values
        // when building the disambiguation prompt for users.
        "pid":             e.pid,
        "display_name":    e.display_name,
        // ── misc ───────────────────────────────────────────────────────────
        "version":         e.version,
        "adapter_version": e.adapter_version,
        "adapter_dcc":     e.adapter_dcc,
        "metadata":        e.metadata,
        "pool": {
            "capacity": e.capacity,
            "lease_owner": e.lease_owner,
            "current_job_id": e.current_job_id,
            "lease_expires_at": e.lease_expires_at
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
            "available": !stale && e.status == ServiceStatus::Available && e.lease_owner.is_none(),
        },
        "stale":           stale,
    })
}

#[cfg(test)]
mod tests;
