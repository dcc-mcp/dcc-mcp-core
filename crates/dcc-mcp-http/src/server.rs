//! The main `McpHttpServer` type.

use axum::{Router, routing};
use std::sync::Arc;
use tokio::{net::TcpListener, sync::watch, task::JoinHandle};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::{
    config::McpHttpConfig,
    error::{HttpError, HttpResult},
    executor::DccExecutorHandle,
    handler::{AppState, handle_delete, handle_get, handle_post},
    session::SessionManager,
};
use dcc_mcp_actions::ActionRegistry;

/// Handle returned by [`McpHttpServer::start`].
///
/// Drop or call [`ServerHandle::shutdown`] to stop the server.
pub struct ServerHandle {
    shutdown_tx: watch::Sender<bool>,
    join: JoinHandle<()>,
    /// Actual port the server is listening on (useful when port=0).
    pub port: u16,
    pub bind_addr: String,
}

impl ServerHandle {
    /// Gracefully shut down the server and wait for it to stop.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.join.await;
    }

    /// Signal shutdown without waiting.
    pub fn signal_shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

/// MCP Streamable HTTP server.
///
/// Embeds an axum HTTP server running on a dedicated Tokio runtime thread.
/// Safe to use from DCC main threads — the server never blocks the caller.
pub struct McpHttpServer {
    registry: Arc<ActionRegistry>,
    config: McpHttpConfig,
    executor: Option<DccExecutorHandle>,
}

impl McpHttpServer {
    /// Create a new server with the given registry and config.
    pub fn new(registry: Arc<ActionRegistry>, config: McpHttpConfig) -> Self {
        Self {
            registry,
            config,
            executor: None,
        }
    }

    /// Attach a DCC main-thread executor for thread-safe DCC API calls.
    pub fn with_executor(mut self, executor: DccExecutorHandle) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Start the HTTP server in a background Tokio task.
    ///
    /// Returns a [`ServerHandle`] for controlling the server lifecycle.
    /// This method is `async` but returns immediately after binding the port.
    pub async fn start(self) -> HttpResult<ServerHandle> {
        let state = AppState {
            registry: self.registry,
            sessions: SessionManager::new(),
            executor: self.executor,
            server_name: self.config.server_name.clone(),
            server_version: self.config.server_version.clone(),
        };

        let endpoint = self.config.endpoint_path.clone();

        let mut router = Router::new()
            .route(
                &endpoint,
                routing::post(handle_post)
                    .get(handle_get)
                    .delete(handle_delete),
            )
            .with_state(state)
            .layer(TraceLayer::new_for_http());

        if self.config.enable_cors {
            router = router.layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods(Any)
                    .allow_headers(Any),
            );
        }

        let bind_addr = self.config.bind_addr();
        let listener = TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| HttpError::BindFailed {
                addr: bind_addr.clone(),
                source: e,
            })?;

        let actual_addr = listener.local_addr().map_err(|e| HttpError::BindFailed {
            addr: bind_addr.clone(),
            source: e,
        })?;

        let port = actual_addr.port();
        let actual_bind = actual_addr.to_string();

        tracing::info!(
            "MCP HTTP server listening on http://{actual_bind}{}",
            self.config.endpoint_path
        );

        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        let join = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    loop {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                        if shutdown_rx.changed().await.is_err() {
                            break;
                        }
                    }
                })
                .await
                .ok();
            tracing::info!("MCP HTTP server stopped");
        });

        Ok(ServerHandle {
            shutdown_tx,
            join,
            port,
            bind_addr: actual_bind,
        })
    }
}

/// Convenience: start a server from the current Tokio runtime context.
///
/// Useful when embedding in Python via `block_on`.
pub fn start_in_runtime(
    runtime: &tokio::runtime::Runtime,
    registry: Arc<ActionRegistry>,
    config: McpHttpConfig,
) -> HttpResult<ServerHandle> {
    runtime.block_on(async { McpHttpServer::new(registry, config).start().await })
}
