//! Shared gateway state and helpers.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{RwLock, watch};

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};

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
    /// Wrapped in `Arc` so every cloned `GatewayState` shares the same sender.
    pub yield_tx: Arc<watch::Sender<bool>>,
}

impl GatewayState {
    /// Return all instances that are live (not stale, not shutting down/unreachable).
    pub fn live_instances(&self, registry: &FileRegistry) -> Vec<ServiceEntry> {
        registry
            .list_all()
            .into_iter()
            .filter(|e| {
                !e.is_stale(self.stale_timeout)
                    && !matches!(
                        e.status,
                        ServiceStatus::ShuttingDown | ServiceStatus::Unreachable
                    )
            })
            .collect()
    }
}

/// Serialize a `ServiceEntry` to a JSON `Value` suitable for gateway responses.
pub fn entry_to_json(e: &ServiceEntry, stale_timeout: Duration) -> Value {
    json!({
        "instance_id": e.instance_id.to_string(),
        "dcc_type":    e.dcc_type,
        "host":        e.host,
        "port":        e.port,
        "mcp_url":     format!("http://{}:{}/mcp", e.host, e.port),
        "status":      e.status.to_string(),
        "scene":       e.scene,
        "version":     e.version,
        "metadata":    e.metadata,
        "stale":       e.is_stale(stale_timeout),
    })
}
