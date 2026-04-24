//! The main `McpHttpServer` type.

use axum::{Router, routing};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::{net::TcpListener, sync::watch, task::JoinHandle};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::{
    config::McpHttpConfig,
    error::{HttpError, HttpResult},
    executor::DccExecutorHandle,
    handler::{AppState, handle_delete, handle_get, handle_post},
    inflight::InFlightRequests,
    session::SessionManager,
};
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_skills::SkillCatalog;

#[path = "server_background.rs"]
mod background_impl;
#[path = "server_gateway.rs"]
mod gateway_impl;
#[path = "server_spawn.rs"]
mod spawn_impl;

/// Live DCC instance metadata that is propagated to `FileRegistry` on every
/// heartbeat tick so that `list_dcc_instances` always shows current state.
///
/// Works for both single-document DCCs (Maya, Blender — only `scene` changes)
/// and multi-document DCCs (Photoshop, After Effects — also `documents` list).
#[derive(Debug, Clone, Default)]
pub struct LiveMetaInner {
    /// Currently active/focused scene or document path.
    pub scene: Option<String>,
    /// DCC application version string.
    pub version: Option<String>,
    /// All open documents (multi-document DCCs like Photoshop).
    /// Empty = no document-list update; use `scene` only.
    pub documents: Vec<String>,
    /// Human-readable instance label shown in disambiguation (e.g. `"PS-Marketing"`).
    pub display_name: Option<String>,
}

pub(crate) type LiveMeta = Arc<RwLock<LiveMetaInner>>;

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
    /// Live scene/version that is sync'd to FileRegistry on every heartbeat.
    /// Updated via [`McpHttpServer::update_live_scene`].
    live_meta: LiveMeta,
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
        let live_meta: LiveMeta = Arc::new(RwLock::new(LiveMetaInner {
            scene: config.scene.clone(),
            version: config.dcc_version.clone(),
            ..Default::default()
        }));
        Self {
            registry: registry.clone(),
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
            resources,
            prompts,
            live_meta,
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
        let live_meta: LiveMeta = Arc::new(RwLock::new(LiveMetaInner {
            scene: config.scene.clone(),
            version: config.dcc_version.clone(),
            ..Default::default()
        }));
        Self {
            registry,
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
            resources,
            prompts,
            live_meta,
        }
    }

    /// Update the live instance metadata pushed to `FileRegistry` each heartbeat.
    ///
    /// Works for both single-document DCCs (Maya, Blender — pass only `scene`)
    /// and multi-document DCCs (Photoshop, After Effects — also pass `documents`
    /// and optionally `display_name`).
    ///
    /// Pass `None` to leave a field unchanged; pass `Some("")` / `Some(vec![])`
    /// to clear it.  Changes are visible in `list_dcc_instances` within the next
    /// heartbeat interval (default 5 s).
    pub fn update_live_scene(
        &self,
        scene: Option<String>,
        version: Option<String>,
        documents: Option<Vec<String>>,
        display_name: Option<String>,
    ) {
        let mut guard = self.live_meta.write();
        if let Some(s) = scene {
            guard.scene = if s.is_empty() { None } else { Some(s) };
        }
        if let Some(v) = version {
            guard.version = if v.is_empty() { None } else { Some(v) };
        }
        if let Some(docs) = documents {
            guard.documents = docs.into_iter().filter(|d| !d.is_empty()).collect();
        }
        if let Some(name) = display_name {
            guard.display_name = if name.is_empty() { None } else { Some(name) };
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

    /// Replace the live-metadata store with a shared one.
    ///
    /// Use this when the caller needs to retain a handle to the store so it
    /// can push scene/version updates after the server starts (e.g. Python
    /// bindings where `PyMcpHttpServer` must share the same `Arc` with the
    /// returned `PyServerHandle`).
    pub fn with_live_meta(mut self, live_meta: LiveMeta) -> Self {
        self.live_meta = live_meta;
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

        background_impl::spawn_session_eviction_task(&sessions, self.config.session_ttl_secs);

        let cancelled_requests: std::sync::Arc<dashmap::DashMap<String, std::time::Instant>> =
            std::sync::Arc::new(dashmap::DashMap::new());

        background_impl::spawn_cancellation_gc_task(&cancelled_requests);

        // Periodic gauge updater for Prometheus (issue #331). Driven
        // by the exporter being live so we do not leak a ticker task
        // on servers that did not opt into metrics.
        #[cfg(feature = "prometheus")]
        let prometheus_gauge_ctx = background_impl::prometheus_gauge_context(
            &self.config,
            self.registry.clone(),
            sessions.clone(),
        );

        let resources = self.resources.clone();
        let prompts = self.prompts.clone();

        background_impl::spawn_resource_update_forwarder(
            self.config.enable_resources,
            &resources,
            &sessions,
        );

        // Build the Prometheus exporter when both the Cargo feature and
        // runtime flag are enabled (issue #331). Kept in an Arc so the
        // `/metrics` route and every tool-call handler share one
        // registry.
        #[cfg(feature = "prometheus")]
        let prometheus =
            background_impl::build_prometheus_exporter(&self.config, self.registry.as_ref());

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
            declared_capabilities: std::sync::Arc::new(self.config.declared_capabilities.clone()),
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
        {
            router = background_impl::attach_metrics_route(
                router,
                &prometheus,
                &self.config,
                prometheus_gauge_ctx.clone(),
            );
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
        let gateway_handle =
            gateway_impl::start_gateway_runner(&self.config, port, &self.live_meta).await;

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
        let (join, serve_thread) = spawn_impl::spawn_http_server(
            listener,
            router,
            &self.config,
            actual_bind.clone(),
            port,
            shutdown_tx.clone(),
            shutdown_rx,
        )
        .await?;

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
