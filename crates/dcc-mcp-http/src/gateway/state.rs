//! Shared gateway state and helpers.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceStatus};

/// Shared state passed to every gateway axum handler.
#[derive(Clone)]
pub struct GatewayState {
    pub registry: Arc<RwLock<FileRegistry>>,
    pub stale_timeout: Duration,
    pub server_name: String,
    /// The version string of this gateway instance (e.g. `"0.12.29"`).
    pub server_version: String,
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
}

impl GatewayState {
    /// Return all instances that are live (not stale, not shutting down/unreachable).
    ///
    /// The `__gateway__` sentinel row is **always** excluded — it is bookkeeping
    /// for gateway election, not an addressable DCC instance.  Including it in
    /// user-facing tool output (`list_dcc_instances`, `get_dcc_instance`,
    /// `connect_to_dcc`) would confuse agents and break the `mcp_url` contract
    /// (the sentinel's host:port points at the gateway facade, not a real DCC).
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
        let (yield_tx, _) = watch::channel(false);
        let (events_tx, _) = broadcast::channel::<String>(8);
        GatewayState {
            registry: reg,
            stale_timeout: Duration::from_secs(30),
            server_name: "test".into(),
            server_version: "0.13.2".into(),
            http_client: reqwest::Client::new(),
            yield_tx: Arc::new(yield_tx),
            events_tx: Arc::new(events_tx),
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
            sentinel.version = Some("0.13.2".into());
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
}
