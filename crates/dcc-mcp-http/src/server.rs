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
    gateway::{GatewayConfig, GatewayRunner},
    handler::{AppState, handle_delete, handle_get, handle_post},
    inflight::InFlightRequests,
    session::SessionManager,
};
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_transport::discovery::types::ServiceEntry;

/// Handle returned by [`McpHttpServer::start`].
///
/// Drop or call [`McpServerHandle::shutdown`] to stop the server.
pub struct McpServerHandle {
    shutdown_tx: watch::Sender<bool>,
    join: JoinHandle<()>,
    /// Actual port the server is listening on (useful when port=0).
    pub port: u16,
    pub bind_addr: String,
    /// `true` if this process won the gateway port competition.
    pub is_gateway: bool,
    // Keep the GatewayHandle alive so background tasks keep running.
    _gateway: Option<crate::gateway::GatewayHandle>,
}

impl McpServerHandle {
    /// Gracefully shut down the server and wait for it to stop.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.join.await;
        // _gateway is dropped here, aborting heartbeat/cleanup tasks
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
    dispatcher: Arc<ActionDispatcher>,
    catalog: Option<Arc<SkillCatalog>>,
    config: McpHttpConfig,
    executor: Option<DccExecutorHandle>,
}

impl McpHttpServer {
    /// Create a new server with the given registry and config.
    ///
    /// A `SkillCatalog` and `ActionDispatcher` are created automatically,
    /// both backed by the same registry. The catalog is pre-wired to the
    /// dispatcher so that `load_skill` auto-registers script handlers.
    pub fn new(registry: Arc<ActionRegistry>, config: McpHttpConfig) -> Self {
        let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            registry.clone(),
            dispatcher.clone(),
        ));
        Self {
            registry: registry.clone(),
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
        }
    }

    /// Create a server with an explicit SkillCatalog.
    pub fn with_catalog(
        registry: Arc<ActionRegistry>,
        catalog: Arc<SkillCatalog>,
        config: McpHttpConfig,
    ) -> Self {
        let dispatcher = catalog
            .dispatcher()
            .cloned()
            .unwrap_or_else(|| Arc::new(ActionDispatcher::new((*registry).clone())));
        Self {
            registry,
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
        }
    }

    /// Get a reference to the server's SkillCatalog (if configured).
    pub fn catalog(&self) -> Option<&Arc<SkillCatalog>> {
        self.catalog.as_ref()
    }

    /// Attach a DCC main-thread executor for thread-safe DCC API calls.
    pub fn with_executor(mut self, executor: DccExecutorHandle) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Attach an [`ActionDispatcher`] with pre-registered handlers.
    ///
    /// Use this when handlers are registered before starting the server.
    /// The dispatcher should be backed by the same [`ActionRegistry`].
    pub fn with_dispatcher(mut self, dispatcher: Arc<ActionDispatcher>) -> Self {
        self.dispatcher = dispatcher;
        self
    }

    /// Start the HTTP server in a background Tokio task.
    ///
    /// Returns a [`McpServerHandle`] for controlling the server lifecycle.
    /// This method is `async` but returns immediately after binding the port.
    ///
    /// When `config.gateway_port > 0`, this method also registers the instance
    /// in the shared `FileRegistry` and attempts to become the gateway via
    /// first-wins TCP port binding.
    pub async fn start(self) -> HttpResult<McpServerHandle> {
        // If no catalog was provided, create a default one
        let catalog = self
            .catalog
            .unwrap_or_else(|| Arc::new(SkillCatalog::new(self.registry.clone())));

        let sessions = SessionManager::new();

        // Spawn background task that evicts idle sessions once per minute.
        if self.config.session_ttl_secs > 0 {
            let sessions_bg = sessions.clone();
            let ttl = std::time::Duration::from_secs(self.config.session_ttl_secs);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    sessions_bg.evict_stale(ttl);
                }
            });
        }

        let cancelled_requests: std::sync::Arc<dashmap::DashMap<String, std::time::Instant>> =
            std::sync::Arc::new(dashmap::DashMap::new());

        // Spawn background task that garbage-collects stale cancellation records.
        //
        // When a client cancels a request that has already completed (common race),
        // the entry in `cancelled_requests` is never consumed by `handle_tools_call`.
        // Without this task the map would grow without bound in long-running servers.
        {
            let cr_bg = cancelled_requests.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    // purge_expired_cancellations uses the same TTL constant defined in handler.rs
                    cr_bg.retain(|_, recorded_at: &mut std::time::Instant| {
                        recorded_at.elapsed() < std::time::Duration::from_secs(30)
                    });
                }
            });
        }

        let state = AppState {
            registry: self.registry,
            dispatcher: self.dispatcher,
            catalog,
            sessions,
            executor: self.executor,
            bridge_registry: crate::BridgeRegistry::new(),
            server_name: self.config.server_name.clone(),
            server_version: self.config.server_version.clone(),
            cancelled_requests,
            in_flight: InFlightRequests::new(),
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

        // ── Optional gateway competition ──────────────────────────────────────
        let gateway_handle = if self.config.gateway_port > 0 {
            let gw_cfg = GatewayConfig {
                host: self.config.host.to_string(),
                gateway_port: self.config.gateway_port,
                stale_timeout_secs: self.config.stale_timeout_secs,
                heartbeat_secs: self.config.heartbeat_secs,
                server_name: self.config.server_name.clone(),
                server_version: self.config.server_version.clone(),
                registry_dir: self.config.registry_dir.clone(),
                challenger_timeout_secs: 120,
            };

            match GatewayRunner::new(gw_cfg) {
                Ok(runner) => {
                    let mut entry = ServiceEntry::new(
                        self.config.dcc_type.as_deref().unwrap_or("unknown"),
                        self.config.host.to_string(),
                        port,
                    );
                    entry.version = self.config.dcc_version.clone();
                    entry.scene = self.config.scene.clone();

                    match runner.start(entry).await {
                        Ok(h) => Some(h),
                        Err(e) => {
                            tracing::warn!("Gateway runner failed to start: {e}");
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to create GatewayRunner: {e}");
                    None
                }
            }
        } else {
            None
        };

        let is_gateway = gateway_handle
            .as_ref()
            .map(|h| h.is_gateway)
            .unwrap_or(false);

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

        Ok(McpServerHandle {
            shutdown_tx,
            join,
            port,
            bind_addr: actual_bind,
            is_gateway,
            _gateway: gateway_handle,
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
) -> HttpResult<McpServerHandle> {
    runtime.block_on(async { McpHttpServer::new(registry, config).start().await })
}
