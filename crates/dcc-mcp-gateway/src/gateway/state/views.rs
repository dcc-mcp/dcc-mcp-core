//! Typed gateway sub-state views (issue #839).
//!
//! These views let handlers depend on only the discovery, routing, eventing,
//! or server-identity slice they need while `GatewayState` continues to own
//! the backwards-compatible concrete fields.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::{RwLock, broadcast, watch};

use crate::gateway::event_log::EventLog;
use crate::gateway::http_registration::HttpInstanceRegistry;
use crate::gateway::mdns_registration::MdnsInstanceRegistry;
use crate::gateway::middleware::MiddlewareChain;
use crate::gateway::relay_registration::RelayInstanceRegistry;
use dcc_mcp_gateway_core::PendingCall;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceStatus};

const ROLE_METADATA_KEY: &str = "dcc_mcp_role";
const ROLE_PER_DCC_SIDECAR: &str = "per-dcc-sidecar";

// ─── Sub-state structs (issue #839) ─────────────────────────────────────────
//
// Each sub-state is a *typed view* over the subset of [`GatewayState`] that a
// given responsibility needs. They borrow from the parent state, so they are
// zero-cost to construct (no Arc bumps, no clones of non-shareable data).

/// Discovery / liveness view over gateway state (issue #839).
///
/// Handlers that only need to inspect the registry — e.g. `list_dcc_instances`,
/// the `live_instances` / `all_instances` helpers, and capability refresh —
/// should depend on this sub-state instead of the full [`GatewayState`].
#[derive(Clone, Copy)]
pub struct DiscoveryState<'a> {
    /// Shared read/write handle on the file-backed service registry.
    pub registry: &'a Arc<RwLock<FileRegistry>>,
    /// Shared in-memory registration source for remote HTTP-registered rows.
    pub http_instance_registry: &'a Arc<parking_lot::RwLock<HttpInstanceRegistry>>,
    /// Shared in-memory source for LAN mDNS-discovered rows.
    pub mdns_instance_registry: &'a Arc<parking_lot::RwLock<MdnsInstanceRegistry>>,
    /// Shared in-memory source for tunnel-relay-discovered rows.
    pub relay_instance_registry: &'a Arc<parking_lot::RwLock<RelayInstanceRegistry>>,
    /// Heartbeat-age after which a registry row is considered stale.
    pub stale_timeout: Duration,
    /// When `false` (default), instances advertising `dcc_type == "unknown"`
    /// are filtered from [`GatewayState::live_instances`]. Issue #555.
    pub allow_unknown_tools: bool,
    /// Gateway facade host; used together with `own_port` to filter the
    /// gateway's own self-row out of fan-out targets (issue #419).
    pub own_host: &'a str,
    /// Gateway facade port; pair of `own_host`.
    pub own_port: u16,
}

/// Routing / dispatch view over gateway state (issue #839).
///
/// Covers the per-call plumbing: outgoing HTTP, per-backend timeouts, and the
/// in-flight pending-call table used so `notifications/cancelled` can reach
/// the correct backend (issue #321 / #314).
#[derive(Clone, Copy)]
pub struct RoutingState<'a> {
    /// Shared reqwest client reused across all backend calls.
    pub http_client: &'a reqwest::Client,
    /// Per-backend request timeout for gateway fan-out calls (issue #314).
    pub backend_timeout: Duration,
    /// Longer timeout applied when the outbound `tools/call` is async-opt-in
    /// (issue #321).
    pub async_dispatch_timeout: Duration,
    /// Gateway wait-for-terminal passthrough timeout (issue #321).
    pub wait_terminal_timeout: Duration,
    /// In-flight forwarded tool calls keyed by gateway-side JSON-RPC id.
    pub pending_calls: &'a Arc<RwLock<HashMap<String, PendingCall>>>,
    /// Backend SSE multiplexer (issue #320).
    pub subscriber: &'a crate::gateway::sse_subscriber::SubscriberManager,
    /// Pluggable middleware chain applied to every `tools/call` dispatch
    /// (issue #770).
    pub middleware_chain: &'a Arc<MiddlewareChain>,
}

/// Eventing view over gateway state (issue #839).
///
/// Handlers that fan gateway-originated notifications out (instance watcher,
/// backend SSE subscriber, resource subscriptions, contention event log)
/// depend on just this sub-state.
#[derive(Clone, Copy)]
pub struct EventState<'a> {
    /// Broadcast channel for server-initiated MCP notifications pushed to SSE
    /// clients.
    pub events_tx: &'a Arc<broadcast::Sender<String>>,
    /// Per-session resource subscriptions keyed by `Mcp-Session-Id`.
    pub resource_subscriptions: &'a Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// Gateway-scoped capability index (issue #653).
    pub capability_index: &'a Arc<crate::gateway::capability::CapabilityIndex>,
    /// Contention event log (issue #766), exposed as the MCP resource
    /// `resources://gateway/events`.
    pub event_log: &'a Arc<EventLog>,
}

/// Identity / protocol view over gateway state (issue #839).
///
/// Covers non-routable metadata: server identity strings, the negotiated MCP
/// protocol version, adapter identity for version-aware election
/// (issue maya#137), and the voluntary-yield channel.
#[derive(Clone, Copy)]
pub struct ServerState<'a> {
    /// Gateway server name reported via MCP `initialize`.
    pub server_name: &'a str,
    /// Gateway server version.
    pub server_version: &'a str,
    /// Protocol version negotiated during the last `initialize` handshake.
    pub protocol_version: &'a Arc<RwLock<Option<String>>>,
    /// Adapter package version advertised on the `__gateway__` sentinel
    /// (issue maya#137).
    pub adapter_version: Option<&'a str>,
    /// DCC type the adapter is bound to (issue maya#137).
    pub adapter_dcc: Option<&'a str>,
    /// Voluntary-yield channel: sending `true` triggers graceful shutdown so
    /// a higher-version challenger can take over.
    pub yield_tx: &'a Arc<watch::Sender<bool>>,
}

impl<'a> DiscoveryState<'a> {
    /// See [`GatewayState::live_instances`].
    pub fn live_instances(&self, registry: &FileRegistry) -> Vec<ServiceEntry> {
        let filtered: Vec<ServiceEntry> = registry
            .list_all()
            .into_iter()
            .filter(|e| {
                e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                    && !e.is_stale(self.stale_timeout)
                    && !matches!(
                        e.status,
                        ServiceStatus::ShuttingDown
                            | ServiceStatus::Unreachable
                            | ServiceStatus::Booting
                            | ServiceStatus::Stale
                    )
                    && !crate::gateway::is_own_instance(e, self.own_host, self.own_port)
                    && (self.allow_unknown_tools || !e.dcc_type.eq_ignore_ascii_case("unknown"))
            })
            .collect();

        prefer_live_sidecars(self.merge_remote_entries(filtered, false))
    }

    /// See [`GatewayState::all_instances`].
    pub fn all_instances(&self, registry: &FileRegistry) -> Vec<ServiceEntry> {
        let filtered = registry
            .list_all()
            .into_iter()
            .filter(|e| {
                e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                    && !crate::gateway::is_own_instance(e, self.own_host, self.own_port)
            })
            .collect();
        self.merge_remote_entries(filtered, true)
    }

    /// See [`GatewayState::read_alive_instances`].
    pub fn read_alive_instances(
        &self,
        registry: &FileRegistry,
    ) -> dcc_mcp_transport::TransportResult<(Vec<ServiceEntry>, usize)> {
        let (raw, evicted) = registry.read_alive()?;
        let filtered = raw
            .into_iter()
            .filter(|e| {
                e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                    && !crate::gateway::is_own_instance(e, self.own_host, self.own_port)
            })
            .collect();
        Ok((self.merge_remote_entries(filtered, true), evicted))
    }

    fn merge_remote_entries(
        &self,
        file_entries: Vec<ServiceEntry>,
        include_unknown: bool,
    ) -> Vec<ServiceEntry> {
        let mdns_entries = if include_unknown {
            self.mdns_instance_registry
                .read()
                .all_entries(SystemTime::now())
        } else {
            self.mdns_instance_registry
                .read()
                .live_entries(SystemTime::now())
                .into_iter()
                .filter(|entry| {
                    self.allow_unknown_tools || !entry.dcc_type.eq_ignore_ascii_case("unknown")
                })
                .collect()
        };
        let relay_entries = if include_unknown {
            self.relay_instance_registry
                .read()
                .all_entries(SystemTime::now())
        } else {
            self.relay_instance_registry
                .read()
                .live_entries(SystemTime::now())
                .into_iter()
                .filter(|entry| {
                    self.allow_unknown_tools || !entry.dcc_type.eq_ignore_ascii_case("unknown")
                })
                .collect()
        };
        let http_entries = if include_unknown {
            self.http_instance_registry
                .read()
                .all_entries(SystemTime::now())
        } else {
            self.http_instance_registry
                .read()
                .live_entries(SystemTime::now())
                .into_iter()
                .filter(|entry| {
                    self.allow_unknown_tools || !entry.dcc_type.eq_ignore_ascii_case("unknown")
                })
                .collect()
        };
        merge_gateway_sources(file_entries, mdns_entries, relay_entries, http_entries)
    }
}

/// Merge discovery sources with deterministic conflict resolution:
/// HTTP > relay > mDNS > file.
pub(crate) fn merge_gateway_sources(
    mut file_entries: Vec<ServiceEntry>,
    mdns_entries: Vec<ServiceEntry>,
    relay_entries: Vec<ServiceEntry>,
    http_entries: Vec<ServiceEntry>,
) -> Vec<ServiceEntry> {
    let http_ids: HashSet<_> = http_entries.iter().map(|entry| entry.instance_id).collect();
    let relay_ids: HashSet<_> = relay_entries
        .iter()
        .map(|entry| entry.instance_id)
        .collect();
    let mdns_ids: HashSet<_> = mdns_entries.iter().map(|entry| entry.instance_id).collect();

    file_entries.retain(|entry| {
        !mdns_ids.contains(&entry.instance_id)
            && !relay_ids.contains(&entry.instance_id)
            && !http_ids.contains(&entry.instance_id)
    });

    let mut mdns_entries: Vec<_> = mdns_entries
        .into_iter()
        .filter(|entry| {
            !relay_ids.contains(&entry.instance_id) && !http_ids.contains(&entry.instance_id)
        })
        .collect();
    let mut relay_entries: Vec<_> = relay_entries
        .into_iter()
        .filter(|entry| !http_ids.contains(&entry.instance_id))
        .collect();

    file_entries.append(&mut mdns_entries);
    file_entries.append(&mut relay_entries);
    file_entries.extend(http_entries);
    file_entries
}

fn is_per_dcc_sidecar(entry: &ServiceEntry) -> bool {
    entry
        .metadata
        .get(ROLE_METADATA_KEY)
        .is_some_and(|role| role == ROLE_PER_DCC_SIDECAR)
}

fn sidecar_owner_key(entry: &ServiceEntry) -> Option<(String, u32)> {
    entry
        .pid
        .map(|pid| (entry.dcc_type.to_ascii_lowercase(), pid))
}

fn prefer_live_sidecars(entries: Vec<ServiceEntry>) -> Vec<ServiceEntry> {
    let sidecar_owners: HashSet<(String, u32)> = entries
        .iter()
        .filter(|entry| is_per_dcc_sidecar(entry))
        .filter_map(sidecar_owner_key)
        .collect();

    if sidecar_owners.is_empty() {
        return entries;
    }

    entries
        .into_iter()
        .filter(|entry| {
            is_per_dcc_sidecar(entry)
                || sidecar_owner_key(entry).is_none_or(|owner| !sidecar_owners.contains(&owner))
        })
        .collect()
}
