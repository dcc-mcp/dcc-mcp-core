//! Python-visible MCP HTTP server configuration.

use super::*;
use dcc_mcp_pybridge::derive::PyWrapper;
use std::collections::HashMap;

/// Python-visible MCP HTTP server configuration.
///
/// Example::
///
///     from dcc_mcp_core import McpHttpConfig
///     config = McpHttpConfig(port=8765, server_name="my-dcc")
///
/// Most accessors are emitted by [`PyWrapper`](dcc_mcp_pybridge::derive::PyWrapper)
/// from the `#[py_wrapper(...)]` table below (issue #528 M3.2). Three
/// accessor families remain hand-written because they require non-trivial
/// conversions the macro grammar cannot express:
///
/// - `spawn_mode` / `set_spawn_mode` — enum ↔ `&str` with `PyResult` validation.
/// - `job_recovery` / `set_job_recovery` — same shape as `spawn_mode` (#567).
/// - `job_storage_path` / `set_job_storage_path` — `Option<PathBuf>` ↔ `Option<String>`.
/// - `registry_dir` / `set_registry_dir` — same `PathBuf` shape as above.
/// - `__repr__` — selects three identifying fields with one renamed
///   (`server_name` → `name`) via the shared `repr_pairs!` helper.
#[pyclass(name = "McpHttpConfig", skip_from_py_object)]
#[derive(Clone, PyWrapper)]
#[py_wrapper(
    inner = "McpHttpConfig",
    fields(
        // ── Core server (read-only after construction) ───────────────────
        port: u16 => [get],
        host: String => [get(to_string)],
        endpoint_path: String => [get(by_str)],
        server_name: String => [get(by_str)],
        server_version: String => [get(by_str)],
        max_sessions: usize => [get],
        request_timeout_ms: u64 => [get],
        enable_cors: bool => [get],

        // ── Sessions ─────────────────────────────────────────────────────
        /// Idle session TTL in seconds. Sessions not touched within this window
        /// are automatically evicted. Default: 3600 (1 hour). Set to 0 to disable.
        session_ttl_secs: u64 => [get, set],

        // ── Prometheus (issue #331) ──────────────────────────────────────
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
        enable_prometheus: bool => [get, set],

        /// Optional HTTP Basic auth for ``/metrics`` (issue #331).
        ///
        /// Tuple of ``(username, password)`` or ``None``. When set,
        /// scrapers must present a matching
        /// ``Authorization: Basic base64(user:pass)`` header or the
        /// endpoint responds with ``401 Unauthorized``. ``None`` leaves
        /// the endpoint open — appropriate for localhost-only dev, but
        /// configure credentials for anything exposed beyond that.
        prometheus_basic_auth: Option<(String, String)> => [get(clone), set],

        // ── Feature toggles ──────────────────────────────────────────────
        /// Enable the opt-in lazy-actions fast-path (#254).
        ///
        /// When ``True``, ``tools/list`` also surfaces three meta-tools:
        /// ``list_actions``, ``describe_action`` and ``call_action``. Useful
        /// for agents whose context budget cannot afford a full ``tools/list``
        /// paging session. Default: ``False``.
        lazy_actions: bool => [get, set],

        /// Enable the built-in ``workflows.*`` tools (issue #348).
        ///
        /// Default: ``False``. Step execution is stubbed in the skeleton —
        /// see :class:`WorkflowSpec` for the parse/validate surface that is
        /// already usable.
        enable_workflows: bool => [get, set],

        /// Best-effort safety net for Python callers that drop a
        /// ``McpServerHandle`` without explicitly calling ``shutdown()``.
        /// Default: ``False``. Prefer ``with server.start() as handle`` or
        /// explicit ``handle.shutdown()`` for deterministic shutdown.
        shutdown_on_drop: bool => [get, set],

        /// Emit the ``$/dcc.jobUpdated`` and ``$/dcc.workflowUpdated`` SSE
        /// channels (issue #326).
        ///
        /// Default: ``True``. When ``False``, the server still emits the
        /// spec-mandated ``notifications/progress`` channel for callers that
        /// supplied ``_meta.progressToken``, but the ``$/dcc.*`` vendor
        /// extensions are suppressed.
        enable_job_notifications: bool => [get, set],

        // ── Gateway configuration ────────────────────────────────────────
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
        gateway_port: u16 => [get, set],

        /// Seconds without heartbeat before an instance is stale. Default: 30.
        stale_timeout_secs: u64 => [get, set],

        /// Heartbeat interval in seconds. ``0`` disables heartbeat. Default: 5.
        heartbeat_secs: u64 => [get, set],

        // ── Instance registration metadata ───────────────────────────────
        /// DCC application type (e.g. ``"maya"``). Reported in the shared registry.
        dcc_type: Option<String> => [get(clone), set],

        /// DCC application version (e.g. ``"2025.1"``).
        dcc_version: Option<String> => [get(clone), set],

        /// Currently open scene/file. Improves routing accuracy.
        scene: Option<String> => [get(clone), set],

        /// Self-probe timeout in milliseconds. 0 disables the probe.
        /// Default: 200. Issue #303 guard.
        self_probe_timeout_ms: u64 => [get, set],

        /// Publish skill-scoped tools under their **bare action name** when no
        /// collision exists on this instance (#307).
        ///
        /// When ``True`` (default), ``tools/list`` emits ``execute_python``
        /// rather than ``maya-scripting.execute_python`` whenever the bare name
        /// is unique within the instance's loaded skills. Collisions fall back
        /// to the full ``<skill>.<action>`` form, and ``tools/call`` accepts
        /// both shapes for one release cycle.
        bare_tool_names: bool => [get, set],

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
        declared_capabilities: Vec<String> => [get(clone), set],

        // ── Gateway timeouts (#314, #321, #322) ──────────────────────────
        /// Per-backend gateway fan-out timeout in milliseconds (issue #314).
        ///
        /// Default: ``10_000`` (10 seconds). Raise this for DCC workflows that
        /// legitimately run backend tools longer than 10 seconds (scene import,
        /// simulation bake, large USD composition) to avoid spurious transport
        /// timeout errors on the gateway fan-out path.
        backend_timeout_ms: u64 => [get, set],

        /// Gateway timeout (ms) for async-dispatch `tools/call` requests
        /// (issue #321). Default: ``60_000``.
        ///
        /// Applies when the outbound call carries ``_meta.dcc.async == true``,
        /// a ``_meta.progressToken``, or targets a tool whose ``ActionMeta``
        /// declares ``execution: async`` / a ``timeout_hint_secs``. Only the
        /// **queuing** step uses this budget — the backend replies with
        /// ``{status: "pending"}`` as soon as the job is enqueued.
        gateway_async_dispatch_timeout_ms: u64 => [get, set],

        /// Gateway timeout (ms) for the opt-in wait-for-terminal passthrough
        /// mode (issue #321). Default: ``600_000`` (10 minutes).
        ///
        /// When the client sets ``_meta.dcc.wait_for_terminal = true`` along
        /// with an async opt-in, the gateway blocks the ``tools/call``
        /// response until a ``$/dcc.jobUpdated`` with a terminal status
        /// arrives. On timeout the gateway returns the last known status
        /// with ``_meta.dcc.timed_out = true`` and leaves the job running
        /// on the backend.
        gateway_wait_terminal_timeout_ms: u64 => [get, set],

        /// Gateway routing-cache TTL (seconds) for `JobRoute` entries
        /// (issue #322). Default: ``86_400`` (24 hours).
        ///
        /// Routes that don't see a terminal notification within this window
        /// are evicted by a background GC task so the cache cannot grow
        /// without bound under pathological agents or crashed backends.
        gateway_route_ttl_secs: u64 => [get, set],

        /// Per-session ceiling on concurrent live gateway routes (issue
        /// #322). ``0`` disables the cap. Default: ``1_000``.
        ///
        /// When a client session is already holding this many live routes,
        /// new async ``tools/call`` requests are rejected with JSON-RPC
        /// ``-32005 too_many_in_flight_jobs``.
        gateway_max_routes_per_session: u64 => [get, set],

        // ── MCP primitives (#350, #349, #351, #355) ──────────────────────
        /// Advertise the MCP Resources primitive (issue #350).
        ///
        /// When ``True`` (default), the server advertises
        /// ``resources: { subscribe, listChanged }`` in its ``initialize``
        /// response and handles ``resources/list`` / ``resources/read`` /
        /// ``resources/subscribe`` / ``resources/unsubscribe``. Built-in
        /// producers surface ``scene://current`` (JSON), ``audit://recent``
        /// (JSON) and ``capture://current_window`` (PNG, when a real window
        /// backend is available).
        enable_resources: bool => [get, set],

        /// Expose ``artefact://`` resources (issue #349).
        ///
        /// Default ``False``. The full artefact store lands in issue #349;
        /// this flag merely gates whether the ``artefact://`` scheme appears
        /// in ``resources/list`` and whether reads return a descriptive
        /// ``-32002`` error versus a normal not-found.
        enable_artefact_resources: bool => [get, set],

        /// Advertise the MCP Prompts primitive (issues #351, #355).
        ///
        /// When ``True`` (default), the server advertises
        /// ``prompts: { listChanged }`` in its ``initialize`` response and
        /// handles ``prompts/list`` + ``prompts/get``. Prompts are sourced
        /// from each loaded skill's sibling ``prompts.yaml`` (pointed at by
        /// ``metadata.dcc-mcp.prompts`` in SKILL.md) plus workflow-derived
        /// auto-generated entries.
        enable_prompts: bool => [get, set],

        /// Enable connection-scoped tool-list caching (issue #438).
        ///
        /// When ``True`` (default), ``tools/list`` stores a per-session
        /// snapshot of the full tool list. On subsequent ``tools/list``
        /// calls within the same session, if the registry has not changed
        /// (no skill load/unload, no group activation/deactivation), the
        /// cached snapshot is returned directly — avoiding redundant
        /// registry scans and tool-construction overhead.
        ///
        /// The cache is automatically invalidated when:
        /// - A skill is loaded or unloaded
        /// - A tool group is activated or deactivated
        /// - The session is evicted (TTL expiry)
        /// - The client sends ``tools/list`` with ``_meta.dcc.refresh = true``
        ///
        /// Set to ``False`` to disable caching (every ``tools/list`` call
        /// rebuilds the full list from scratch). Useful for debugging or
        /// when tool definitions are mutated externally.
        enable_tool_cache: bool => [get, set],
    ),
)]
pub struct PyMcpHttpConfig {
    pub(crate) inner: McpHttpConfig,
}

#[pymethods]
impl PyMcpHttpConfig {
    /// Create a new config. ``port=0`` binds to any available port.
    #[new]
    #[pyo3(signature = (port=8765, server_name=None, server_version=None, enable_cors=false, request_timeout_ms=30000, backend_timeout_ms=120_000, enable_prometheus=false, prometheus_basic_auth=None, gateway_async_dispatch_timeout_ms=60_000, gateway_wait_terminal_timeout_ms=600_000, gateway_route_ttl_secs=86_400, gateway_max_routes_per_session=1_000, shutdown_on_drop=false))]
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
        shutdown_on_drop: bool,
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
        cfg.shutdown_on_drop = shutdown_on_drop;
        // Issue #303: PyO3-embedded hosts (Maya on Windows etc.) cannot
        // rely on shared tokio worker threads to drive the accept loop
        // after `block_on` returns. Default to `Dedicated` so the listener
        // runs on its own OS thread owning a `current_thread` runtime.
        cfg.spawn_mode = ServerSpawnMode::Dedicated;
        Self { inner: cfg }
    }

    // All trivial getters/setters are emitted by `#[derive(PyWrapper)]`
    // via the `#[py_wrapper(...)]` table on the struct above (issue #528
    // M3.2). Only conversions the macro grammar cannot express remain
    // hand-written here:
    //
    //  - `spawn_mode` / `set_spawn_mode` — `&str` ↔ enum with `PyResult`.
    //  - `job_storage_path` / `set_job_storage_path` — `Option<PathBuf>`
    //    ↔ `Option<String>` (lossy round-trip).
    //  - `registry_dir` / `set_registry_dir` — same shape as above.
    //  - `__repr__` — selects three identifying fields with one renamed
    //    (`server_name` → `name`) via the shared `repr_pairs!` helper.

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

    /// Arbitrary FileRegistry metadata for this running instance.
    ///
    /// Rez launchers commonly set context fields such as ``context_bundle``,
    /// ``production_domain``, ``context_kind``, ``project``, ``task``,
    /// ``toolset_profile`` and ``package_provenance`` so gateway discovery can
    /// route by the resolved package context.
    #[getter]
    fn instance_metadata(&self) -> HashMap<String, String> {
        self.inner.instance_metadata.clone()
    }

    #[setter]
    fn set_instance_metadata(&mut self, metadata: HashMap<String, String>) {
        self.inner.instance_metadata = metadata;
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

    /// Gateway tool-exposure mode (issue #652).
    ///
    /// Returns one of ``"full" | "slim" | "both" | "rest"``. See
    /// :attr:`set_gateway_tool_exposure` for the meaning of each value.
    #[getter]
    fn gateway_tool_exposure(&self) -> &'static str {
        self.inner.gateway_tool_exposure.as_str()
    }

    /// Set the gateway tool-exposure mode (issue #652).
    ///
    /// Accepts ``"full" | "slim" | "both" | "rest"`` (case-insensitive).
    /// Unknown values raise ``ValueError`` instead of silently falling
    /// back so configuration typos are surfaced immediately.
    ///
    /// * ``"full"`` — publish every live backend tool through
    ///   ``tools/list`` (legacy behaviour; default).
    /// * ``"slim"`` — only gateway meta-tools + skill management are
    ///   visible; backend capabilities reached via dynamic wrappers.
    /// * ``"both"`` — alias of ``"full"`` today, reserved for the
    ///   transition window once dynamic wrapper tools land (#657).
    /// * ``"rest"`` — same bounded surface as ``"slim"``; signals that
    ///   REST is the canonical capability API.
    #[setter]
    fn set_gateway_tool_exposure(&mut self, mode: &str) -> PyResult<()> {
        self.inner.gateway_tool_exposure =
            mode.parse()
                .map_err(|e: crate::gateway::ParseGatewayToolExposureError| {
                    pyo3::exceptions::PyValueError::new_err(e.to_string())
                })?;
        Ok(())
    }

    /// Whether the gateway emits Cursor-safe tool names (#656).
    ///
    /// When ``True`` (the default), the gateway publishes tool names
    /// of the form ``i_<id8>__<escaped_tool>`` that contain only
    /// ``[A-Za-z0-9_]``. When ``False``, the gateway falls back to the
    /// pre-#656 SEP-986 dotted form ``<id8>.<tool>``.
    #[getter]
    fn gateway_cursor_safe_tool_names(&self) -> bool {
        self.inner.gateway_cursor_safe_tool_names
    }

    /// Enable or disable Cursor-safe gateway tool names (#656).
    ///
    /// Flip to ``False`` only when you need diagnostic parity with a
    /// single-instance server that publishes SEP-986 dotted names
    /// directly. Cursor and several other MCP clients silently hide
    /// any tool name containing ``.`` or ``-`` from the agent, so
    /// leaving this ``True`` is strongly recommended.
    #[setter]
    fn set_gateway_cursor_safe_tool_names(&mut self, enabled: bool) {
        self.inner.gateway_cursor_safe_tool_names = enabled;
    }

    /// Lower-case wire identifier of the configured job-recovery policy
    /// (issue #567). Returns ``"drop"`` (default) or ``"requeue"``.
    ///
    /// ``"drop"`` rewrites every ``Pending`` / ``Running`` row left over
    /// by a previous process to ``Interrupted`` on startup.
    /// ``"requeue"`` is reserved for a future release that persists tool
    /// arguments alongside the job row; today it is accepted but
    /// degrades to ``"drop"`` semantics with a ``WARN`` log so adapters
    /// can plumb the knob through now.
    #[getter]
    fn job_recovery(&self) -> &'static str {
        self.inner.job_recovery.as_str()
    }

    #[setter]
    fn set_job_recovery(&mut self, policy: &str) -> PyResult<()> {
        self.inner.job_recovery = crate::config::JobRecoveryPolicy::parse(policy)
            .map_err(pyo3::exceptions::PyValueError::new_err)?;
        Ok(())
    }

    fn __repr__(&self) -> String {
        dcc_mcp_pybridge::repr_pairs!(
            "McpHttpConfig",
            [
                ("port", self.inner.port),
                ("name", self.inner.server_name),
                ("gateway_port", self.inner.gateway_port),
            ]
        )
    }
}

// ── Drift-detection tests ─────────────────────────────────────────────────────
//
// Every field in `McpHttpConfig` that Python callers should be able to read
// must have a matching getter on `PyMcpHttpConfig`. Most are emitted by
// `#[derive(PyWrapper)]` from the `#[py_wrapper(...)]` table on the struct
// (see #528 M3.2); the remainder are hand-written in the `#[pymethods]`
// block.
//
// When you add a new field:
//   1. Add it to the `fields(...)` list in `#[py_wrapper(...)]`, **or** add
//      a hand-written `#[getter]` if the conversion is non-trivial.
//   2. Add `let _ = cfg.field_name();` to the test below.
//
// The test fails to **compile** if a getter is removed — that is the intended
// safety net against silent drift between the Rust config and the Python API.
#[cfg(test)]
mod drift_tests {
    use super::*;
    use crate::config::McpHttpConfig;

    fn default_cfg() -> PyMcpHttpConfig {
        PyMcpHttpConfig {
            inner: McpHttpConfig::default(),
        }
    }

    #[test]
    fn all_mcp_http_config_fields_have_py_getters() {
        let cfg = default_cfg();

        // ── Core server ────────────────────────────────────────────────────
        let _ = cfg.port();
        let _ = cfg.host();
        let _ = cfg.endpoint_path();
        let _ = cfg.server_name();
        let _ = cfg.server_version();
        let _ = cfg.max_sessions();
        let _ = cfg.request_timeout_ms();
        let _ = cfg.enable_cors();
        let _ = cfg.session_ttl_secs();
        let _ = cfg.spawn_mode();
        let _ = cfg.self_probe_timeout_ms();
        let _ = cfg.backend_timeout_ms();
        let _ = cfg.bare_tool_names();
        let _ = cfg.declared_capabilities();
        let _ = cfg.enable_tool_cache();

        // ── Features ───────────────────────────────────────────────────────
        let _ = cfg.lazy_actions();
        let _ = cfg.enable_workflows();
        let _ = cfg.enable_job_notifications();
        let _ = cfg.shutdown_on_drop();
        let _ = cfg.job_storage_path();
        let _ = cfg.job_recovery();

        // ── Prometheus ─────────────────────────────────────────────────────
        let _ = cfg.enable_prometheus();
        let _ = cfg.prometheus_basic_auth();

        // ── Gateway ────────────────────────────────────────────────────────
        let _ = cfg.gateway_port();
        let _ = cfg.registry_dir();
        let _ = cfg.stale_timeout_secs();
        let _ = cfg.heartbeat_secs();
        let _ = cfg.gateway_async_dispatch_timeout_ms();
        let _ = cfg.gateway_wait_terminal_timeout_ms();
        let _ = cfg.gateway_route_ttl_secs();
        let _ = cfg.gateway_max_routes_per_session();

        // ── Instance registration metadata ─────────────────────────────────
        let _ = cfg.dcc_type();
        let _ = cfg.dcc_version();
        let _ = cfg.scene();
        let _ = cfg.instance_metadata();

        // ── MCP primitives ─────────────────────────────────────────────────
        let _ = cfg.enable_resources();
        let _ = cfg.enable_artefact_resources();
        let _ = cfg.enable_prompts();
    }

    #[test]
    fn repr_contains_port() {
        let cfg = PyMcpHttpConfig {
            inner: McpHttpConfig::new(1234),
        };
        assert!(cfg.__repr__().contains("1234"));
    }

    #[test]
    fn repr_contains_class_name() {
        let cfg = default_cfg();
        assert!(cfg.__repr__().contains("McpHttpConfig"));
    }
}
