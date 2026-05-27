//! Admin HTTP surface — `/tunnels` listing + `/healthz` probe.
//!
//! Bound on a separate port from the agent + frontend listeners so an
//! operator can firewall it independently (e.g. private VLAN-only).
//! All endpoints are read-only — there is no mutation surface here. The
//! relay's lifecycle is owned by the operator's process supervisor.
//!
//! ### `GET /tunnels`
//!
//! Returns a JSON array of [`TunnelSummary`] rows, one per live tunnel.
//! Wall-clock fields (`registered_at_ms_ago`, `last_heartbeat_ms_ago`)
//! are computed at response time, not on insert, so the snapshot reflects
//! the relay's view *now*.
//!
//! ### `GET /healthz`
//!
//! Returns `200 OK` with body `"ok"` when the relay process is up. Future
//! readiness probes (e.g. JWT secret rotated, registry capacity) can be
//! added here without breaking the existing contract.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::{info, warn};

use crate::registry::TunnelRegistry;

/// One row of the `/tunnels` listing response. Snake-case to match the
/// rest of the wire surface (`auth.rs`, `frame.rs`).
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelSummary {
    /// Stable per-tunnel id minted at registration.
    pub tunnel_id: String,

    /// Stable DCC-MCP instance id represented by this tunnel.
    pub instance_id: String,

    /// DCC tag the agent declared in its `RegisterRequest`.
    pub dcc: String,

    /// Explicit DCC type alias for gateway discovery.
    pub dcc_type: String,

    /// Capability tags the agent advertised. Verbatim — the relay does
    /// not validate these against any registry.
    pub capabilities: Vec<String>,

    /// Optional opaque capability fingerprint.
    pub capabilities_fingerprint: Option<String>,

    /// Optional adapter package version.
    pub adapter_version: Option<String>,

    /// Optional active scene/document.
    pub scene: Option<String>,

    /// Build identifier the agent reported.
    pub agent_version: String,

    /// Public frontend URL assigned to this tunnel.
    pub public_url: String,

    /// Milliseconds since the tunnel was first accepted. Useful as an
    /// "uptime" column in operator dashboards.
    pub registered_at_ms_ago: u128,

    /// Milliseconds since the last `Heartbeat` frame. The relay's
    /// `stale_timeout` setting is the eviction cutoff.
    pub last_heartbeat_ms_ago: u128,

    /// Currently-multiplexed sessions on this tunnel.
    pub session_count: usize,
}

/// Build the admin axum router. Exposed for tests so they can mount it
/// in-process without binding a real port.
pub fn router(registry: Arc<TunnelRegistry>) -> Router {
    Router::new()
        .route("/tunnels", get(list_tunnels))
        .route("/healthz", get(healthz))
        .with_state(registry)
}

async fn list_tunnels(State(reg): State<Arc<TunnelRegistry>>) -> impl IntoResponse {
    let now = std::time::Instant::now();
    let summaries: Vec<TunnelSummary> = reg
        .iter()
        .map(|e| {
            let v = e.value();
            TunnelSummary {
                tunnel_id: v.tunnel_id.clone(),
                instance_id: v.instance_id.clone(),
                dcc: v.dcc.clone(),
                dcc_type: v.dcc_type.clone(),
                capabilities: v.capabilities.clone(),
                capabilities_fingerprint: v.capabilities_fingerprint.clone(),
                adapter_version: v.adapter_version.clone(),
                scene: v.scene.clone(),
                agent_version: v.agent_version.clone(),
                public_url: v.public_url.clone(),
                registered_at_ms_ago: now.saturating_duration_since(v.registered_at).as_millis(),
                last_heartbeat_ms_ago: now.saturating_duration_since(v.last_seen()).as_millis(),
                session_count: v.handle.session_count(),
            }
        })
        .collect();
    (StatusCode::OK, Json(summaries))
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// Bind the admin router on `bind` and serve forever. Returns the
/// resolved socket address so the caller can advertise it (e.g. in
/// startup logs or tests).
pub async fn serve(
    bind: SocketAddr,
    registry: Arc<TunnelRegistry>,
) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind(bind).await?;
    let addr = listener.local_addr()?;
    info!(%addr, "tunnel relay admin endpoint listening");
    let app = router(registry);
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            warn!(error = %e, "admin server exited");
        }
    });
    Ok((addr, task))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    use parking_lot::RwLock;

    use crate::handle::TunnelHandle;
    use crate::registry::TunnelEntry;

    fn make_entry(id: &str, dcc: &str) -> TunnelEntry {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        TunnelEntry {
            tunnel_id: id.into(),
            instance_id: id.into(),
            dcc: dcc.into(),
            dcc_type: dcc.into(),
            capabilities: vec!["scene.read".into()],
            capabilities_fingerprint: Some("fp-1".into()),
            adapter_version: Some("test-adapter/1.0".into()),
            scene: Some("shot.usd".into()),
            agent_version: "test/0.0".into(),
            public_url: format!("ws://relay.example/tunnel/{id}"),
            registered_at: Instant::now(),
            last_heartbeat: RwLock::new(Instant::now()),
            handle: Arc::new(TunnelHandle::new(tx)),
        }
    }

    #[tokio::test]
    async fn list_tunnels_returns_one_row_per_entry() {
        let reg = Arc::new(TunnelRegistry::new());
        reg.insert(make_entry("t1", "maya"));
        reg.insert(make_entry("t2", "houdini"));

        let resp = list_tunnels(State(Arc::clone(&reg))).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: Vec<TunnelSummary> = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(
            parsed
                .iter()
                .any(|s| s.tunnel_id == "t1" && s.dcc == "maya")
        );
        assert!(parsed.iter().any(|s| {
            s.tunnel_id == "t1"
                && s.instance_id == "t1"
                && s.dcc_type == "maya"
                && s.capabilities_fingerprint.as_deref() == Some("fp-1")
                && s.adapter_version.as_deref() == Some("test-adapter/1.0")
                && s.scene.as_deref() == Some("shot.usd")
                && s.public_url == "ws://relay.example/tunnel/t1"
        }));
        assert!(
            parsed
                .iter()
                .any(|s| s.tunnel_id == "t2" && s.dcc == "houdini")
        );
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let resp = healthz().await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
