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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};
use uuid::Uuid;

use dcc_mcp_gateway_core::naming::instance_short;
use dcc_mcp_gateway_core::policy::GatewayPolicy;

use super::event_log::EventLog;
use super::http_registration::{HttpInstanceRegistry, entry_mcp_url, entry_registry_source};
use super::instance_diagnostics::{InstanceDiagnostics, InstanceDiagnosticsStore};
use super::mdns_registration::MdnsInstanceRegistry;
use super::relay_registration::RelayInstanceRegistry;

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
pub(crate) use views::merge_gateway_sources;
pub use views::{DiscoveryState, EventState, RoutingState, ServerState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveInstanceError {
    PrefixTooShort {
        prefix: String,
        min_len: usize,
    },
    NoMatch {
        hint: Option<String>,
        dcc: Option<String>,
    },
    MultipleMatches {
        candidates: Vec<String>,
    },
}

impl fmt::Display for ResolveInstanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrefixTooShort { prefix, min_len } => write!(
                f,
                "prefix-too-short: instance_id prefix '{prefix}' is shorter than {min_len} characters"
            ),
            Self::NoMatch {
                hint: Some(hint), ..
            } => {
                write!(
                    f,
                    "no-live-instance-match: no live instance matches instance_id='{hint}'"
                )
            }
            Self::NoMatch {
                hint: None,
                dcc: Some(dcc),
            } => {
                write!(f, "no-live-instance-match: no live '{dcc}' instance")
            }
            Self::NoMatch {
                hint: None,
                dcc: None,
            } => {
                write!(f, "no-live-instance-match: no live DCC instances")
            }
            Self::MultipleMatches { candidates } => write!(
                f,
                "multiple-instances-match: specify instance_id; candidates=[{}]",
                candidates.join(", ")
            ),
        }
    }
}

/// Shared state passed to every gateway axum handler.
///
/// This struct owns the concrete fields; [`DiscoveryState`] / [`RoutingState`]
/// / [`EventState`] / [`ServerState`] are *views* constructed on demand via
/// [`Self::discovery`], [`Self::routing`], [`Self::events`], [`Self::server`]
/// (issue #839 — backwards-compatible SRP split).
#[derive(Clone)]
pub struct GatewayState {
    pub registry: Arc<RwLock<FileRegistry>>,
    pub http_instance_registry: Arc<parking_lot::RwLock<HttpInstanceRegistry>>,
    pub mdns_instance_registry: Arc<parking_lot::RwLock<MdnsInstanceRegistry>>,
    pub relay_instance_registry: Arc<parking_lot::RwLock<RelayInstanceRegistry>>,
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
    /// MCP initialize client attribution keyed by `Mcp-Session-Id`.
    ///
    /// The store is bounded and only keeps protocol client identity fields, so
    /// later calls can be attributed without persisting raw request bodies.
    pub client_attribution: Arc<super::ClientAttributionStore>,
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
    /// Gateway-scoped dynamic capability policy.
    pub policy: Arc<GatewayPolicy>,
    /// Adapter package version (e.g. `dcc_mcp_maya = "0.3.0"`) advertised
    /// by this gateway on its `__gateway__` sentinel and used by the
    /// version-aware election comparison (issue maya#137).
    pub adapter_version: Option<String>,
    /// DCC type the adapter is bound to (e.g. `"maya"`) — drives the
    /// real-DCC vs `"unknown"` tiebreaker in gateway election
    /// (issue maya#137).
    pub adapter_dcc: Option<String>,

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

    /// Cached per-instance readiness + last call error (#1076).
    pub instance_diagnostics: Arc<InstanceDiagnosticsStore>,

    /// Opt-in development traffic capture (RFC 0003 P0).
    pub traffic_capture: Arc<super::traffic::TrafficCapture>,

    /// Search-quality telemetry store used by admin/debug APIs and metrics.
    pub search_telemetry: Arc<super::search_telemetry::SearchTelemetryStore>,

    /// Whether stable gateway `/v1/debug/*` routes are mounted on this router.
    ///
    /// The routes require the `admin` feature at compile time and an AdminState
    /// at runtime. OpenAPI generation reads this so the published contract does
    /// not advertise routes disabled by `--no-admin` / `admin_enabled = false`.
    pub debug_routes_enabled: bool,
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
            http_instance_registry: &self.http_instance_registry,
            mdns_instance_registry: &self.mdns_instance_registry,
            relay_instance_registry: &self.relay_instance_registry,
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

    /// MCP URL for this gateway process.
    #[must_use]
    pub fn gateway_mcp_url(&self) -> String {
        format!("http://{}:{}/mcp", self.own_host, self.own_port)
    }

    /// Serialize a registry row with optional cached diagnostics (#1076).
    #[must_use]
    pub fn instance_json(&self, e: &ServiceEntry) -> Value {
        let diag = self.instance_diagnostics.get(&e.instance_id);
        let mut row = entry_to_json(e, self.stale_timeout, diag.as_ref());
        let app_ui = app_ui_diagnostics(e, &self.capability_index.snapshot());
        if !row.get("diagnostics").is_some_and(Value::is_object) {
            row["diagnostics"] = json!({});
        }
        row["diagnostics"]["app_ui"] = app_ui;
        row
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
        self.prune_expired_http_instances();
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
        self.prune_expired_http_instances();
        self.discovery().all_instances(registry)
    }

    /// Resolve a user-provided instance hint against the shared live-instance view.
    pub fn resolve_instance(
        &self,
        registry: &FileRegistry,
        instance_hint: Option<&str>,
        dcc_filter: Option<&str>,
    ) -> Result<ServiceEntry, ResolveInstanceError> {
        const MIN_PREFIX_LEN: usize = 4;

        let candidates: Vec<ServiceEntry> = self
            .live_instances(registry)
            .into_iter()
            .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type.eq_ignore_ascii_case(f)))
            .collect();

        if let Some(raw_hint) = instance_hint.map(str::trim).filter(|hint| !hint.is_empty()) {
            if let Ok(uuid) = Uuid::parse_str(raw_hint) {
                return candidates
                    .into_iter()
                    .find(|e| e.instance_id == uuid)
                    .ok_or_else(|| ResolveInstanceError::NoMatch {
                        hint: Some(raw_hint.to_string()),
                        dcc: dcc_filter.map(str::to_string),
                    });
            }

            let hint = raw_hint.to_ascii_lowercase();
            if hint.len() < MIN_PREFIX_LEN {
                return Err(ResolveInstanceError::PrefixTooShort {
                    prefix: raw_hint.to_string(),
                    min_len: MIN_PREFIX_LEN,
                });
            }

            let matches: Vec<ServiceEntry> = candidates
                .into_iter()
                .filter(|e| e.instance_id.simple().to_string().starts_with(&hint))
                .collect();
            return match matches.as_slice() {
                [] => Err(ResolveInstanceError::NoMatch {
                    hint: Some(raw_hint.to_string()),
                    dcc: dcc_filter.map(str::to_string),
                }),
                [entry] => Ok(entry.clone()),
                _ => Err(ResolveInstanceError::MultipleMatches {
                    candidates: matches.iter().map(instance_candidate).collect(),
                }),
            };
        }

        match candidates.as_slice() {
            [] => Err(ResolveInstanceError::NoMatch {
                hint: None,
                dcc: dcc_filter.map(str::to_string),
            }),
            [entry] => Ok(entry.clone()),
            _ => Err(ResolveInstanceError::MultipleMatches {
                candidates: candidates.iter().map(instance_candidate).collect(),
            }),
        }
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
        self.prune_expired_http_instances();
        self.discovery().read_alive_instances(registry)
    }

    fn prune_expired_http_instances(&self) {
        let expired = self
            .http_instance_registry
            .write()
            .prune_expired(std::time::SystemTime::now());
        for instance_id in expired {
            self.capability_index.remove_instance(instance_id);
        }
    }
}

fn instance_candidate(entry: &ServiceEntry) -> String {
    format!(
        "{}:{}:{}",
        entry.dcc_type,
        entry.instance_id,
        entry_to_short(&entry.instance_id),
    )
}

fn entry_to_short(instance_id: &Uuid) -> String {
    instance_id.simple().to_string()[..8].to_string()
}

fn first_metadata_value<'a>(
    metadata: &'a HashMap<String, String>,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| metadata.get(*key).map(String::as_str))
        .find(|value| !value.trim().is_empty())
}

fn lifecycle_json(e: &ServiceEntry) -> Value {
    let role = e
        .metadata
        .get("dcc_mcp_role")
        .map(String::as_str)
        .unwrap_or("runtime");
    let sidecar_pid = e
        .metadata
        .get("sidecar_pid")
        .and_then(|value| value.parse::<u32>().ok());
    let restart_command =
        first_metadata_value(&e.metadata, &["restart_command", "dcc_mcp_restart_command"]);
    let launch_command =
        first_metadata_value(&e.metadata, &["launch_command", "dcc_mcp_launch_command"]);
    let owner = first_metadata_value(
        &e.metadata,
        &[
            "owner",
            "test_owner",
            "dcc_mcp_owner",
            "dcc_mcp_test_owner",
            "dcc_mcp.owner",
        ],
    );
    let session = first_metadata_value(
        &e.metadata,
        &[
            "session",
            "test_session",
            "dcc_mcp_session",
            "dcc_mcp_test_session",
            "dcc_mcp.session",
        ],
    );
    let safe_stop_url = first_metadata_value(
        &e.metadata,
        &[
            "safe_stop_url",
            "dcc_mcp_safe_stop_url",
            "dcc_mcp.safe_stop_url",
            "stop_url",
        ],
    );
    let safe_stop_method = first_metadata_value(
        &e.metadata,
        &[
            "safe_stop_method",
            "dcc_mcp_safe_stop_method",
            "dcc_mcp.safe_stop_method",
        ],
    )
    .unwrap_or("POST");
    let install_root = first_metadata_value(
        &e.metadata,
        &[
            "install_root",
            "adapter_root",
            "adapter_install_root",
            "package_root",
            "root_path",
        ],
    );

    json!({
        "role": role,
        "owner": owner,
        "session": session,
        "sidecar_pid": sidecar_pid,
        "supports_safe_stop": sidecar_pid.is_some() || safe_stop_url.is_some(),
        "safe_stop_url": safe_stop_url,
        "safe_stop_method": safe_stop_method,
        "restartable": sidecar_pid.is_some() || restart_command.is_some() || launch_command.is_some(),
        "restart_command": restart_command,
        "launch_command": launch_command,
        "install_root": install_root,
    })
}

fn app_ui_diagnostics(e: &ServiceEntry, snap: &crate::gateway::capability::IndexSnapshot) -> Value {
    let explicit_status = first_metadata_value(
        &e.metadata,
        &[
            "app_ui.status",
            "dcc_mcp_app_ui_status",
            "dcc_mcp.app_ui.status",
        ],
    )
    .map(|value| value.trim().to_ascii_lowercase());
    let explicit_reason = first_metadata_value(
        &e.metadata,
        &[
            "app_ui.reason",
            "dcc_mcp_app_ui_reason",
            "dcc_mcp.app_ui.reason",
        ],
    );

    let mut tools: Vec<String> = snap
        .records
        .iter()
        .filter(|record| record.instance_id == e.instance_id)
        .filter(|record| record.loaded)
        .filter(|record| record.callable_id.starts_with("app_ui__"))
        .map(|record| record.callable_id.clone())
        .collect();
    tools.sort();
    tools.dedup();

    let status = match explicit_status.as_deref() {
        Some("disabled") | Some("disabled_by_policy") | Some("policy_disabled") => {
            "disabled_by_policy"
        }
        Some("available") => "available",
        Some("unavailable") => "unavailable",
        _ if tools.is_empty() => "unavailable",
        _ => "available",
    };

    json!({
        "status": status,
        "tools": tools,
        "reason": explicit_reason,
    })
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
pub fn entry_to_json(
    e: &ServiceEntry,
    stale_timeout: Duration,
    diagnostics: Option<&InstanceDiagnostics>,
) -> Value {
    let stale = e.is_stale(stale_timeout) || e.status == ServiceStatus::Stale;
    let status = if stale {
        "stale".to_string()
    } else {
        e.status.to_string()
    };
    let source_meta = e
        .extras
        .get("source_meta")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut row = json!({
        "instance_id":     e.instance_id.to_string(),
        "instance_short":  instance_short(&e.instance_id),
        // Derived `{dcc}@{version}-{short8}` (RFC #998 Addendum B).
        // Agents reading gateway://instances see DCC + version + short
        // ID inline without cross-referencing three separate fields.
        "display_id":      e.display_id(),
        "dcc_type":        e.dcc_type,
        "host":            e.host,
        "port":            e.port,
        "mcp_url":         entry_mcp_url(e),
        "source":          entry_registry_source(e),
        "source_meta":     source_meta,
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
        "lifecycle":       lifecycle_json(e),
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
    });
    // ── host execution readiness (issue #1331) ────────────────────────────
    //
    // Always include this summary so admin/agent surfaces can branch on a
    // single string without parsing the nested `diagnostics.readiness`
    // block. Status is `unknown` when no readiness probe has reported yet.
    let host_exec = super::instance_diagnostics::HostExecutionStatus::from_diagnostics(diagnostics);
    let missing_bits = super::instance_diagnostics::HostExecutionStatus::missing_bits(diagnostics);
    row["host_execution"] = json!({
        "status": host_exec.label(),
        "missing_bits": missing_bits,
    });
    if let Some(diag) = diagnostics.filter(|d| {
        d.readiness.is_some() || d.last_error.is_some() || d.probed_at_unix_secs.is_some()
    }) {
        row["diagnostics"] =
            super::instance_diagnostics::InstanceDiagnosticsStore::to_json_value(diag);
    }
    row
}

/// Count instance rows by their additive `source` field.
pub(crate) fn instance_source_counts(rows: &[Value]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for source in ["file", "http", "mdns", "relay"] {
        counts.insert(source.to_string(), 0);
    }
    for row in rows {
        let source = row
            .get("source")
            .and_then(Value::as_str)
            .filter(|source| !source.trim().is_empty())
            .unwrap_or("file");
        *counts.entry(source.to_string()).or_default() += 1;
    }
    counts
}

#[cfg(test)]
mod tests;
