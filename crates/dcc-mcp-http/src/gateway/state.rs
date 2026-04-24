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
                        ServiceStatus::ShuttingDown | ServiceStatus::Unreachable
                    )
                    && !super::is_own_instance(e, &self.own_host, self.own_port)
            })
            .collect()
    }
}

/// Serialize a `ServiceEntry` to a JSON `Value` suitable for gateway responses.
///
/// The returned object always contains every field so that MCP clients can
/// display a rich disambiguation prompt when multiple instances of the same
/// DCC type are live at the same time.
pub fn entry_to_json(e: &ServiceEntry, stale_timeout: Duration) -> Value {
    json!({
        "instance_id":  e.instance_id.to_string(),
        "dcc_type":     e.dcc_type,
        "host":         e.host,
        "port":         e.port,
        "mcp_url":      format!("http://{}:{}/mcp", e.host, e.port),
        "status":       e.status.to_string(),
        // ── document / scene ───────────────────────────────────────────────
        // `scene` is the active / primary document (same field as before).
        // `documents` is the full list for multi-document apps (Photoshop etc.).
        "scene":        e.scene,
        "documents":    e.documents,
        // ── disambiguation helpers ─────────────────────────────────────────
        // Both fields are null when not set; agents should skip null values
        // when building the disambiguation prompt for users.
        "pid":          e.pid,
        "display_name": e.display_name,
        // ── misc ───────────────────────────────────────────────────────────
        "version":      e.version,
        "metadata":     e.metadata,
        "stale":        e.is_stale(stale_timeout),
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
}
