//! The main `McpHttpServer` type.

use axum::{Router, routing};
use std::sync::Arc;
use tokio::{net::TcpListener, sync::watch, task::JoinHandle};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::{
    config::{McpHttpConfig, ServerSpawnMode},
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
///
/// In [`ServerSpawnMode::Dedicated`] mode the listener runs on a dedicated
/// OS thread that owns a `current_thread` Tokio runtime — [`Self::serve_thread`]
/// holds that thread's join handle. This fixes issue #303 by preventing the
/// listener's accept loop from being starved under PyO3-embedded hosts.
pub struct McpServerHandle {
    shutdown_tx: watch::Sender<bool>,
    /// JoinHandle for the serve task when running in
    /// [`ServerSpawnMode::Ambient`] mode.
    join: Option<JoinHandle<()>>,
    /// OS thread JoinHandle when running in [`ServerSpawnMode::Dedicated`]
    /// mode. The thread owns a `current_thread` runtime and drives the
    /// serve future directly — guaranteed not to be starved (issue #303).
    serve_thread: Option<std::thread::JoinHandle<()>>,
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
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
        // For Dedicated mode, the serve_thread observes shutdown_tx and
        // exits on its own; we do not block on it here because callers
        // frequently invoke shutdown() from a tokio runtime and joining
        // an OS thread there would deadlock. Drop handles cleanup.
        drop(self.serve_thread.take());
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
    resources: crate::resources::ResourceRegistry,
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
        let resources = crate::resources::ResourceRegistry::new(
            config.enable_resources,
            config.enable_artefact_resources,
        );
        Self {
            registry: registry.clone(),
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
            resources,
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
        let resources = crate::resources::ResourceRegistry::new(
            config.enable_resources,
            config.enable_artefact_resources,
        );
        Self {
            registry,
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
            resources,
        }
    }

    /// Access the MCP Resources registry for this server (issue #350).
    ///
    /// Register additional [`crate::ResourceProducer`] implementations or
    /// publish a scene snapshot via
    /// [`crate::ResourceRegistry::set_scene`] **before** calling
    /// [`Self::start`]. The registry is shared with the running server —
    /// producers registered here are reflected in `resources/list` and
    /// `resources/read`.
    pub fn resources(&self) -> &crate::resources::ResourceRegistry {
        &self.resources
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

        let resources = self.resources.clone();

        // Forward `notifications/resources/updated` broadcasts to each
        // subscribed session's SSE channel (issue #350).
        if self.config.enable_resources {
            let resources_bg = resources.clone();
            let sessions_bg = sessions.clone();
            tokio::spawn(async move {
                let mut rx = resources_bg.watch_updates();
                while let Ok(uri) = rx.recv().await {
                    let notification = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/resources/updated",
                        "params": { "uri": uri }
                    });
                    let event = crate::protocol::format_sse_event(&notification, None);
                    for sid in resources_bg.sessions_subscribed_to(&uri) {
                        sessions_bg.push_event(&sid, event.clone());
                    }
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
            pending_elicitations: std::sync::Arc::new(dashmap::DashMap::new()),
            lazy_actions: self.config.lazy_actions,
            bare_tool_names: self.config.bare_tool_names,
            jobs: std::sync::Arc::new(crate::job::JobManager::new()),
            resources,
            enable_resources: self.config.enable_resources,
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
                backend_timeout_ms: self.config.backend_timeout_ms,
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

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // ── Spawn strategy (issue #303) ──────────────────────────────────────
        //
        // `Ambient`   — run `axum::serve` as a tokio::spawn task. Correct for
        //               standalone binaries (`#[tokio::main]`) where a driver
        //               thread is guaranteed to outlive the server.
        // `Dedicated` — run `axum::serve` inside a `current_thread` runtime on
        //               its own OS thread. The thread owns the runtime and
        //               blocks on the serve future, so the accept loop cannot
        //               be starved even if the parent runtime's workers go
        //               idle (Maya on Windows / PyO3-embedded hosts).
        let (join, serve_thread) = match self.config.spawn_mode {
            ServerSpawnMode::Ambient => {
                let mut shutdown_rx_a = shutdown_rx.clone();
                let join = tokio::spawn(async move {
                    axum::serve(listener, router)
                        .with_graceful_shutdown(async move {
                            loop {
                                if *shutdown_rx_a.borrow() {
                                    break;
                                }
                                if shutdown_rx_a.changed().await.is_err() {
                                    break;
                                }
                            }
                        })
                        .await
                        .ok();
                    tracing::info!("MCP HTTP server stopped");
                });
                // Self-probe: confirm the accept loop is actually running
                // before we return a handle that claims to be bound.
                if self.config.self_probe_timeout_ms > 0 {
                    let probe_host = if self.config.host.is_unspecified() {
                        "127.0.0.1".to_string()
                    } else {
                        self.config.host.to_string()
                    };
                    let probe_addr = format!("{probe_host}:{port}");
                    let timeout =
                        std::time::Duration::from_millis(self.config.self_probe_timeout_ms);
                    let mut reachable = false;
                    for _ in 0..5 {
                        match tokio::time::timeout(
                            timeout,
                            tokio::net::TcpStream::connect(&probe_addr),
                        )
                        .await
                        {
                            Ok(Ok(_)) => {
                                reachable = true;
                                break;
                            }
                            _ => {
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            }
                        }
                    }
                    if !reachable {
                        let _ = shutdown_tx.send(true);
                        let _ = join.await;
                        return Err(HttpError::BindFailed {
                            addr: actual_bind.clone(),
                            source: std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "instance listener self-probe failed (issue #303 guard)",
                            ),
                        });
                    }
                }
                (Some(join), None)
            }
            ServerSpawnMode::Dedicated => {
                // Re-bind the port inside the dedicated thread's runtime —
                // a TcpListener is pinned to the runtime it was created on.
                // Safely hand off: drop the existing listener, bind again
                // on the new runtime. Because we use SO_REUSEADDR=false
                // elsewhere, we briefly close the port here; that's safe
                // because we still hold exclusive ownership of the port's
                // "intent to bind" (`port` is already allocated from `0`).
                let rebind_addr = actual_bind.clone();
                drop(listener);

                let (ready_tx, ready_rx) =
                    std::sync::mpsc::sync_channel::<Result<(), std::io::Error>>(1);
                let mut shutdown_rx_d = shutdown_rx.clone();
                let self_probe_timeout_ms = self.config.self_probe_timeout_ms;
                let probe_bind = actual_bind.clone();

                let thread = std::thread::Builder::new()
                    .name(format!("dcc-mcp-http-{}", port))
                    .spawn(move || {
                        let rt = match tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                        {
                            Ok(rt) => rt,
                            Err(e) => {
                                let _ = ready_tx.send(Err(std::io::Error::other(format!(
                                    "failed to build dedicated runtime: {e}"
                                ))));
                                return;
                            }
                        };
                        rt.block_on(async move {
                            let listener = match TcpListener::bind(&rebind_addr).await {
                                Ok(l) => l,
                                Err(e) => {
                                    let _ = ready_tx.send(Err(e));
                                    return;
                                }
                            };
                            // Signal ready: the listener is bound and
                            // accept() will run on the next poll.
                            let _ = ready_tx.send(Ok(()));
                            axum::serve(listener, router)
                                .with_graceful_shutdown(async move {
                                    loop {
                                        if *shutdown_rx_d.borrow() {
                                            break;
                                        }
                                        if shutdown_rx_d.changed().await.is_err() {
                                            break;
                                        }
                                    }
                                })
                                .await
                                .ok();
                            tracing::info!("MCP HTTP server (dedicated) stopped");
                        });
                    })
                    .map_err(|e| HttpError::BindFailed {
                        addr: actual_bind.clone(),
                        source: e,
                    })?;

                // Wait for the thread to signal it has bound the listener.
                match ready_rx.recv_timeout(std::time::Duration::from_secs(10)) {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        return Err(HttpError::BindFailed {
                            addr: actual_bind.clone(),
                            source: e,
                        });
                    }
                    Err(e) => {
                        return Err(HttpError::BindFailed {
                            addr: actual_bind.clone(),
                            source: std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                format!("dedicated thread did not signal readiness: {e}"),
                            ),
                        });
                    }
                }

                // Self-probe from the caller's runtime to confirm the
                // dedicated thread's accept loop is actually serving.
                if self_probe_timeout_ms > 0 {
                    let timeout = std::time::Duration::from_millis(self_probe_timeout_ms);
                    let mut reachable = false;
                    for _ in 0..5 {
                        match tokio::time::timeout(
                            timeout,
                            tokio::net::TcpStream::connect(&probe_bind),
                        )
                        .await
                        {
                            Ok(Ok(_)) => {
                                reachable = true;
                                break;
                            }
                            _ => {
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            }
                        }
                    }
                    if !reachable {
                        let _ = shutdown_tx.send(true);
                        return Err(HttpError::BindFailed {
                            addr: actual_bind.clone(),
                            source: std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "dedicated listener self-probe failed (issue #303 guard)",
                            ),
                        });
                    }
                }

                (None, Some(thread))
            }
        };

        Ok(McpServerHandle {
            shutdown_tx,
            join,
            serve_thread,
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
