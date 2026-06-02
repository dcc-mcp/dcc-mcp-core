//! The main `McpHttpServer` type.

use axum::{Json, Router, routing};
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::{net::TcpListener, sync::watch, task::JoinHandle};
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use crate::{
    config::McpHttpConfig,
    error::{HttpError, HttpResult},
    executor::DccExecutorHandle,
    handler::AppState,
    session::SessionManager,
};
use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
use dcc_mcp_skill_rest::ReadinessProbe;
use dcc_mcp_skills::SkillCatalog;

mod background_impl;
#[cfg(feature = "auto-gateway")]
mod gateway_impl;
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
    /// Arbitrary string metadata merged into the FileRegistry row on heartbeat.
    pub metadata: HashMap<String, String>,
}

pub type LiveMeta = Arc<RwLock<LiveMetaInner>>;

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
    ///
    /// Always `false` when the crate is built with
    /// `--no-default-features` (the `auto-gateway` feature is off and
    /// `gateway_port` becomes a no-op).
    pub is_gateway: bool,
    // Keep the GatewayHandle alive so background tasks keep running.
    #[cfg(feature = "auto-gateway")]
    _gateway: Option<dcc_mcp_gateway::GatewayHandle>,
}

impl McpServerHandle {
    /// Gracefully shut down the server and wait for it to stop.
    pub async fn shutdown(mut self) {
        // Issue #718: deregister from FileRegistry *before* waiting for
        // the serve loop to finish. Peers reading `services.json` should
        // see the row disappear as soon as `shutdown()` is invoked rather
        // than waiting the full `stale_timeout_secs` (default 30 s). The
        // call is idempotent — the `GatewayHandle::Drop` path is still a
        // correctness safety net for callers who skip `shutdown()`.
        #[cfg(feature = "auto-gateway")]
        if let Some(gw) = self._gateway.as_mut() {
            gw.deregister_all();
        }

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
pub fn build_resource_registry(config: &McpHttpConfig) -> crate::resources::ResourceRegistry {
    if config.features.enable_artefact_resources {
        let root = config
            .gateway
            .registry_dir
            .clone()
            .unwrap_or_else(std::env::temp_dir)
            .join("dcc-mcp-artefacts");
        match dcc_mcp_artefact::FilesystemArtefactStore::new_in(root) {
            Ok(store) => {
                let shared: dcc_mcp_artefact::SharedArtefactStore = Arc::new(store);
                return crate::resources::ResourceRegistry::new_with_artefact_store(
                    config.features.enable_resources,
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
        config.features.enable_resources,
        config.features.enable_artefact_resources,
    )
}

pub struct McpHttpServer {
    registry: Arc<ToolRegistry>,
    dispatcher: Arc<ToolDispatcher>,
    catalog: Option<Arc<SkillCatalog>>,
    config: McpHttpConfig,
    executor: Option<DccExecutorHandle>,
    /// Tokio runtime that drains the host-bridge mpsc (PyO3 / `dispatcher_to_executor_handle`).
    ///
    /// When set together with [`Self::with_executor`], REST `POST /v1/call` uses
    /// [`dcc_mcp_http_server::ThreadRoutedInvoker`]. Without it, the server still
    /// starts and MCP `tools/call` keeps executor routing; REST falls back to
    /// direct dispatch (issue #1055).
    host_bridge_runtime: Option<tokio::runtime::Handle>,
    resources: crate::resources::ResourceRegistry,
    prompts: crate::prompts::PromptRegistry,
    /// Live scene/version that is sync'd to FileRegistry on every heartbeat.
    /// Updated via [`McpHttpServer::update_live_scene`].
    live_meta: LiveMeta,
    /// Optional shared [`ReadinessProbe`] gating DCC-touching
    /// `tools/call` dispatches (issue #714). When `None`, the server
    /// falls back to [`AppState::default_readiness`] (fully-ready) so
    /// existing embedders keep working unchanged.
    readiness: Option<Arc<dyn ReadinessProbe>>,
}

impl McpHttpServer {
    /// Create a new server with the given registry and config.
    ///
    /// A `SkillCatalog` and `ToolDispatcher` are created automatically,
    /// both backed by the same registry. The catalog is pre-wired to the
    /// dispatcher so that `load_skill` auto-registers script handlers.
    pub fn new(registry: Arc<ToolRegistry>, config: McpHttpConfig) -> Self {
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            registry.clone(),
            dispatcher.clone(),
        ));
        let resources = build_resource_registry(&config);
        let prompts = crate::prompts::PromptRegistry::new(config.features.enable_prompts);
        let live_meta: LiveMeta = Arc::new(RwLock::new(LiveMetaInner {
            scene: config.instance.scene.clone(),
            version: config.instance.dcc_version.clone(),
            ..Default::default()
        }));
        Self {
            registry: registry.clone(),
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
            host_bridge_runtime: None,
            resources,
            prompts,
            live_meta,
            readiness: None,
        }
    }

    /// Create a server with an explicit SkillCatalog.
    pub fn with_catalog(
        registry: Arc<ToolRegistry>,
        catalog: Arc<SkillCatalog>,
        config: McpHttpConfig,
    ) -> Self {
        let dispatcher = catalog
            .dispatcher()
            .cloned()
            .unwrap_or_else(|| Arc::new(ToolDispatcher::new((*registry).clone())));
        let resources = build_resource_registry(&config);
        let prompts = crate::prompts::PromptRegistry::new(config.features.enable_prompts);
        let live_meta: LiveMeta = Arc::new(RwLock::new(LiveMetaInner {
            scene: config.instance.scene.clone(),
            version: config.instance.dcc_version.clone(),
            ..Default::default()
        }));
        Self {
            registry,
            dispatcher,
            catalog: Some(catalog),
            config,
            executor: None,
            host_bridge_runtime: None,
            resources,
            prompts,
            live_meta,
            readiness: None,
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

    /// Merge arbitrary string metadata pushed to `FileRegistry` each heartbeat.
    ///
    /// Empty values clear the matching metadata key. Existing unrelated keys
    /// remain unchanged.
    pub fn update_live_metadata(&self, metadata: HashMap<String, String>) {
        let mut guard = self.live_meta.write();
        for (name, value) in metadata {
            if value.is_empty() {
                guard.metadata.remove(&name);
            } else {
                guard.metadata.insert(name, value);
            }
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

    /// Attach a DCC main-thread executor for thread-safe DCC API calls (MCP `tools/call`).
    ///
    /// Does not require [`Self::with_host_bridge_runtime`]; that handle is only
    /// needed when REST `POST /v1/call` should use the same host-bridge path.
    pub fn with_executor(mut self, executor: DccExecutorHandle) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Tokio runtime that services the host-bridge mpsc paired with
    /// [`dcc_mcp_http_server::dispatcher_to_executor_handle`].
    ///
    /// Pass the same [`Handle`] given to `dispatcher_to_executor_handle`. When
    /// combined with [`Self::with_executor`], REST `POST /v1/call` honours
    /// `thread_affinity=main` like MCP `tools/call`.
    pub fn with_host_bridge_runtime(mut self, runtime: tokio::runtime::Handle) -> Self {
        self.host_bridge_runtime = Some(runtime);
        self
    }

    /// Attach an [`ToolDispatcher`] with pre-registered handlers.
    ///
    /// Use this when handlers are registered before starting the server.
    /// The dispatcher should be backed by the same [`ToolRegistry`].
    pub fn with_dispatcher(mut self, dispatcher: Arc<ToolDispatcher>) -> Self {
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

    /// Replace the [`ResourceRegistry`](crate::resources::ResourceRegistry)
    /// with a caller-supplied one (issue #730).
    ///
    /// Use this when the embedding layer needs to retain a handle to
    /// the registry so it can push scene snapshots, register custom
    /// producers, or wire an `OutputBuffer` **after** the server has
    /// been constructed. The canonical caller is the PyO3 binding in
    /// `crate::python::skill_server`, where `PyMcpHttpServer` pre-builds
    /// the registry at construction time so `server.resources()` can
    /// return the same instance both before and after `start()`.
    pub fn with_resources(mut self, resources: crate::resources::ResourceRegistry) -> Self {
        self.resources = resources;
        self
    }

    /// Use the given [`PromptRegistry`] instead of the default one.
    pub fn with_prompts(mut self, prompts: crate::prompts::PromptRegistry) -> Self {
        self.prompts = prompts;
        self
    }

    /// Install a shared [`ReadinessProbe`] (issue #714).
    ///
    /// The same probe is wired into **both** the MCP `tools/call`
    /// handler and the REST `POST /v1/call` handler, so a single
    /// `probe.set_dispatcher_ready(true); probe.set_dcc_ready(true)`
    /// from the hosting DCC adapter (e.g. `dcc-mcp-maya`) flips base
    /// routing readiness for every surface at once. Adapters that run
    /// main-thread tools should also flip the host execution bridge and
    /// main-thread executor bits when those paths are usable.
    ///
    /// When not installed, the server defaults to
    /// [`AppState::default_readiness`] (fully-ready) so existing
    /// standalone embedders and tests do not regress.
    pub fn with_readiness(mut self, probe: Arc<dyn ReadinessProbe>) -> Self {
        self.readiness = Some(probe);
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

        background_impl::spawn_session_eviction_task(
            &sessions,
            self.config.session.session_ttl_secs,
        );

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
            self.config.features.enable_resources,
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
            self.config.features.enable_job_notifications,
        );
        // Bridge JobManager transitions onto the notifier (#326).
        let notifier_cb = job_notifier.clone();
        jobs.subscribe(move |event| notifier_cb.on_job_event(event));
        // Issue #328: recover any in-flight rows from a prior run and
        // mark them Interrupted. Emits `$/dcc.jobUpdated` through the
        // just-wired notifier. Errors are logged but not fatal — the
        // server continues with an empty in-process map.
        //
        // Issue #567: `job_recovery = Requeue` is accepted but
        // currently degrades to `Drop`. Tool arguments are not
        // persisted on the `jobs` row yet, so reconstructing the
        // original `JobSpec` for re-submission isn't possible. We
        // emit a `WARN` once at startup so operators who plumbed
        // `Requeue` through their adapter know the requested policy
        // is reserved-but-not-active, then fall through to the same
        // `recover_from_storage` (Drop semantics) path.
        if jobs.storage().is_some() {
            if matches!(
                self.config.job.job_recovery,
                crate::config::JobRecoveryPolicy::Requeue
            ) {
                tracing::warn!(
                    requested_policy = "requeue",
                    effective_policy = "drop",
                    issue = "loonghao/dcc-mcp-core#567",
                    "job_recovery=Requeue is accepted but degrades to Drop until tool-arg persistence lands; \
                     in-flight rows will be marked Interrupted as if Drop were configured"
                );
            }
            match jobs.recover_from_storage() {
                Ok(n) if n > 0 => tracing::info!(
                    interrupted_jobs = n,
                    policy = self.config.job.job_recovery.as_str(),
                    "JobManager recovered pending/running rows from storage and marked them Interrupted"
                ),
                Ok(_) => tracing::debug!(
                    policy = self.config.job.job_recovery.as_str(),
                    "JobManager storage recovery found no in-flight rows"
                ),
                Err(e) => tracing::error!(
                    error = %e,
                    policy = self.config.job.job_recovery.as_str(),
                    "JobManager storage recovery failed — in-process map stays empty"
                ),
            }
        }

        // Issue #714 — the same ReadinessProbe instance backs both
        // `POST /v1/call` (REST) and `POST /mcp` (MCP tools/call), so
        // one flip from the DCC adapter gates every surface at once.
        let readiness = self
            .readiness
            .clone()
            .unwrap_or_else(AppState::default_readiness);

        let catalog_source =
            std::sync::Arc::new(dcc_mcp_skill_rest::CatalogSource::new(catalog.clone()));
        let invoker: std::sync::Arc<dyn dcc_mcp_skill_rest::ToolInvoker> =
            match (self.executor.clone(), self.host_bridge_runtime.clone()) {
                (Some(executor), Some(bridge_runtime)) => {
                    std::sync::Arc::new(dcc_mcp_http_server::ThreadRoutedInvoker::new(
                        self.dispatcher.clone(),
                        executor,
                        bridge_runtime,
                    ))
                }
                (Some(_), None) => {
                    tracing::warn!(
                        issue = "loonghao/dcc-mcp-core#1055",
                        "McpHttpServer: executor without host_bridge_runtime — MCP tools/call \
                     keeps main-thread routing; REST POST /v1/call uses direct dispatch. \
                     Call with_host_bridge_runtime(Handle) when the executor comes from \
                     dispatcher_to_executor_handle"
                    );
                    std::sync::Arc::new(dcc_mcp_skill_rest::DispatcherInvoker::new(
                        self.dispatcher.clone(),
                    ))
                }
                _ if self.config.features.standalone_main_thread_execution => std::sync::Arc::new(
                    dcc_mcp_skill_rest::DispatcherInvoker::new_standalone_main_thread(
                        self.dispatcher.clone(),
                    ),
                ),
                _ => std::sync::Arc::new(dcc_mcp_skill_rest::DispatcherInvoker::new(
                    self.dispatcher.clone(),
                )),
            };
        let rest_service = dcc_mcp_skill_rest::SkillRestService::new(catalog_source, invoker)
            .with_resources(std::sync::Arc::new(
                crate::rest_providers::ResourceRegistryAdapter::new(
                    resources.clone(),
                    catalog.clone(),
                ),
            ))
            .with_prompts(std::sync::Arc::new(
                crate::rest_providers::PromptRegistryAdapter::new(prompts.clone(), catalog.clone()),
            ));

        let mut rest_config = dcc_mcp_skill_rest::SkillRestConfig::new(rest_service)
            .with_readiness(readiness.clone());
        rest_config.server_title = self.config.server.server_name.clone();
        rest_config.server_version = self.config.server.server_version.clone();
        let rest_router = dcc_mcp_skill_rest::build_skill_rest_router(rest_config);

        let server_state =
            dcc_mcp_http_server::ServerState::builder(self.registry, self.dispatcher, catalog)
                .with_sessions(sessions)
                .with_executor(self.executor)
                .with_standalone_main_thread_execution(
                    self.config.features.standalone_main_thread_execution,
                )
                .with_server_identity(
                    self.config.server.server_name.clone(),
                    self.config.server.server_version.clone(),
                )
                .with_cancelled_requests(cancelled_requests)
                .with_lazy_actions(self.config.features.lazy_actions)
                .with_bare_tool_names(self.config.features.bare_tool_names)
                .with_exclude_skill_stubs_from_tools_list(
                    self.config.features.exclude_skill_stubs_from_tools_list,
                )
                .with_exclude_group_stubs_from_tools_list(
                    self.config.features.exclude_group_stubs_from_tools_list,
                )
                .with_declared_capabilities(self.config.instance.declared_capabilities.clone())
                .with_jobs(jobs)
                .with_job_notifier(job_notifier)
                .with_resources_enabled(self.config.features.enable_resources)
                .with_prompts_enabled(self.config.features.enable_prompts)
                .with_tool_cache_enabled(self.config.session.enable_tool_cache);
        #[cfg(feature = "prometheus")]
        let server_state = server_state.with_prometheus(prometheus.clone());

        let state = AppState {
            server: server_state.build(),
            bridge_registry: crate::BridgeRegistry::new(),
            resources,
            prompts,
            readiness,
        };

        let app_state_for_rmcp = state.clone();

        let mut router = Router::new()
            .route(
                "/health",
                routing::get(|| async { Json(json!({"ok": true, "service": "dcc-mcp-http"})) }),
            )
            .with_state(state)
            .merge(rest_router)
            .layer(RequestBodyLimitLayer::new(
                self.config.queue.max_request_body_bytes,
            ))
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

        // MCP endpoint — speaks MCP 2025-11-25 via the official rmcp SDK.
        router = crate::handler::rmcp_mount::attach_rmcp_endpoint(router, &app_state_for_rmcp);

        if self.config.server.enable_cors {
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
            server_name = %self.config.server.server_name,
            server_version = %self.config.server.server_version,
            "MCP HTTP server {} v{} listening on http://{}{}",
            self.config.server.server_name,
            self.config.server.server_version,
            actual_bind,
            self.config.server.endpoint_path,
        );

        // ── Optional gateway competition ──────────────────────────────────────
        //
        // Gated by the `auto-gateway` feature (default-on, issue #1357).
        // When the feature is off, `gateway_port` is a no-op and this
        // server runs purely as a per-DCC backend.
        #[cfg(feature = "auto-gateway")]
        let gateway_handle =
            gateway_impl::start_gateway_runner(&self.config, port, &self.live_meta).await;

        #[cfg(feature = "auto-gateway")]
        let is_gateway = gateway_handle
            .as_ref()
            .map(|h| h.is_gateway)
            .unwrap_or(false);

        #[cfg(not(feature = "auto-gateway"))]
        let is_gateway = false;

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
            #[cfg(feature = "auto-gateway")]
            _gateway: gateway_handle,
        })
    }
}

/// Convenience: start a server from the current Tokio runtime context.
///
/// Useful when embedding in Python via `block_on`.
pub fn start_in_runtime(
    runtime: &tokio::runtime::Runtime,
    registry: Arc<ToolRegistry>,
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
    match &config.job.job_storage_path {
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
