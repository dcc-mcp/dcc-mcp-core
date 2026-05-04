//! Shared gateway state and helpers.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceStatus};

/// A call that the gateway has forwarded to a backend and is still awaiting.
#[derive(Debug, Clone)]
pub struct PendingCall {
    pub backend_url: String,
    pub backend_request_id: String,
}

/// Shared state passed to every gateway axum handler.
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
    /// Gateway tool-exposure mode (issue #652).
    ///
    /// Controls whether `tools/list` fans out to every live backend
    /// (`Full` / `Both`) or stays bounded at the gateway meta-tools +
    /// skill-management surface (`Slim` / `Rest`). Default: `Full`.
    pub tool_exposure: super::config::GatewayToolExposure,

    /// Emit Cursor-safe tool names (`i_<id8>__<escaped>`) when fanning
    /// out to backends (issue #656). Default: `true`. Only consulted
    /// when [`Self::tool_exposure`] is a fan-out mode.
    pub cursor_safe_tool_names: bool,

    /// Gateway-scoped capability index (issue #653). Populated by the
    /// refresh loop and queried by the REST / MCP dynamic-capability
    /// wrappers (#654 / #655).
    ///
    /// `Arc` so every handler can hold an owned, cheaply-cloned
    /// reference; the inner `RwLock` is held only for the
    /// milliseconds it takes to swap a single instance's slice.
    pub capability_index: Arc<super::capability::CapabilityIndex>,
}

impl GatewayState {
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
        registry
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
                    )
                    && !super::is_own_instance(e, &self.own_host, self.own_port)
                    && (self.allow_unknown_tools || !e.dcc_type.eq_ignore_ascii_case("unknown"))
            })
            .collect()
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
        registry
            .list_all()
            .into_iter()
            .filter(|e| {
                e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                    && !super::is_own_instance(e, &self.own_host, self.own_port)
            })
            .collect()
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
        let (raw, evicted) = registry.read_alive()?;
        let filtered = raw
            .into_iter()
            .filter(|e| {
                e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                    && !super::is_own_instance(e, &self.own_host, self.own_port)
            })
            .collect();
        Ok((filtered, evicted))
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
    let stale = e.is_stale(stale_timeout);
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
mod tests {
    use super::*;
    use dcc_mcp_transport::discovery::types::ServiceEntry;
    use std::sync::Arc;
    use tokio::sync::{RwLock, broadcast, watch};

    fn test_gateway_state(reg: Arc<RwLock<FileRegistry>>) -> GatewayState {
        test_gateway_state_with_own(reg, "127.0.0.1", 9765)
    }

    fn test_gateway_state_with_own(
        reg: Arc<RwLock<FileRegistry>>,
        own_host: &str,
        own_port: u16,
    ) -> GatewayState {
        test_gateway_state_with_own_and_unknown(reg, own_host, own_port, false)
    }

    fn test_gateway_state_with_own_and_unknown(
        reg: Arc<RwLock<FileRegistry>>,
        own_host: &str,
        own_port: u16,
        allow_unknown_tools: bool,
    ) -> GatewayState {
        let (yield_tx, _) = watch::channel(false);
        let (events_tx, _) = broadcast::channel::<String>(8);
        GatewayState {
            registry: reg,
            stale_timeout: Duration::from_secs(30),
            backend_timeout: Duration::from_secs(10),
            async_dispatch_timeout: Duration::from_secs(60),
            wait_terminal_timeout: Duration::from_secs(600),
            server_name: "test".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
            own_host: own_host.to_string(),
            own_port,
            http_client: reqwest::Client::new(),
            yield_tx: Arc::new(yield_tx),
            events_tx: Arc::new(events_tx),
            protocol_version: Arc::new(RwLock::new(None)),
            resource_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            pending_calls: Arc::new(RwLock::new(HashMap::new())),
            subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
            allow_unknown_tools,
            adapter_version: None,
            adapter_dcc: None,
            tool_exposure: crate::gateway::GatewayToolExposure::Rest,
            cursor_safe_tool_names: true,
            capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        }
    }

    // Regression test for the sibling of issue #230: the `__gateway__` sentinel
    // must never appear in user-facing DCC instance listings (e.g.
    // `list_dcc_instances`, `get_dcc_instance`, `connect_to_dcc`). Exposing
    // it would invite agents to `connect_to_dcc("__gateway__")` and loop
    // requests back through the gateway facade.
    #[tokio::test]
    async fn test_live_instances_excludes_gateway_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

        {
            let r = registry.read().await;
            let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
            sentinel.version = Some(env!("CARGO_PKG_VERSION").into());
            r.register(sentinel).unwrap();

            let maya = ServiceEntry::new("maya", "127.0.0.1", 18812);
            r.register(maya).unwrap();
        }

        let gs = test_gateway_state(registry.clone());
        let live = gs.live_instances(&*registry.read().await);
        assert_eq!(live.len(), 1, "only the maya row should be returned");
        assert_eq!(live[0].dcc_type, "maya");
        assert!(
            !live.iter().any(|e| e.dcc_type == GATEWAY_SENTINEL_DCC_TYPE),
            "gateway sentinel must never appear in user-facing listings"
        );
    }

    /// Regression test for issue #419: when the gateway process is also a
    /// DCC instance (e.g. Maya that won the gateway election), its own
    /// plain-instance row must be hidden from `live_instances` so the
    /// facade does not fan `tools/list` / `tools/call` back into itself.
    #[tokio::test]
    async fn test_live_instances_excludes_gateway_self_row() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

        {
            let r = registry.read().await;
            // The sentinel + the gateway's own DCC row share host/port.
            let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
            sentinel.version = Some(env!("CARGO_PKG_VERSION").into());
            r.register(sentinel).unwrap();

            // Self DCC row — same host/port as the gateway facade.
            let maya_self = ServiceEntry::new("maya", "127.0.0.1", 9765);
            r.register(maya_self).unwrap();

            // A second Maya instance on a different port — must survive.
            let maya_other = ServiceEntry::new("maya", "127.0.0.1", 18812);
            r.register(maya_other).unwrap();
        }

        let gs = test_gateway_state_with_own(registry.clone(), "127.0.0.1", 9765);
        let live = gs.live_instances(&*registry.read().await);
        assert_eq!(
            live.len(),
            1,
            "only the non-self maya row should remain; got {live:#?}"
        );
        assert_eq!(live[0].port, 18812);
    }

    /// Regression test for issue #419: `localhost` / `::1` / `0.0.0.0` must
    /// all normalise to the same address so that a gateway bound on
    /// `127.0.0.1` still filters out a self-row advertised as `localhost`
    /// (DCC adapters vary in how they populate `ServiceEntry::host`).
    #[tokio::test]
    async fn test_live_instances_self_row_localhost_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

        {
            let r = registry.read().await;
            // Self row advertised as "localhost" — must still be filtered
            // when the gateway is bound to 127.0.0.1.
            let maya_self = ServiceEntry::new("maya", "localhost", 9765);
            r.register(maya_self).unwrap();
        }

        let gs = test_gateway_state_with_own(registry.clone(), "127.0.0.1", 9765);
        let live = gs.live_instances(&*registry.read().await);
        assert!(
            live.is_empty(),
            "self row with localhost alias must be filtered; got {live:#?}"
        );
    }

    /// Issue #555: instances with `dcc_type == "unknown"` must be hidden from
    /// `live_instances` when `allow_unknown_tools` is `false` (the default).
    #[tokio::test]
    async fn test_live_instances_hides_unknown_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

        {
            let r = registry.read().await;
            let unknown = ServiceEntry::new("unknown", "127.0.0.1", 18812);
            r.register(unknown).unwrap();

            let maya = ServiceEntry::new("maya", "127.0.0.1", 18813);
            r.register(maya).unwrap();
        }

        let gs =
            test_gateway_state_with_own_and_unknown(registry.clone(), "127.0.0.1", 9765, false);
        let live = gs.live_instances(&*registry.read().await);
        assert_eq!(live.len(), 1, "only the maya row should be returned");
        assert_eq!(live[0].dcc_type, "maya");
        assert!(
            !live
                .iter()
                .any(|e| e.dcc_type.eq_ignore_ascii_case("unknown")),
            "unknown dcc_type must be filtered when allow_unknown_tools is false"
        );
    }

    /// Issue #555: when `allow_unknown_tools` is `true`, unknown instances
    /// survive the filter.
    #[tokio::test]
    async fn test_live_instances_shows_unknown_when_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

        {
            let r = registry.read().await;
            let unknown = ServiceEntry::new("unknown", "127.0.0.1", 18812);
            r.register(unknown).unwrap();

            let maya = ServiceEntry::new("maya", "127.0.0.1", 18813);
            r.register(maya).unwrap();
        }

        let gs = test_gateway_state_with_own_and_unknown(registry.clone(), "127.0.0.1", 9765, true);
        let live = gs.live_instances(&*registry.read().await);
        assert_eq!(live.len(), 2, "both rows should be returned when allowed");
        assert!(
            live.iter()
                .any(|e| e.dcc_type.eq_ignore_ascii_case("unknown")),
            "unknown dcc_type must be present when allow_unknown_tools is true"
        );
    }

    /// Issue maya#138: `all_instances` keeps stale and `unknown` rows
    /// (dropping only the gateway sentinel and the gateway's own
    /// self-row) so the operator-facing `list_dcc_instances` tool can
    /// surface a complete picture of the registry directory.
    #[tokio::test]
    async fn test_all_instances_keeps_stale_and_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

        {
            let r = registry.read().await;
            // The bookkeeping sentinel — must always be filtered.
            let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
            sentinel.version = Some("0.14.18".into());
            r.register(sentinel).unwrap();

            // The standalone server's "unknown" row — kept by all_instances
            // so operators can see why connect_to_dcc cannot route to it.
            let unknown = ServiceEntry::new("unknown", "127.0.0.1", 18900);
            r.register(unknown).unwrap();

            // A live Maya plugin.
            let maya = ServiceEntry::new("maya", "127.0.0.1", 18812);
            r.register(maya).unwrap();

            // A stale Maya plugin (heartbeat 10 minutes ago).
            let mut stale = ServiceEntry::new("maya", "127.0.0.1", 18813);
            stale.last_heartbeat = std::time::SystemTime::now() - Duration::from_secs(600);
            r.register(stale).unwrap();
        }

        let gs =
            test_gateway_state_with_own_and_unknown(registry.clone(), "127.0.0.1", 9765, false);
        let all = gs.all_instances(&*registry.read().await);

        assert_eq!(
            all.len(),
            3,
            "expected unknown + live maya + stale maya, got {all:?}"
        );
        assert!(
            !all.iter().any(|e| e.dcc_type == GATEWAY_SENTINEL_DCC_TYPE),
            "gateway sentinel must always be filtered from operator output"
        );
        assert!(
            all.iter().any(|e| e.dcc_type == "unknown"),
            "unknown row must be retained even when allow_unknown_tools is false"
        );
        assert!(
            all.iter().any(|e| e.is_stale(gs.stale_timeout)),
            "stale row must be retained for diagnostics"
        );
    }

    /// Issue maya#138: `entry_to_json` reports `status: "stale"` once a
    /// row has aged past `stale_timeout`, regardless of the original
    /// `ServiceStatus`, so callers can branch without a separate field.
    #[test]
    fn test_entry_to_json_status_stale_for_aged_row() {
        let mut e = ServiceEntry::new("maya", "127.0.0.1", 18812);
        e.last_heartbeat = std::time::SystemTime::now() - Duration::from_secs(600);
        let json = entry_to_json(&e, Duration::from_secs(30));
        assert_eq!(json["status"].as_str(), Some("stale"));
        assert_eq!(json["stale"].as_bool(), Some(true));
    }

    #[test]
    fn test_entry_to_json_includes_pool_state() {
        let mut e = ServiceEntry::new("maya", "127.0.0.1", 18812).with_capacity(2);
        e.acquire_lease(
            "workflow-1",
            Some("job-1".to_string()),
            Some(std::time::SystemTime::now() + Duration::from_secs(60)),
        );

        let json = entry_to_json(&e, Duration::from_secs(30));

        assert_eq!(json["status"].as_str(), Some("busy"));
        assert_eq!(json["pool"]["capacity"].as_u64(), Some(2));
        assert_eq!(json["pool"]["lease_owner"].as_str(), Some("workflow-1"));
        assert_eq!(json["pool"]["current_job_id"].as_str(), Some("job-1"));
        assert_eq!(json["pool"]["available"].as_bool(), Some(false));
        assert!(json["pool"]["lease_expires_at"].as_u64().is_some());
    }

    // ── Issue #719: read_alive_instances ───────────────────────────────────

    /// A row whose PID points at a live process survives the prune; a row
    /// whose owning process has exited (simulated by dropping a separate
    /// `FileRegistry` handle) is evicted — even if its heartbeat was
    /// freshly written. Dead rows also disappear from the on-disk
    /// `services.json`, not just from the returned slice.
    #[tokio::test]
    async fn test_read_alive_instances_prunes_dead_pid() {
        let dir = tempfile::tempdir().unwrap();

        // Reader handle represents the gateway process — keeps the
        // `live` row's sentinel lock held for the duration of the test.
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

        let live_id;
        {
            let r = registry.read().await;
            let mut live = ServiceEntry::new("maya", "127.0.0.1", 18812);
            live.pid = Some(std::process::id());
            live_id = live.instance_id;
            r.register(live).unwrap();
        }

        // Separate "writer" handle simulates a crashed DCC process: it
        // registers a row, then its `FileRegistry` is dropped which
        // releases the sentinel lock and leaves a ghost row on disk
        // for the reader handle to find.
        let dead_id = {
            let writer = FileRegistry::new(dir.path()).unwrap();
            let mut dead = ServiceEntry::new("blender", "127.0.0.1", 18813);
            dead.pid = Some(u32::MAX - 1);
            let dead_id = dead.instance_id;
            writer.register(dead).unwrap();
            dead_id
            // `writer` dropped here → its sentinel lock is released.
        };

        let gs = test_gateway_state(registry.clone());
        let (alive, evicted) = gs
            .read_alive_instances(&*registry.read().await)
            .expect("read_alive_instances must succeed");

        assert_eq!(evicted, 1, "exactly one dead row must be evicted");
        assert_eq!(alive.len(), 1, "only the live row survives");
        assert_eq!(alive[0].instance_id, live_id);
        assert_ne!(alive[0].instance_id, dead_id);

        // The dead row must also be gone from services.json — not just
        // filtered out of the returned slice.
        let raw = gs.all_instances(&*registry.read().await);
        assert!(
            raw.iter().all(|e| e.instance_id != dead_id),
            "dead row must be purged from the on-disk registry after read_alive_instances",
        );
    }

    /// Fail-open guard (#227): a row without a `pid` is assumed alive and
    /// must survive the prune — older registrations predate the pid field.
    #[tokio::test]
    async fn test_read_alive_instances_keeps_rows_without_pid() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

        {
            let r = registry.read().await;
            // `ServiceEntry::new` defaults pid to the current process; null
            // it out to simulate a legacy registration that predates the
            // pid field.
            let mut legacy = ServiceEntry::new("photoshop", "127.0.0.1", 18814);
            legacy.pid = None;
            r.register(legacy).unwrap();
        }

        let gs = test_gateway_state(registry.clone());
        let (alive, evicted) = gs
            .read_alive_instances(&*registry.read().await)
            .expect("read_alive_instances must succeed");

        assert_eq!(evicted, 0);
        assert_eq!(alive.len(), 1, "pid-less rows must survive (#227 contract)");
        assert_eq!(alive[0].dcc_type, "photoshop");
        assert!(
            alive[0].pid.is_none(),
            "pid must remain null after read_alive"
        );
    }

    /// Regression guard for maya#138 and #419: the PID-pruned path must
    /// still filter out the bookkeeping `__gateway__` sentinel and the
    /// gateway's own self-row. Otherwise a gateway that crashed and
    /// re-bound would briefly expose its own sentinel to agents.
    #[tokio::test]
    async fn test_read_alive_instances_filters_sentinel_and_self() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

        {
            let r = registry.read().await;

            // Sentinel row — carries the current pid (looks alive) but
            // must still be excluded.
            let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
            sentinel.pid = Some(std::process::id());
            r.register(sentinel).unwrap();

            // Gateway's own plain-instance row (same host/port as the
            // facade under test).
            let mut self_row = ServiceEntry::new("maya", "127.0.0.1", 9765);
            self_row.pid = Some(std::process::id());
            r.register(self_row).unwrap();

            // A real, non-self Maya instance on another port — must
            // survive.
            let mut other = ServiceEntry::new("maya", "127.0.0.1", 18815);
            other.pid = Some(std::process::id());
            r.register(other).unwrap();
        }

        let gs = test_gateway_state_with_own(registry.clone(), "127.0.0.1", 9765);
        let (alive, evicted) = gs
            .read_alive_instances(&*registry.read().await)
            .expect("read_alive_instances must succeed");

        assert_eq!(evicted, 0, "no rows were dead; nothing should be evicted");
        assert_eq!(
            alive.len(),
            1,
            "only the non-self non-sentinel maya row should remain; got {alive:#?}",
        );
        assert_eq!(alive[0].port, 18815);
        assert!(
            !alive
                .iter()
                .any(|e| e.dcc_type == GATEWAY_SENTINEL_DCC_TYPE),
            "sentinel must never appear in read_alive_instances output",
        );
    }
}
