//! Python-visible MCP HTTP server configuration.

use super::*;

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
        cfg.spawn_mode = ServerSpawnMode::Dedicated;
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
            ServerSpawnMode::Ambient => "ambient",
            ServerSpawnMode::Dedicated => "dedicated",
        }
    }

    #[setter]
    fn set_spawn_mode(&mut self, mode: &str) -> PyResult<()> {
        self.inner.spawn_mode = match mode {
            "ambient" => ServerSpawnMode::Ambient,
            "dedicated" => ServerSpawnMode::Dedicated,
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
