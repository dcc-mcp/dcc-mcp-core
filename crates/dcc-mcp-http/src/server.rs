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
/// Build the [`ResourceRegistry`](crate::resources::ResourceRegistry) for
/// a server.
///
/// When artefact resources are enabled, a content-addressed
/// [`FilesystemArtefactStore`](dcc_mcp_artefact::FilesystemArtefactStore)
/// is anchored at `<registry_dir>/artefacts` (falling back to the OS
/// temp dir when no registry dir is configured). Wiring the Python
/// helpers to the same store is the caller's responsibility — see
/// `dcc_mcp_artefact::python::set_default_store` for that path.
fn build_resource_registry(config: &McpHttpConfig) -> crate::resources::ResourceRegistry {
    if config.enable_artefact_resources {
        let root = config
            .registry_dir
            .clone()
            .unwrap_or_else(std::env::temp_dir)
            .join("dcc-mcp-artefacts");
        match dcc_mcp_artefact::FilesystemArtefactStore::new_in(root) {
            Ok(store) => {
                let shared: dcc_mcp_artefact::SharedArtefactStore = Arc::new(store);
                return crate::resources::ResourceRegistry::new_with_artefact_store(
                    config.enable_resources,
                    true,
                    shared,
                );
            }
            Err(err) => {
                tracing::warn!(
                    %err,
                    "failed to create FilesystemArtefactStore; falling back to in-memory",
                );
            }
        }
    }
    crate::resources::ResourceRegistry::new(
        config.enable_resources,
        config.enable_artefact_resources,
    )
}

pub struct McpHttpServer {
    registry: Arc<ActionRegistry>,
    dispatcher: Arc<ActionDispatcher>,
    catalog: Option<Arc<SkillCatalog>>,
    config: McpHttpConfig,
    executor: Option<DccExecutorHandle>,
    resources: crate::resources::ResourceRegistry,
    prompts: crate::prompts::PromptRegistry,
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
        let resources = build_resource_registry(&config);
        let prompts = crate::prompts::PromptRegistry::new(config.enable_prompts);
        Self {
            registry: registry.clone(),
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
            resources,
            prompts,
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
        let resources = build_resource_registry(&config);
        let prompts = crate::prompts::PromptRegistry::new(config.enable_prompts);
        Self {
            registry,
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
            resources,
            prompts,
        }
    }

    /// Access the MCP Prompts registry for this server (issues #351, #355).
    pub fn prompts(&self) -> &crate::prompts::PromptRegistry {
        &self.prompts
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

        // Periodic gauge updater for Prometheus (issue #331). Driven
        // by the exporter being live so we do not leak a ticker task
        // on servers that did not opt into metrics.
        #[cfg(feature = "prometheus")]
        let prometheus_gauge_ctx = if self.config.enable_prometheus {
            Some((self.registry.clone(), sessions.clone()))
        } else {
            None
        };

        let resources = self.resources.clone();
        let prompts = self.prompts.clone();

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

        // Build the Prometheus exporter when both the Cargo feature and
        // runtime flag are enabled (issue #331). Kept in an Arc so the
        // `/metrics` route and every tool-call handler share one
        // registry.
        #[cfg(feature = "prometheus")]
        let prometheus = if self.config.enable_prometheus {
            let exporter = dcc_mcp_telemetry::PrometheusExporter::new();
            // Seed the gauge for registered tools so scrapers see a
            // meaningful value on the very first scrape.
            exporter.set_registered_tools(self.registry.list_actions(None).len() as i64);
            Some(exporter)
        } else {
            None
        };

        let jobs = build_job_manager(&self.config)?;
        let job_notifier = crate::notifications::JobNotifier::new(
            sessions.clone(),
            self.config.enable_job_notifications,
        );
        // Bridge JobManager transitions onto the notifier (#326).
        let notifier_cb = job_notifier.clone();
        jobs.subscribe(move |event| notifier_cb.on_job_event(event));
        // Issue #328: recover any in-flight rows from a prior run and
        // mark them Interrupted. Emits `$/dcc.jobUpdated` through the
        // just-wired notifier. Errors are logged but not fatal — the
        // server continues with an empty in-process map.
        if jobs.storage().is_some() {
            match jobs.recover_from_storage() {
                Ok(n) if n > 0 => tracing::info!(
                    interrupted_jobs = n,
                    "JobManager recovered pending/running rows from storage and marked them Interrupted"
                ),
                Ok(_) => tracing::debug!("JobManager storage recovery found no in-flight rows"),
                Err(e) => tracing::error!(
                    error = %e,
                    "JobManager storage recovery failed — in-process map stays empty"
                ),
            }
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
            jobs,
            job_notifier,
            resources,
            enable_resources: self.config.enable_resources,
            prompts,
            enable_prompts: self.config.enable_prompts,
            #[cfg(feature = "prometheus")]
            prometheus: prometheus.clone(),
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

        // Prometheus `/metrics` endpoint (issue #331). Mounted on the
        // same router so scrapers share the MCP server's listening
        // port, TLS terminator, and ingress config. The route has its
        // own `MetricsState`, independent of the MCP AppState.
        #[cfg(feature = "prometheus")]
        if let Some(exporter) = prometheus.as_ref() {
            let metrics_state = crate::metrics::MetricsState::new(
                exporter.clone(),
                self.config.prometheus_basic_auth.clone(),
            );
            let metrics_router = Router::new()
                .route("/metrics", routing::get(crate::metrics::handle_metrics))
                .with_state(metrics_state);
            router = router.merge(metrics_router);
            tracing::info!("Prometheus /metrics endpoint enabled");

            // Spawn a low-frequency gauge updater so `active_sessions`
            // and `registered_tools` stay fresh without poking every
            // handler path. 5-second tick is finer than the default
            // Prometheus scrape interval (15 s) yet costs nothing
            // meaningful.
            if let Some((registry, sessions_for_gauge)) = prometheus_gauge_ctx.clone() {
                let exporter_bg = exporter.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                    loop {
                        interval.tick().await;
                        exporter_bg.set_registered_tools(registry.list_actions(None).len() as i64);
                        exporter_bg.set_active_sessions(sessions_for_gauge.count() as i64);
                    }
                });
            }
        }

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

/// Build the [`JobManager`](crate::job::JobManager) for this server,
/// attaching a SQLite-backed [`JobStorage`](crate::job_storage::JobStorage)
/// when `config.job_storage_path` is set.
///
/// Fails fast with [`HttpError::Internal`] when the caller asked for
/// persistence but the `job-persist-sqlite` Cargo feature is not
/// compiled in (issue #328) — we must not silently run without the
/// persistence the deployment expected.
fn build_job_manager(config: &McpHttpConfig) -> HttpResult<Arc<crate::job::JobManager>> {
    match &config.job_storage_path {
        Some(path) => {
            #[cfg(feature = "job-persist-sqlite")]
            {
                let storage = crate::job_storage::SqliteStorage::open(path).map_err(|e| {
                    tracing::error!(error = %e, path = %path.display(), "failed to open SQLite JobStorage");
                    HttpError::Internal(format!(
                        "failed to open SQLite job storage at {}: {e}",
                        path.display()
                    ))
                })?;
                Ok(Arc::new(crate::job::JobManager::with_storage(Arc::new(
                    storage,
                ))))
            }
            #[cfg(not(feature = "job-persist-sqlite"))]
            {
                tracing::error!(
                    path = %path.display(),
                    "McpHttpConfig.job_storage_path is set but the `job-persist-sqlite` \
                     Cargo feature is not enabled"
                );
                Err(HttpError::Internal(format!(
                    "job_storage_path={} requires the `job-persist-sqlite` Cargo feature; \
                     rebuild dcc-mcp-core with --features job-persist-sqlite or clear \
                     job_storage_path",
                    path.display()
                )))
            }
        }
        None => Ok(Arc::new(crate::job::JobManager::new())),
    }
}
