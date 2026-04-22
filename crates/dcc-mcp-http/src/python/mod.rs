//! PyO3 bindings for the MCP HTTP server.

use parking_lot::RwLock;
use pyo3::prelude::*;
use std::sync::Arc;
use tokio::runtime::Runtime;

use crate::{
    config::McpHttpConfig,
    server::{LiveMetaInner, McpHttpServer, McpServerHandle},
};
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_skills::SkillCatalog;

/// Python-visible MCP HTTP server configuration.
///
/// Example::
///
///     from dcc_mcp_core import McpHttpConfig
///     config = McpHttpConfig(port=8765, server_name="my-dcc")
#[pyclass(name = "McpHttpConfig", skip_from_py_object)]
#[derive(Clone)]
pub struct PyMcpHttpConfig {
    pub(crate) inner: McpHttpConfig,
}

#[pymethods]
impl PyMcpHttpConfig {
    /// Create a new config. ``port=0`` binds to any available port.
    #[new]
    #[pyo3(signature = (port=8765, server_name=None, server_version=None, enable_cors=false, request_timeout_ms=30000, backend_timeout_ms=10_000, enable_prometheus=false, prometheus_basic_auth=None, gateway_async_dispatch_timeout_ms=60_000, gateway_wait_terminal_timeout_ms=600_000, gateway_route_ttl_secs=86_400, gateway_max_routes_per_session=1_000))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        port: u16,
        server_name: Option<String>,
        server_version: Option<String>,
        enable_cors: bool,
        request_timeout_ms: u64,
        backend_timeout_ms: u64,
        enable_prometheus: bool,
        prometheus_basic_auth: Option<(String, String)>,
        gateway_async_dispatch_timeout_ms: u64,
        gateway_wait_terminal_timeout_ms: u64,
        gateway_route_ttl_secs: u64,
        gateway_max_routes_per_session: u64,
    ) -> Self {
        let mut cfg = McpHttpConfig::new(port);
        if let Some(name) = server_name {
            cfg.server_name = name;
        }
        if let Some(ver) = server_version {
            cfg.server_version = ver;
        }
        cfg.enable_cors = enable_cors;
        cfg.request_timeout_ms = request_timeout_ms;
        cfg.backend_timeout_ms = backend_timeout_ms;
        cfg.enable_prometheus = enable_prometheus;
        cfg.prometheus_basic_auth = prometheus_basic_auth;
        cfg.gateway_async_dispatch_timeout_ms = gateway_async_dispatch_timeout_ms;
        cfg.gateway_wait_terminal_timeout_ms = gateway_wait_terminal_timeout_ms;
        cfg.gateway_route_ttl_secs = gateway_route_ttl_secs;
        cfg.gateway_max_routes_per_session = gateway_max_routes_per_session;
        // Issue #303: PyO3-embedded hosts (Maya on Windows etc.) cannot
        // rely on shared tokio worker threads to drive the accept loop
        // after `block_on` returns. Default to `Dedicated` so the listener
        // runs on its own OS thread owning a `current_thread` runtime.
        cfg.spawn_mode = crate::config::ServerSpawnMode::Dedicated;
        Self { inner: cfg }
    }

    #[getter]
    fn port(&self) -> u16 {
        self.inner.port
    }

    #[getter]
    fn host(&self) -> String {
        self.inner.host.to_string()
    }

    #[getter]
    fn endpoint_path(&self) -> &str {
        &self.inner.endpoint_path
    }

    #[getter]
    fn server_name(&self) -> &str {
        &self.inner.server_name
    }

    #[getter]
    fn server_version(&self) -> &str {
        &self.inner.server_version
    }

    #[getter]
    fn max_sessions(&self) -> usize {
        self.inner.max_sessions
    }

    #[getter]
    fn request_timeout_ms(&self) -> u64 {
        self.inner.request_timeout_ms
    }

    #[getter]
    fn enable_cors(&self) -> bool {
        self.inner.enable_cors
    }

    /// Enable the Prometheus ``/metrics`` endpoint (issue #331).
    ///
    /// When ``True``, ``McpHttpServer.start()`` mounts a ``GET /metrics``
    /// route alongside ``/mcp``. The payload is a standard Prometheus
    /// text-exposition body (``text/plain; version=0.0.4``) suitable
    /// for direct scraping by Prometheus, VictoriaMetrics, or any
    /// OpenMetrics-compatible collector.
    ///
    /// Requires the ``prometheus`` Cargo feature to be enabled at
    /// wheel-build time. On wheels built without the feature this
    /// flag is accepted but silently has no effect.
    #[getter]
    fn enable_prometheus(&self) -> bool {
        self.inner.enable_prometheus
    }

    #[setter]
    fn set_enable_prometheus(&mut self, enabled: bool) {
        self.inner.enable_prometheus = enabled;
    }

    /// Optional HTTP Basic auth for ``/metrics`` (issue #331).
    ///
    /// Tuple of ``(username, password)`` or ``None``. When set,
    /// scrapers must present a matching
    /// ``Authorization: Basic base64(user:pass)`` header or the
    /// endpoint responds with ``401 Unauthorized``. ``None`` leaves
    /// the endpoint open — appropriate for localhost-only dev, but
    /// configure credentials for anything exposed beyond that.
    #[getter]
    fn prometheus_basic_auth(&self) -> Option<(String, String)> {
        self.inner.prometheus_basic_auth.clone()
    }

    #[setter]
    fn set_prometheus_basic_auth(&mut self, auth: Option<(String, String)>) {
        self.inner.prometheus_basic_auth = auth;
    }

    /// Idle session TTL in seconds. Sessions not touched within this window are
    /// automatically evicted. Default: 3600 (1 hour). Set to 0 to disable.
    #[getter]
    fn session_ttl_secs(&self) -> u64 {
        self.inner.session_ttl_secs
    }

    #[setter]
    fn set_session_ttl_secs(&mut self, secs: u64) {
        self.inner.session_ttl_secs = secs;
    }

    /// Enable the opt-in lazy-actions fast-path (#254).
    ///
    /// When ``True``, ``tools/list`` also surfaces three meta-tools:
    /// ``list_actions``, ``describe_action`` and ``call_action``. Useful
    /// for agents whose context budget cannot afford a full ``tools/list``
    /// paging session. Default: ``False``.
    #[getter]
    fn lazy_actions(&self) -> bool {
        self.inner.lazy_actions
    }

    #[setter]
    fn set_lazy_actions(&mut self, enabled: bool) {
        self.inner.lazy_actions = enabled;
    }

    /// Enable the built-in ``workflows.*`` tools (issue #348).
    ///
    /// Default: ``False``. Step execution is stubbed in the skeleton —
    /// see :class:`WorkflowSpec` for the parse/validate surface that is
    /// already usable.
    #[getter]
    fn enable_workflows(&self) -> bool {
        self.inner.enable_workflows
    }

    #[setter]
    fn set_enable_workflows(&mut self, enabled: bool) {
        self.inner.enable_workflows = enabled;
    }

    /// Emit the ``$/dcc.jobUpdated`` and ``$/dcc.workflowUpdated`` SSE
    /// channels (issue #326).
    ///
    /// Default: ``True``. When ``False``, the server still emits the
    /// spec-mandated ``notifications/progress`` channel for callers that
    /// supplied ``_meta.progressToken``, but the ``$/dcc.*`` vendor
    /// extensions are suppressed.
    #[getter]
    fn enable_job_notifications(&self) -> bool {
        self.inner.enable_job_notifications
    }

    #[setter]
    fn set_enable_job_notifications(&mut self, enabled: bool) {
        self.inner.enable_job_notifications = enabled;
    }

    /// Optional filesystem path to a SQLite database used to persist
    /// tracked jobs across server restarts (issue #328).
    ///
    /// When set, ``McpHttpServer.start()`` opens (or creates) the file
    /// and attaches a write-through storage backend to the
    /// ``JobManager``; any pre-existing ``pending``/``running`` rows
    /// are rewritten to a terminal ``interrupted`` status on startup
    /// so clients never see silently "lost" jobs.
    ///
    /// Requires the ``job-persist-sqlite`` Cargo feature; when the
    /// wheel was built without that feature, setting this path
    /// causes ``server.start()`` to raise a descriptive error.
    ///
    /// Default: ``None`` (in-memory only, no persistence).
    #[getter]
    fn job_storage_path(&self) -> Option<String> {
        self.inner
            .job_storage_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    #[setter]
    fn set_job_storage_path(&mut self, path: Option<String>) {
        self.inner.job_storage_path = path.map(std::path::PathBuf::from);
    }

    // ── Gateway configuration ────────────────────────────────────────────────

    /// Gateway port to compete for. First process to bind wins.
    /// ``0`` disables gateway (default).
    ///
    /// Example::
    ///
    ///     config = McpHttpConfig(port=0, server_name="maya")
    ///     config.gateway_port = 9765   # join the gateway competition
    ///     config.dcc_type = "maya"
    ///     server = McpHttpServer(registry, config)
    ///     handle = server.start()
    ///     print(handle.is_gateway)   # True if this process won
    #[getter]
    fn gateway_port(&self) -> u16 {
        self.inner.gateway_port
    }

    #[setter]
    fn set_gateway_port(&mut self, port: u16) {
        self.inner.gateway_port = port;
    }

    /// Shared FileRegistry directory path. ``None`` uses a system temp dir.
    #[getter]
    fn registry_dir(&self) -> Option<String> {
        self.inner
            .registry_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    #[setter]
    fn set_registry_dir(&mut self, dir: Option<String>) {
        self.inner.registry_dir = dir.map(std::path::PathBuf::from);
    }

    /// Seconds without heartbeat before an instance is stale. Default: 30.
    #[getter]
    fn stale_timeout_secs(&self) -> u64 {
        self.inner.stale_timeout_secs
    }

    #[setter]
    fn set_stale_timeout_secs(&mut self, secs: u64) {
        self.inner.stale_timeout_secs = secs;
    }

    /// Heartbeat interval in seconds. ``0`` disables heartbeat. Default: 5.
    #[getter]
    fn heartbeat_secs(&self) -> u64 {
        self.inner.heartbeat_secs
    }

    #[setter]
    fn set_heartbeat_secs(&mut self, secs: u64) {
        self.inner.heartbeat_secs = secs;
    }

    // ── Instance registration metadata ───────────────────────────────────────

    /// DCC application type (e.g. ``"maya"``). Reported in the shared registry.
    #[getter]
    fn dcc_type(&self) -> Option<String> {
        self.inner.dcc_type.clone()
    }

    #[setter]
    fn set_dcc_type(&mut self, v: Option<String>) {
        self.inner.dcc_type = v;
    }

    /// DCC application version (e.g. ``"2025.1"``).
    #[getter]
    fn dcc_version(&self) -> Option<String> {
        self.inner.dcc_version.clone()
    }

    #[setter]
    fn set_dcc_version(&mut self, v: Option<String>) {
        self.inner.dcc_version = v;
    }

    /// Currently open scene/file. Improves routing accuracy.
    #[getter]
    fn scene(&self) -> Option<String> {
        self.inner.scene.clone()
    }

    #[setter]
    fn set_scene(&mut self, v: Option<String>) {
        self.inner.scene = v;
    }

    /// Listener spawn strategy (issue #303).
    ///
    /// - ``"ambient"`` — listener runs as ``tokio::spawn`` on the caller's
    ///   runtime. Correct for standalone binaries.
    /// - ``"dedicated"`` — listener runs on its own OS thread owning a
    ///   ``current_thread`` runtime. Default for PyO3-embedded callers
    ///   (Maya/Blender/etc.) where shared workers can be starved.
    #[getter]
    fn spawn_mode(&self) -> &'static str {
        match self.inner.spawn_mode {
            crate::config::ServerSpawnMode::Ambient => "ambient",
            crate::config::ServerSpawnMode::Dedicated => "dedicated",
        }
    }

    #[setter]
    fn set_spawn_mode(&mut self, mode: &str) -> PyResult<()> {
        self.inner.spawn_mode = match mode {
            "ambient" => crate::config::ServerSpawnMode::Ambient,
            "dedicated" => crate::config::ServerSpawnMode::Dedicated,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "spawn_mode must be 'ambient' or 'dedicated', got {other:?}"
                )));
            }
        };
        Ok(())
    }

    /// Self-probe timeout in milliseconds. 0 disables the probe.
    /// Default: 200. Issue #303 guard.
    #[getter]
    fn self_probe_timeout_ms(&self) -> u64 {
        self.inner.self_probe_timeout_ms
    }

    #[setter]
    fn set_self_probe_timeout_ms(&mut self, ms: u64) {
        self.inner.self_probe_timeout_ms = ms;
    }

    /// Publish skill-scoped tools under their **bare action name** when no
    /// collision exists on this instance (#307).
    ///
    /// When ``True`` (default), ``tools/list`` emits ``execute_python``
    /// rather than ``maya-scripting.execute_python`` whenever the bare name
    /// is unique within the instance's loaded skills. Collisions fall back
    /// to the full ``<skill>.<action>`` form, and ``tools/call`` accepts
    /// both shapes for one release cycle.
    #[getter]
    fn bare_tool_names(&self) -> bool {
        self.inner.bare_tool_names
    }

    #[setter]
    fn set_bare_tool_names(&mut self, enabled: bool) {
        self.inner.bare_tool_names = enabled;
    }

    /// DCC capabilities this adapter provides (issue #354).
    ///
    /// Freeform string tags (e.g. ``"usd"``, ``"scene.mutate"``,
    /// ``"filesystem.read"``) consumed by the capability gate in
    /// ``tools/call``. Tools whose ``required_capabilities`` are not fully
    /// covered still surface in ``tools/list`` but fail the call with
    /// JSON-RPC error ``-32001 capability_missing`` and carry
    /// ``_meta.dcc.missing_capabilities`` in the list response so clients
    /// can filter them out of the menu.
    ///
    /// Defaults to an empty list. Hard-code the capabilities your DCC
    /// adapter knows it provides; there is no runtime introspection.
    #[getter]
    fn declared_capabilities(&self) -> Vec<String> {
        self.inner.declared_capabilities.clone()
    }

    #[setter]
    fn set_declared_capabilities(&mut self, caps: Vec<String>) {
        self.inner.declared_capabilities = caps;
    }

    /// Per-backend gateway fan-out timeout in milliseconds (issue #314).
    ///
    /// Default: ``10_000`` (10 seconds). Raise this for DCC workflows that
    /// legitimately run backend tools longer than 10 seconds (scene import,
    /// simulation bake, large USD composition) to avoid spurious transport
    /// timeout errors on the gateway fan-out path.
    #[getter]
    fn backend_timeout_ms(&self) -> u64 {
        self.inner.backend_timeout_ms
    }

    #[setter]
    fn set_backend_timeout_ms(&mut self, ms: u64) {
        self.inner.backend_timeout_ms = ms;
    }

    /// Gateway timeout (ms) for async-dispatch `tools/call` requests
    /// (issue #321). Default: ``60_000``.
    ///
    /// Applies when the outbound call carries ``_meta.dcc.async == true``,
    /// a ``_meta.progressToken``, or targets a tool whose ``ActionMeta``
    /// declares ``execution: async`` / a ``timeout_hint_secs``. Only the
    /// **queuing** step uses this budget — the backend replies with
    /// ``{status: "pending"}`` as soon as the job is enqueued.
    #[getter]
    fn gateway_async_dispatch_timeout_ms(&self) -> u64 {
        self.inner.gateway_async_dispatch_timeout_ms
    }

    #[setter]
    fn set_gateway_async_dispatch_timeout_ms(&mut self, ms: u64) {
        self.inner.gateway_async_dispatch_timeout_ms = ms;
    }

    /// Gateway timeout (ms) for the opt-in wait-for-terminal passthrough
    /// mode (issue #321). Default: ``600_000`` (10 minutes).
    ///
    /// When the client sets ``_meta.dcc.wait_for_terminal = true`` along
    /// with an async opt-in, the gateway blocks the ``tools/call``
    /// response until a ``$/dcc.jobUpdated`` with a terminal status
    /// arrives. On timeout the gateway returns the last known status
    /// with ``_meta.dcc.timed_out = true`` and leaves the job running
    /// on the backend.
    #[getter]
    fn gateway_wait_terminal_timeout_ms(&self) -> u64 {
        self.inner.gateway_wait_terminal_timeout_ms
    }

    #[setter]
    fn set_gateway_wait_terminal_timeout_ms(&mut self, ms: u64) {
        self.inner.gateway_wait_terminal_timeout_ms = ms;
    }

    /// Gateway routing-cache TTL (seconds) for `JobRoute` entries
    /// (issue #322). Default: ``86_400`` (24 hours).
    ///
    /// Routes that don't see a terminal notification within this window
    /// are evicted by a background GC task so the cache cannot grow
    /// without bound under pathological agents or crashed backends.
    #[getter]
    fn gateway_route_ttl_secs(&self) -> u64 {
        self.inner.gateway_route_ttl_secs
    }

    #[setter]
    fn set_gateway_route_ttl_secs(&mut self, secs: u64) {
        self.inner.gateway_route_ttl_secs = secs;
    }

    /// Per-session ceiling on concurrent live gateway routes (issue
    /// #322). ``0`` disables the cap. Default: ``1_000``.
    ///
    /// When a client session is already holding this many live routes,
    /// new async ``tools/call`` requests are rejected with JSON-RPC
    /// ``-32005 too_many_in_flight_jobs``.
    #[getter]
    fn gateway_max_routes_per_session(&self) -> u64 {
        self.inner.gateway_max_routes_per_session
    }

    #[setter]
    fn set_gateway_max_routes_per_session(&mut self, cap: u64) {
        self.inner.gateway_max_routes_per_session = cap;
    }

    /// Advertise the MCP Resources primitive (issue #350).
    ///
    /// When ``True`` (default), the server advertises
    /// ``resources: { subscribe, listChanged }`` in its ``initialize``
    /// response and handles ``resources/list`` / ``resources/read`` /
    /// ``resources/subscribe`` / ``resources/unsubscribe``. Built-in
    /// producers surface ``scene://current`` (JSON), ``audit://recent``
    /// (JSON) and ``capture://current_window`` (PNG, when a real window
    /// backend is available).
    #[getter]
    fn enable_resources(&self) -> bool {
        self.inner.enable_resources
    }

    #[setter]
    fn set_enable_resources(&mut self, enabled: bool) {
        self.inner.enable_resources = enabled;
    }

    /// Expose ``artefact://`` resources (issue #349).
    ///
    /// Default ``False``. The full artefact store lands in issue #349;
    /// this flag merely gates whether the ``artefact://`` scheme appears
    /// in ``resources/list`` and whether reads return a descriptive
    /// ``-32002`` error versus a normal not-found.
    #[getter]
    fn enable_artefact_resources(&self) -> bool {
        self.inner.enable_artefact_resources
    }

    #[setter]
    fn set_enable_artefact_resources(&mut self, enabled: bool) {
        self.inner.enable_artefact_resources = enabled;
    }

    /// Advertise the MCP Prompts primitive (issues #351, #355).
    ///
    /// When ``True`` (default), the server advertises
    /// ``prompts: { listChanged }`` in its ``initialize`` response and
    /// handles ``prompts/list`` + ``prompts/get``. Prompts are sourced
    /// from each loaded skill's sibling ``prompts.yaml`` (pointed at by
    /// ``metadata.dcc-mcp.prompts`` in SKILL.md) plus workflow-derived
    /// auto-generated entries.
    #[getter]
    fn enable_prompts(&self) -> bool {
        self.inner.enable_prompts
    }

    #[setter]
    fn set_enable_prompts(&mut self, enabled: bool) {
        self.inner.enable_prompts = enabled;
    }

    fn __repr__(&self) -> String {
        format!(
            "McpHttpConfig(port={}, name={}, gateway_port={})",
            self.inner.port, self.inner.server_name, self.inner.gateway_port
        )
    }
}

/// Handle returned by `McpHttpServer.start()`.
///
/// Example::
///
///     handle = server.start()
///     # ... later ...
///     handle.shutdown()
#[pyclass(name = "McpServerHandle", skip_from_py_object)]
pub struct PyServerHandle {
    inner: Option<McpServerHandle>,
    runtime: Arc<Runtime>,
    pub port: u16,
    pub bind_addr: String,
    /// ``True`` if this process won the gateway port competition.
    pub is_gateway: bool,
    /// Shared live metadata — mirrors `McpHttpServer::live_meta` so Python
    /// can push scene/version/documents updates that flow into FileRegistry
    /// on the next heartbeat tick.
    live_meta: Arc<RwLock<LiveMetaInner>>,
}

#[pymethods]
impl PyServerHandle {
    /// The actual port the server is listening on.
    #[getter]
    fn port(&self) -> u16 {
        self.port
    }

    /// The bind address (e.g. ``127.0.0.1:8765``).
    #[getter]
    fn bind_addr(&self) -> &str {
        &self.bind_addr
    }

    /// The full MCP endpoint URL.
    fn mcp_url(&self) -> String {
        format!("http://{}/mcp", self.bind_addr)
    }

    /// Gracefully shut down the server.
    fn shutdown(&mut self) {
        if let Some(handle) = self.inner.take() {
            self.runtime.block_on(handle.shutdown());
        }
    }

    /// Signal shutdown without blocking.
    fn signal_shutdown(&self) {
        if let Some(handle) = &self.inner {
            handle.signal_shutdown();
        }
    }

    /// ``True`` if this process won the gateway port competition.
    #[getter]
    fn is_gateway(&self) -> bool {
        self.is_gateway
    }

    /// Update the live instance metadata in the gateway registry.
    ///
    /// Works for both single-document DCCs (Maya, Blender — pass ``scene``
    /// only) and multi-document DCCs (Photoshop, After Effects — also pass
    /// ``documents`` with the full list of open files and optionally
    /// ``display_name`` to label the instance).
    ///
    /// Values are written into the shared live-metadata store and propagated
    /// to ``FileRegistry`` on the next heartbeat tick (≤ 5 s).  After the
    /// update, ``list_dcc_instances`` reflects the change so AI agents and
    /// users can identify the correct instance without restarting.
    ///
    /// Pass ``None`` to leave a field unchanged; pass ``""`` / ``[]`` to
    /// clear it.
    ///
    /// Examples::
    ///
    ///     # Maya — single active scene:
    ///     handle.update_scene("C:/projects/hero/rig.ma")
    ///
    ///     # Photoshop — active document + all open docs + instance label:
    ///     handle.update_scene(
    ///         scene="hero_comp.psd",
    ///         documents=["hero_comp.psd", "bg_plate.psd", "overlay.psd"],
    ///         display_name="PS-Marketing",
    ///     )
    ///
    ///     # Clear the document list (single-doc mode again):
    ///     handle.update_scene(documents=[])
    ///
    /// Args:
    ///     scene: Active/focused scene or document path.
    ///             ``None`` = no change, ``""`` = clear.
    ///     version: DCC application version string.
    ///              ``None`` = no change, ``""`` = clear.
    ///     documents: Full list of open documents (multi-doc DCCs).
    ///                ``None`` = no change, ``[]`` = clear list.
    ///     display_name: Human-readable instance label (e.g. ``"PS-Marketing"``).
    ///                   ``None`` = no change, ``""`` = clear.
    #[pyo3(signature = (scene=None, version=None, documents=None, display_name=None))]
    fn update_scene(
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

    fn __repr__(&self) -> String {
        format!(
            "McpServerHandle(addr={}, running={}, is_gateway={})",
            self.bind_addr,
            self.inner.is_some(),
            self.is_gateway,
        )
    }
}

/// MCP Streamable HTTP server for embedding in DCC software.
///
/// Example::
///
///     from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig
///
///     registry = ActionRegistry()
///     registry.register("get_scene_info", description="Get scene info", category="scene")
///
///     server = McpHttpServer(registry, McpHttpConfig(port=8765))
///     handle = server.start()
///     print(f"MCP server at {handle.mcp_url()}")
///     # MCP Host connects to http://127.0.0.1:8765/mcp
///
///     # Shutdown:
///     handle.shutdown()
#[pyclass(name = "McpHttpServer", skip_from_py_object)]
pub struct PyMcpHttpServer {
    registry: Arc<ActionRegistry>,
    dispatcher: Arc<ActionDispatcher>,
    catalog: Arc<SkillCatalog>,
    config: McpHttpConfig,
    runtime: Arc<Runtime>,
    /// Shared live metadata — written by Python via `update_scene()` /
    /// `update_gateway_metadata()`; propagated to FileRegistry each heartbeat.
    live_meta: Arc<RwLock<LiveMetaInner>>,
}

#[pymethods]
impl PyMcpHttpServer {
    /// Create a new MCP HTTP server.
    ///
    /// Args:
    ///     registry: An ``ActionRegistry`` with registered DCC actions.
    ///     config: A ``McpHttpConfig``. If omitted, defaults to port 8765.
    #[new]
    #[pyo3(signature = (registry, config=None))]
    fn new(registry: &ActionRegistry, config: Option<&PyMcpHttpConfig>) -> PyResult<Self> {
        let cfg = config.map(|c| c.inner.clone()).unwrap_or_default();

        let runtime =
            Runtime::new().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let reg = Arc::new(registry.clone());
        let dispatcher = Arc::new(ActionDispatcher::new((*reg).clone()));
        // Wire the catalog to the same dispatcher so load_skill auto-registers handlers
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            reg.clone(),
            dispatcher.clone(),
        ));

        let live_meta = Arc::new(RwLock::new(LiveMetaInner {
            scene: cfg.scene.clone(),
            version: cfg.dcc_version.clone(),
            ..Default::default()
        }));
        Ok(Self {
            registry: reg,
            dispatcher,
            catalog,
            config: cfg,
            runtime: Arc::new(runtime),
            live_meta,
        })
    }

    /// Start the server and return a :class:`McpServerHandle`.
    ///
    /// This call returns immediately; the server runs in a background thread.
    fn start(&self) -> PyResult<PyServerHandle> {
        let server = McpHttpServer::with_catalog(
            self.registry.clone(),
            self.catalog.clone(),
            self.config.clone(),
        )
        .with_dispatcher(self.dispatcher.clone())
        .with_live_meta(self.live_meta.clone());
        let handle = self
            .runtime
            .block_on(server.start())
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let port = handle.port;
        let bind_addr = handle.bind_addr.clone();
        let is_gateway = handle.is_gateway;

        Ok(PyServerHandle {
            inner: Some(handle),
            runtime: self.runtime.clone(),
            port,
            bind_addr,
            is_gateway,
            live_meta: self.live_meta.clone(),
        })
    }

    /// Register a Python callable as the handler for ``action_name``.
    ///
    /// The callable receives a single argument: a dict of action parameters.
    /// It must return a JSON-serialisable value.
    ///
    /// Example::
    ///
    ///     server.register_handler("get_scene_info", lambda params: {"scene": "untitled"})
    ///
    /// Raises:
    ///     TypeError: If ``handler`` is not callable.
    #[pyo3(signature = (action_name, handler))]
    fn register_handler(
        &self,
        py: Python<'_>,
        action_name: &str,
        handler: Py<PyAny>,
    ) -> PyResult<()> {
        if !handler.bind(py).is_callable() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "handler must be callable",
            ));
        }
        // Store a Rust closure in the dispatcher that calls the Python callable.
        // The closure re-acquires the GIL via Python::attach (pyo3 0.28+)
        // and converts both params and return values through serde_json so the
        // Python-side contract matches ActionDispatcher: dict/list/scalars in,
        // JSON-serialisable values out.
        let handler_ref = handler.clone_ref(py);
        self.dispatcher
            .register_handler(action_name, move |params| {
                Python::attach(|gil| {
                    use dcc_mcp_utils::py_json::{json_value_to_bound_py, py_any_to_json_value};

                    let py_params = json_value_to_bound_py(gil, &params)
                        .map_err(|e| format!("failed to convert params: {e}"))?;
                    let raw = handler_ref
                        .call1(gil, (py_params,))
                        .map_err(|e| format!("handler error: {e}"))?;
                    py_any_to_json_value(raw.bind(gil)).map_err(|e| e.to_string())
                })
            });
        Ok(())
    }

    /// Return ``True`` if a handler is registered for ``action_name``.
    #[pyo3(signature = (action_name))]
    fn has_handler(&self, action_name: &str) -> bool {
        self.dispatcher.has_handler(action_name)
    }

    /// The server's :class:`ToolRegistry`.
    ///
    /// Returned value shares the underlying storage with the server —
    /// ``register()`` calls on it will update the tools exposed via
    /// ``tools/list``. Must be populated **before** calling :meth:`start`.
    #[getter]
    fn registry(&self) -> ActionRegistry {
        (*self.registry).clone()
    }

    /// Access the server's SkillCatalog for progressive skill loading.
    ///
    /// Returns a debug representation of the catalog state (total/loaded counts).
    /// Use ``discover()``, ``load_skill()``, ``list_skills()`` etc. directly on
    /// the server object to interact with skills.
    #[getter]
    fn catalog(&self) -> String {
        format!(
            "SkillCatalog(total={}, loaded={})",
            self.catalog.len(),
            self.catalog.loaded_count()
        )
    }

    /// Discover skills from standard scan paths.
    ///
    /// Args:
    ///     extra_paths: Additional directories to scan.
    ///     dcc_name: DCC name filter (e.g. ``"maya"``).
    ///
    /// Returns the number of newly discovered skills.
    #[pyo3(signature = (extra_paths=None, dcc_name=None))]
    fn discover(&self, extra_paths: Option<Vec<String>>, dcc_name: Option<&str>) -> usize {
        self.catalog.discover(extra_paths.as_deref(), dcc_name)
    }

    /// Load a skill by name — registers its tools in the ActionRegistry.
    ///
    /// Returns the list of registered action names.
    /// Raises ``ValueError`` if the skill is not found.
    fn load_skill(&self, skill_name: &str) -> PyResult<Vec<String>> {
        self.catalog
            .load_skill(skill_name)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Unload a skill — removes its tools from the ActionRegistry.
    ///
    /// Returns the number of actions removed.
    /// Raises ``ValueError`` if the skill is not loaded.
    fn unload_skill(&self, skill_name: &str) -> PyResult<usize> {
        self.catalog
            .unload_skill(skill_name)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Search for skills matching the given criteria.
    #[pyo3(signature = (query=None, tags=vec![], dcc=None))]
    fn find_skills(
        &self,
        py: Python<'_>,
        query: Option<&str>,
        tags: Vec<String>,
        dcc: Option<&str>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        self.catalog
            .find_skills(query, &tag_refs, dcc)
            .into_iter()
            .map(|s| {
                let val = serde_json::to_value(&s)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                json_value_to_pyobject(py, &val)
            })
            .collect::<PyResult<Vec<Py<PyAny>>>>()
    }

    /// List all skills with their load status.
    #[pyo3(signature = (status=None))]
    fn list_skills(&self, py: Python<'_>, status: Option<&str>) -> PyResult<Vec<Py<PyAny>>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        self.catalog
            .list_skills(status)
            .into_iter()
            .map(|s| {
                let val = serde_json::to_value(&s)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                json_value_to_pyobject(py, &val)
            })
            .collect::<PyResult<Vec<Py<PyAny>>>>()
    }

    /// Get detailed info about a specific skill as a Python dict.
    ///
    /// Returns ``None`` if the skill is not found.
    fn get_skill_info(&self, py: Python<'_>, skill_name: &str) -> PyResult<Option<Py<PyAny>>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        match self.catalog.get_skill_info(skill_name) {
            Some(info) => {
                let val = serde_json::to_value(&info)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                Ok(Some(json_value_to_pyobject(py, &val)?))
            }
            None => Ok(None),
        }
    }

    /// Check if a skill is loaded.
    fn is_loaded(&self, skill_name: &str) -> bool {
        self.catalog.is_loaded(skill_name)
    }

    /// Number of loaded skills.
    fn loaded_count(&self) -> usize {
        self.catalog.loaded_count()
    }

    fn __repr__(&self) -> String {
        format!(
            "McpHttpServer(name={}, port={})",
            self.config.server_name, self.config.port
        )
    }
}

// ── WorkspaceRoots (issue #354) ──────────────────────────────────────────

/// Typed `workspace://` URI resolver built from the client-advertised MCP
/// roots (issue #354).
///
/// Example::
///
///     from dcc_mcp_core import WorkspaceRoots
///     roots = WorkspaceRoots(["/projects/hero"])
///     assert roots.resolve("workspace://scenes/a.usd").endswith("scenes/a.usd")
///     assert roots.roots == ["file:///projects/hero"]
#[pyclass(name = "WorkspaceRoots", skip_from_py_object)]
#[derive(Clone, Default)]
pub struct PyWorkspaceRoots {
    pub(crate) inner: crate::workspace::WorkspaceRoots,
}

#[pymethods]
impl PyWorkspaceRoots {
    /// Build from a list of filesystem roots, URI strings, or a mix.
    ///
    /// Each entry that already starts with a scheme (``file://``,
    /// ``custom://``) is kept verbatim; bare paths are converted into a
    /// ``file://`` URI.
    #[new]
    #[pyo3(signature = (roots = None))]
    fn new(roots: Option<Vec<String>>) -> Self {
        let raw = roots.unwrap_or_default();
        let mut client_roots = Vec::with_capacity(raw.len());
        for r in raw {
            let uri = if r.contains("://") {
                r
            } else {
                let normalised = r.replace('\\', "/");
                if normalised.starts_with('/') {
                    format!("file://{normalised}")
                } else {
                    format!("file:///{normalised}")
                }
            };
            client_roots.push(crate::protocol::ClientRoot { uri, name: None });
        }
        Self {
            inner: crate::workspace::WorkspaceRoots::from_client_roots(&client_roots),
        }
    }

    /// All roots (as URI strings) in declaration order.
    #[getter]
    fn roots(&self) -> Vec<String> {
        self.inner.roots().to_vec()
    }

    /// Resolve a typed path against the workspace.
    ///
    /// Rules:
    ///
    /// * ``workspace://<rest>`` → joined against the first advertised
    ///   ``file://`` root. Raises ``ValueError`` (MCP error code
    ///   ``-32602``) when no roots are advertised.
    /// * Absolute platform paths are returned unchanged.
    /// * Relative paths are joined against the first root when one is
    ///   available; otherwise returned unchanged.
    fn resolve(&self, path: &str) -> PyResult<String> {
        match self.inner.resolve(path) {
            Ok(p) => Ok(p.to_string_lossy().into_owned()),
            Err(e) => Err(pyo3::exceptions::PyValueError::new_err(e.to_string())),
        }
    }

    fn __repr__(&self) -> String {
        format!("WorkspaceRoots(roots={:?})", self.inner.roots())
    }
}

/// Register all Python classes in this module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyMcpHttpConfig>()?;
    m.add_class::<PyMcpHttpServer>()?;
    m.add_class::<PyServerHandle>()?;
    m.add_class::<PyBridgeContext>()?;
    m.add_class::<PyBridgeRegistry>()?;
    m.add_class::<PyWorkspaceRoots>()?;
    m.add_function(wrap_pyfunction!(py_create_skill_server, m)?)?;
    m.add_function(wrap_pyfunction!(py_get_bridge_context, m)?)?;
    m.add_function(wrap_pyfunction!(py_register_bridge, m)?)?;
    Ok(())
}

/// Create a pre-configured `McpHttpServer` for a specific DCC application.
///
/// This is the recommended entry-point for the **Skills-First** workflow.
/// It automatically:
///
/// 1. Creates an `ActionRegistry` and `ActionDispatcher`.
/// 2. Creates a `SkillCatalog` wired to the dispatcher.
/// 3. Discovers skills from **both** env vars (per-app + global):
///    - ``DCC_MCP_{APP}_SKILL_PATHS`` — e.g. ``DCC_MCP_MAYA_SKILL_PATHS``
///    - ``DCC_MCP_SKILL_PATHS`` — global fallback
/// 4. Returns a ready-to-start ``McpHttpServer``.
///
/// Args:
///     app_name: DCC application name (e.g. ``"maya"``, ``"blender"``).
///               Used to derive the per-app env var and as the MCP server name.
///     config:   Optional ``McpHttpConfig``; defaults to port 8765.
///     extra_paths: Extra skill directories to scan in addition to env var paths.
///     dcc_name: Override the DCC filter for skill scanning (defaults to ``app_name``).
///
/// Example::
///
///     import os
///     os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"
///
///     from dcc_mcp_core import create_skill_manager, McpHttpConfig
///
///     server = create_skill_manager("maya", McpHttpConfig(port=8765))
///     handle = server.start()
///     print(f"Maya MCP server at {handle.mcp_url()}")
///     # Agents connect, call find_skills() and load_skill() to discover tools.
///
/// .. note::
///
///     The returned server's ``SkillCatalog`` is pre-populated with discovered
///     skills but none are *loaded* yet. Use ``server.load_skill(name)`` or
///     the ``load_skill`` MCP tool to load skills on demand.
#[pyfunction]
#[pyo3(name = "create_skill_server")]
#[pyo3(signature = (app_name, config=None, extra_paths=None, dcc_name=None))]
pub fn py_create_skill_server(
    app_name: &str,
    config: Option<&PyMcpHttpConfig>,
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> PyResult<PyMcpHttpServer> {
    use dcc_mcp_utils::filesystem::get_app_skill_paths_from_env;

    // Determine DCC filter — default to app_name
    let effective_dcc = dcc_name.unwrap_or(app_name);

    // Build config with app_name as default server name
    let mut cfg = config.map(|c| c.inner.clone()).unwrap_or_default();
    if cfg.server_name == "dcc-mcp-server" || cfg.server_name.is_empty() {
        cfg.server_name = format!("{app_name}-mcp");
    }
    // Issue #303: force Dedicated mode for PyO3 callers, which matches
    // what PyMcpHttpConfig's constructor picks when called from Python.
    // Callers that really know what they're doing (i.e. running inside a
    // persistent #[tokio::main] driver) can still set spawn_mode back to
    // "ambient" on the config before passing it in.
    if matches!(cfg.spawn_mode, crate::config::ServerSpawnMode::Ambient) {
        cfg.spawn_mode = crate::config::ServerSpawnMode::Dedicated;
    }

    let runtime =
        Runtime::new().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

    let reg = Arc::new(ActionRegistry::new());
    let dispatcher = Arc::new(ActionDispatcher::new((*reg).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        reg.clone(),
        dispatcher.clone(),
    ));

    // Collect paths: explicit extra_paths + per-app env var + global env var
    let mut all_paths: Vec<String> = extra_paths.unwrap_or_default();
    all_paths.extend(get_app_skill_paths_from_env(app_name));
    let discover_paths = if all_paths.is_empty() {
        None
    } else {
        Some(all_paths)
    };

    // Discover skills (lenient — missing deps are skipped, not errors)
    let discovered = catalog.discover(discover_paths.as_deref(), Some(effective_dcc));
    tracing::info!("create_skill_server({app_name}): discovered {discovered} skill(s)");

    let live_meta = Arc::new(RwLock::new(LiveMetaInner {
        scene: cfg.scene.clone(),
        version: cfg.dcc_version.clone(),
        ..Default::default()
    }));
    Ok(PyMcpHttpServer {
        registry: reg,
        dispatcher,
        catalog,
        config: cfg,
        runtime: Arc::new(runtime),
        live_meta,
    })
}

/// Global bridge context registry (for gateway mode).
///
/// This singleton stores bridge connections that skill scripts can query.
use std::sync::OnceLock;
static BRIDGE_REGISTRY: OnceLock<crate::BridgeRegistry> = OnceLock::new();

// ── PyBridgeContext ──────────────────────────────────────────────────────

/// Python-facing bridge connection context.
///
/// Example::
///
///     from dcc_mcp_core import get_bridge_context, register_bridge
///
///     register_bridge("photoshop", "ws://localhost:9001")
///     ctx = get_bridge_context("photoshop")
///     if ctx:
///         print(ctx.dcc_type, ctx.bridge_url, ctx.connected)
#[pyclass(name = "BridgeContext", get_all, skip_from_py_object)]
#[derive(Debug, Clone)]
pub struct PyBridgeContext {
    pub dcc_type: String,
    pub bridge_url: String,
    pub connected: bool,
}

#[pymethods]
impl PyBridgeContext {
    fn __repr__(&self) -> String {
        format!(
            "BridgeContext(dcc_type={}, url={}, connected={})",
            self.dcc_type, self.bridge_url, self.connected
        )
    }
}

impl From<crate::BridgeContext> for PyBridgeContext {
    fn from(ctx: crate::BridgeContext) -> Self {
        Self {
            dcc_type: ctx.dcc_type,
            bridge_url: ctx.bridge_url,
            connected: ctx.connected,
        }
    }
}

// ── PyBridgeRegistry ─────────────────────────────────────────────────────

/// Python-facing bridge connection registry.
///
/// Thread-safe registry for bridge connections available in gateway mode.
/// Bridge plugins register their connection info, and skill scripts query
/// it to discover available bridges.
///
/// Example::
///
///     from dcc_mcp_core import BridgeRegistry
///
///     registry = BridgeRegistry()
///     registry.register("photoshop", "ws://localhost:9001")
///     registry.register("zbrush", "http://localhost:8765")
///
///     ctx = registry.get("photoshop")
///     print(ctx.bridge_url, ctx.connected)
///
///     for ctx in registry.list_all():
///         print(ctx.dcc_type, ctx.connected)
///
///     registry.set_disconnected("photoshop")
///     registry.unregister("zbrush")
#[pyclass(name = "BridgeRegistry", skip_from_py_object)]
#[derive(Debug, Clone)]
pub struct PyBridgeRegistry {
    inner: crate::BridgeRegistry,
}

#[pymethods]
impl PyBridgeRegistry {
    #[new]
    fn new() -> Self {
        Self {
            inner: crate::BridgeRegistry::new(),
        }
    }

    /// Register or update a bridge connection.
    ///
    /// Args:
    ///     dcc_type: DCC type identifier (e.g., ``"photoshop"``).
    ///     url: Bridge endpoint URL (e.g., ``"ws://localhost:9001"``).
    ///
    /// Raises:
    ///     ValueError: If ``dcc_type`` or ``url`` is empty.
    fn register(&self, dcc_type: String, url: String) -> PyResult<()> {
        self.inner
            .register(dcc_type, url)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Get bridge context for a specific DCC type.
    ///
    /// Returns ``None`` if no bridge is registered for the given DCC type.
    fn get(&self, dcc_type: &str) -> Option<PyBridgeContext> {
        self.inner.get(dcc_type).map(PyBridgeContext::from)
    }

    /// Get bridge URL for a specific DCC type (convenience method).
    ///
    /// Returns ``None`` if no bridge is registered.
    fn get_url(&self, dcc_type: &str) -> Option<String> {
        self.inner.get_url(dcc_type)
    }

    /// List all registered bridges.
    fn list_all(&self) -> Vec<PyBridgeContext> {
        self.inner
            .list_all()
            .into_iter()
            .map(PyBridgeContext::from)
            .collect()
    }

    /// Mark a bridge as disconnected without removing it from the registry.
    ///
    /// Raises:
    ///     ValueError: If the bridge is not found.
    fn set_disconnected(&self, dcc_type: &str) -> PyResult<()> {
        self.inner
            .set_disconnected(dcc_type)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Remove a bridge from the registry.
    ///
    /// Raises:
    ///     ValueError: If the bridge is not found.
    fn unregister(&self, dcc_type: &str) -> PyResult<()> {
        self.inner
            .unregister(dcc_type)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Clear all registered bridges.
    fn clear(&self) {
        self.inner.clear();
    }

    /// Check if a bridge is registered for the given DCC type.
    fn contains(&self, dcc_type: &str) -> bool {
        self.inner.contains(dcc_type)
    }

    /// Get the number of registered bridges.
    fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if the registry is empty.
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn __repr__(&self) -> String {
        format!("BridgeRegistry(count={})", self.inner.len())
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }
}

// ── Global bridge functions ──────────────────────────────────────────────

/// Get bridge context for a specific DCC type.
///
/// In gateway mode, external bridge plugins register their connection info
/// via :func:`register_bridge`, allowing skill scripts to access bridges from
/// other processes.
///
/// Args:
///     dcc_type: DCC type identifier (e.g., ``"photoshop"``, ``"zbrush"``).
///
/// Returns:
///     A :class:`BridgeContext` if registered, or ``None``.
///
/// Example::
///
///     from dcc_mcp_core import get_bridge_context, register_bridge
///
///     register_bridge("photoshop", "ws://localhost:9001")
///     ctx = get_bridge_context("photoshop")
///     if ctx:
///         print(ctx.bridge_url, ctx.connected)
///     else:
///         raise PhotoshopNotAvailableError("Bridge not connected")
#[pyfunction]
#[pyo3(name = "get_bridge_context")]
pub fn py_get_bridge_context(dcc_type: &str) -> Option<PyBridgeContext> {
    let registry = BRIDGE_REGISTRY.get_or_init(crate::BridgeRegistry::new);
    registry.get(dcc_type).map(PyBridgeContext::from)
}

/// Register a bridge connection in the global registry.
///
/// Called by bridge plugins to register their connection info so that
/// skill scripts can discover and use them via :func:`get_bridge_context`.
///
/// Args:
///     dcc_type: DCC type identifier (e.g., ``"photoshop"``).
///     url: Bridge endpoint URL (e.g., ``"ws://localhost:9001"``).
///
/// Raises:
///     ValueError: If ``dcc_type`` or ``url`` is empty.
///
/// Example::
///
///     from dcc_mcp_core import register_bridge
///
///     register_bridge("photoshop", "ws://localhost:9001")
///     register_bridge("zbrush", "http://localhost:8765")
#[pyfunction]
#[pyo3(name = "register_bridge")]
pub fn py_register_bridge(dcc_type: String, url: String) -> PyResult<()> {
    let registry = BRIDGE_REGISTRY.get_or_init(crate::BridgeRegistry::new);
    registry
        .register(dcc_type, url)
        .map_err(pyo3::exceptions::PyValueError::new_err)
}

/// Register a bridge connection (internal/gateway use).
///
/// Called by bridge plugins to register their connection info.
#[doc(hidden)]
pub fn register_bridge_internal(dcc_type: String, url: String) -> Result<(), String> {
    let registry = BRIDGE_REGISTRY.get_or_init(crate::BridgeRegistry::new);
    registry.register(dcc_type, url)
}
