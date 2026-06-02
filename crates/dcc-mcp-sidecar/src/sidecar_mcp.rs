//! Minimal MCP Streamable-HTTP listener inside the sidecar process.
//!
//! The gateway routes ``tools/call`` requests to a sidecar's MCP URL
//! instead of the per-DCC in-process URL whenever the sidecar is the
//! reachable endpoint for a given DCC instance (RFC #998 Phase 2).
//! This module is the listener that fronts the dispatch path.
//!
//! ## Scope (intentionally minimal)
//!
//! This is **not** a full MCP server. It implements just enough of
//! the Streamable-HTTP protocol that the gateway's `call_tool`
//! routing decision can land:
//!
//! | Method            | Behaviour |
//! |-------------------|-----------|
//! | `initialize`      | Returns a capability envelope advertising `tools: { listChanged: false }` only. |
//! | `tools/call`      | Dispatches via the in-process [`HostRpcClient`]; returns the result envelope verbatim. |
//! | `ping`            | Echo `{}` — needed by some hosts' health probes. |
//! | `notifications/*` | Accepted and discarded (per JSON-RPC, no response when `id` is absent). |
//! | everything else   | `-32601` "method not found". |
//!
//! Discovery (`tools/list`, `resources/read`) is intentionally NOT
//! served here. The gateway is the authoritative discovery surface;
//! the sidecar only handles the dispatch.
//!
//! ## Why a separate file
//!
//! `sidecar.rs` is the binary's lifecycle composition root (CLI,
//! FileRegistry, PPID-watch, HostRpcClient connect). Splitting the
//! HTTP listener out keeps each surface comprehensible — the test
//! contract for one is "the process lifecycle is correct" and for
//! the other is "the wire protocol is correct".

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::routing::{get, post};
use dcc_mcp_host_rpc::HostRpcClient;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, watch};

mod handlers;
#[cfg(test)]
mod tests;
mod trace;

use handlers::{
    handle_health, handle_healthz, handle_mcp_post, handle_v1_healthz, handle_v1_readyz,
};

/// The MCP protocol version this listener speaks back to clients.
/// Pinned as a constant so test assertions cannot drift away from
/// what the gateway expects.
pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

/// `server_name` advertised in the `initialize` response. Stable
/// string so the gateway / admin UI can identify a sidecar-served
/// endpoint at a glance.
pub const SIDECAR_SERVER_NAME: &str = "dcc-mcp-sidecar";

/// Shared HTTP-handler state.
///
/// Held in an `Arc` so axum can clone it freely between requests;
/// the inner `Mutex` serialises access to the [`HostRpcClient`]
/// because most per-DCC transports (Maya `commandPort`,
/// Houdini `hrpyc`, …) are inherently single-flight.
#[derive(Clone)]
pub struct SidecarMcpState {
    pub(crate) host_rpc: Arc<Mutex<Box<dyn HostRpcClient>>>,
    pub(crate) server_version: String,
}

impl SidecarMcpState {
    /// Wrap a [`HostRpcClient`] for the HTTP handler.
    ///
    /// The common path passes a connected client. Startup-failure diagnostics
    /// may pass an unavailable client so `/v1/readyz` and `tools/call` can
    /// report structured failure details through the same listener.
    pub fn new(host_rpc: Box<dyn HostRpcClient>, server_version: impl Into<String>) -> Self {
        Self {
            host_rpc: Arc::new(Mutex::new(host_rpc)),
            server_version: server_version.into(),
        }
    }

    /// Tear down the inner client; useful for test fixtures that
    /// want to assert the close path explicitly. In production the
    /// listener's `shutdown` flow drives the close indirectly when
    /// it drops the last `Arc<Mutex<...>>` reference.
    #[allow(dead_code)]
    pub async fn close(&self) {
        let guard = self.host_rpc.lock().await;
        guard.close().await;
    }

    /// Replace the dispatcher behind the already-bound MCP listener.
    ///
    /// Used by the sidecar lifecycle when the DCC host RPC endpoint appears
    /// after the sidecar has already started with a diagnostic placeholder.
    pub async fn replace_host_rpc(&self, host_rpc: Box<dyn HostRpcClient>) {
        let mut guard = self.host_rpc.lock().await;
        guard.close().await;
        *guard = host_rpc;
    }
}

/// Handle returned by [`spawn_listener`].
///
/// Owns the resolved bind address (so the caller can stamp it into
/// the FileRegistry row) and a `watch` channel that, when set to
/// `()`, signals the axum server to shut down gracefully.
pub struct SidecarMcpListenerHandle {
    pub bind_addr: SocketAddr,
    pub mcp_url: String,
    pub join: tokio::task::JoinHandle<()>,
    pub shutdown_tx: watch::Sender<()>,
}

impl SidecarMcpListenerHandle {
    /// Trigger graceful shutdown and wait for the listener task to
    /// finish (with a hard timeout so a stuck axum cannot block the
    /// sidecar's main exit path forever).
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        if (tokio::time::timeout(Duration::from_secs(5), self.join).await).is_err() {
            tracing::warn!(
                bind_addr = %self.bind_addr,
                "sidecar MCP listener did not exit within 5s; abandoning"
            );
        }
    }
}

/// Bind an MCP HTTP listener on `host:port` (`port = 0` ⇒ OS-assigned)
/// and start serving in the background.
///
/// Returns once the listener is **proven accepting** — the
/// `TcpListener::bind` succeeded and the local address has been
/// resolved. Errors at this stage are returned synchronously so the
/// sidecar's run loop can decide whether to fall back (e.g. retry
/// on a different port) or abort.
pub async fn spawn_listener(
    state: SidecarMcpState,
    host: &str,
    port: u16,
) -> anyhow::Result<SidecarMcpListenerHandle> {
    let listener = TcpListener::bind((host, port))
        .await
        .map_err(|e| anyhow::anyhow!("sidecar MCP bind {host}:{port}: {e}"))?;
    let bind_addr = listener.local_addr()?;
    let mcp_url = format!("http://{}/mcp", bind_addr);

    let router = Router::new()
        .route("/mcp", post(handle_mcp_post))
        .route("/health", get(handle_health))
        .route("/healthz", get(handle_healthz))
        .route("/v1/healthz", get(handle_v1_healthz))
        .route("/v1/readyz", get(handle_v1_readyz))
        .with_state(state);

    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let mut shutdown_rx_for_task = shutdown_rx.clone();
    // Mark the seeded value as already-read so the first `.changed()`
    // only fires when the caller actually invokes ``shutdown_tx.send``.
    shutdown_rx_for_task.borrow_and_update();

    let join = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx_for_task.changed().await;
            })
            .await
        {
            tracing::error!(error = %e, "sidecar MCP listener exited with error");
        }
    });

    Ok(SidecarMcpListenerHandle {
        bind_addr,
        mcp_url,
        join,
        shutdown_tx,
    })
}
