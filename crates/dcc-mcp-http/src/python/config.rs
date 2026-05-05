//! Python-visible MCP HTTP server configuration.

use super::*;
use std::collections::HashMap;

/// Python-visible MCP HTTP server configuration.
///
/// Example::
///
///     from dcc_mcp_core import McpHttpConfig
///     config = McpHttpConfig(port=8765, server_name="my-dcc")
///
/// All accessors are now **hand-written** because the `PyWrapper` macro
/// generates direct field access (`self.inner.port`) which no longer
/// compiles after `McpHttpConfig` was split into 9 sub-config structs
/// (issue #764). Each hand-written getter/setter accesses the correct
/// nested field path (e.g., `self.inner.server.port`).
///
/// Three accessor families use non-trivial conversions and remain
/// hand-written (see commented markers in the `#[pymethods]` block):
///
/// - `spawn_mode` / `set_spawn_mode` — enum ↔ `&str` with `PyResult` validation.
/// - `job_recovery` / `set_job_recovery` — same shape as `spawn_mode` (#567).
/// - `job_storage_path` / `set_job_storage_path` — `Option<PathBuf>` ↔ `Option<String>`.
/// - `registry_dir` / `set_registry_dir` — same `PathBuf` shape as above.
/// - `instance_metadata` / `set_instance_metadata` — `HashMap<String, String>` clone.
/// - `gateway_cursor_safe_tool_names` / `set_gateway_cursor_safe_tool_names` — bool.
/// - `__repr__` — selects three identifying fields with one renamed
///   (`server_name` → `name`) via the shared `repr_pairs!` helper.
#[pyclass(name = "McpHttpConfig", skip_from_py_object)]
#[derive(Clone)]
pub struct PyMcpHttpConfig {
    pub(crate) inner: McpHttpConfig,
}

impl From<McpHttpConfig> for PyMcpHttpConfig {
    fn from(inner: McpHttpConfig) -> Self {
        Self { inner }
    }
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
        let mut cfg = McpHttpConfig::default();
        cfg.server.port = port;
        if let Some(name) = server_name {
            cfg.server.server_name = name;
        }
        if let Some(ver) = server_version {
            cfg.server.server_version = ver;
        }
        cfg.server.enable_cors = enable_cors;
        cfg.server.request_timeout_ms = request_timeout_ms;
        cfg.gateway.backend_timeout_ms = backend_timeout_ms;
        cfg.telemetry.enable_prometheus = enable_prometheus;
        cfg.telemetry.prometheus_basic_auth = prometheus_basic_auth;
        cfg.gateway.gateway_async_dispatch_timeout_ms = gateway_async_dispatch_timeout_ms;
        cfg.gateway.gateway_wait_terminal_timeout_ms = gateway_wait_terminal_timeout_ms;
        cfg.gateway.gateway_route_ttl_secs = gateway_route_ttl_secs;
        cfg.gateway.gateway_max_routes_per_session = gateway_max_routes_per_session;
        cfg.features.shutdown_on_drop = shutdown_on_drop;
        // Issue #303: PyO3-embedded hosts (Maya on Windows etc.) cannot
        // rely on shared tokio worker threads to drive the accept loop
        // after `block_on` returns. Default to `Dedicated` so the listener
        // runs on its own OS thread owning a `current_thread` runtime.
        cfg.server.spawn_mode = ServerSpawnMode::Dedicated;
        Self { inner: cfg }
    }

    // ── ServerConfig getters (read-only) ─────────────────────────────

    /// Port the server listens on.
    #[getter]
    fn port(&self) -> u16 {
        self.inner.server.port
    }

    /// IP address the server binds to (always a string, e.g. ``"127.0.0.1"``).
    #[getter]
    fn host(&self) -> String {
        self.inner.server.host.to_string()
    }

    /// MCP endpoint path (default ``"/mcp"``).
    #[getter]
    fn endpoint_path(&self) -> String {
        self.inner.server.endpoint_path.clone()
    }

    /// Server name reported in ``initialize`` response.
    #[getter]
    fn server_name(&self) -> String {
        self.inner.server.server_name.clone()
    }

    /// Server version reported in ``initialize`` response.
    #[getter]
    fn server_version(&self) -> String {
        self.inner.server.server_version.clone()
    }

    /// Maximum concurrent SSE sessions.
    #[getter]
    fn max_sessions(&self) -> usize {
        self.inner.server.max_sessions
    }

    /// Request timeout in milliseconds.
    #[getter]
    fn request_timeout_ms(&self) -> u64 {
        self.inner.server.request_timeout_ms
    }

    /// Whether CORS is enabled for browser clients.
    #[getter]
    fn enable_cors(&self) -> bool {
        self.inner.server.enable_cors
    }

    // ── SessionConfig getters/setters ────────────────────────────────

    /// Idle session TTL in seconds. Set to ``0`` to disable.
    #[getter]
    fn session_ttl_secs(&self) -> u64 {
        self.inner.session.session_ttl_secs
    }

    /// Idle session TTL in seconds. Set to ``0`` to disable.
    #[setter]
    fn set_session_ttl_secs(&mut self, v: u64) {
        self.inner.session.session_ttl_secs = v;
    }

    /// Whether connection-scoped tool-list caching is enabled.
    #[getter]
    fn enable_tool_cache(&self) -> bool {
        self.inner.session.enable_tool_cache
    }

    /// Whether connection-scoped tool-list caching is enabled.
    #[setter]
    fn set_enable_tool_cache(&mut self, v: bool) {
        self.inner.session.enable_tool_cache = v;
    }

    // ── TelemetryConfig getters/setters ──────────────────────────────

    /// Enable the Prometheus ``/metrics`` endpoint (issue #331).
    #[getter]
    fn enable_prometheus(&self) -> bool {
        self.inner.telemetry.enable_prometheus
    }

    /// Enable the Prometheus ``/metrics`` endpoint (issue #331).
    #[setter]
    fn set_enable_prometheus(&mut self, v: bool) {
        self.inner.telemetry.enable_prometheus = v;
    }

    /// Optional HTTP Basic auth for ``/metrics`` as ``(username, password)``.
    #[getter]
    fn prometheus_basic_auth(&self) -> Option<(String, String)> {
        self.inner.telemetry.prometheus_basic_auth.clone()
    }

    /// Optional HTTP Basic auth for ``/metrics`` as ``(username, password)``.
    #[setter]
    fn set_prometheus_basic_auth(&mut self, v: Option<(String, String)>) {
        self.inner.telemetry.prometheus_basic_auth = v;
    }

    // ── FeatureFlags getters/setters ─────────────────────────────────

    /// Enable the opt-in lazy-actions fast-path (#254).
    #[getter]
    fn lazy_actions(&self) -> bool {
        self.inner.features.lazy_actions
    }

    /// Enable the opt-in lazy-actions fast-path (#254).
    #[setter]
    fn set_lazy_actions(&mut self, v: bool) {
        self.inner.features.lazy_actions = v;
    }

    /// Enable the built-in ``workflows.*`` tools (issue #348).
    #[getter]
    fn enable_workflows(&self) -> bool {
        self.inner.workflow.enable_workflows
    }

    /// Enable the built-in ``workflows.*`` tools (issue #348).
    #[setter]
    fn set_enable_workflows(&mut self, v: bool) {
        self.inner.workflow.enable_workflows = v;
    }

    /// Best-effort ``shutdown()`` on ``drop()``.
    #[getter]
    fn shutdown_on_drop(&self) -> bool {
        self.inner.features.shutdown_on_drop
    }

    /// Best-effort ``shutdown()`` on ``drop()``.
    #[setter]
    fn set_shutdown_on_drop(&mut self, v: bool) {
        self.inner.features.shutdown_on_drop = v;
    }

    /// Emit the ``$/dcc.jobUpdated`` SSE channel (issue #326).
    #[getter]
    fn enable_job_notifications(&self) -> bool {
        self.inner.features.enable_job_notifications
    }

    /// Emit the ``$/dcc.jobUpdated`` SSE channel (issue #326).
    #[setter]
    fn set_enable_job_notifications(&mut self, v: bool) {
        self.inner.features.enable_job_notifications = v;
    }

    /// Publish tools under their bare action name when unique (#307).
    #[getter]
    fn bare_tool_names(&self) -> bool {
        self.inner.features.bare_tool_names
    }

    /// Publish tools under their bare action name when unique (#307).
    #[setter]
    fn set_bare_tool_names(&mut self, v: bool) {
        self.inner.features.bare_tool_names = v;
    }

    /// Advertise the MCP Resources primitive (issue #350).
    #[getter]
    fn enable_resources(&self) -> bool {
        self.inner.features.enable_resources
    }

    /// Advertise the MCP Resources primitive (issue #350).
    #[setter]
    fn set_enable_resources(&mut self, v: bool) {
        self.inner.features.enable_resources = v;
    }

    /// Expose ``artefact://`` resources (issue #349).
    #[getter]
    fn enable_artefact_resources(&self) -> bool {
        self.inner.features.enable_artefact_resources
    }

    /// Expose ``artefact://`` resources (issue #349).
    #[setter]
    fn set_enable_artefact_resources(&mut self, v: bool) {
        self.inner.features.enable_artefact_resources = v;
    }

    /// Advertise the MCP Prompts primitive (issues #351, #355).
    #[getter]
    fn enable_prompts(&self) -> bool {
        self.inner.features.enable_prompts
    }

    /// Advertise the MCP Prompts primitive (issues #351, #355).
    #[setter]
    fn set_enable_prompts(&mut self, v: bool) {
        self.inner.features.enable_prompts = v;
    }

    // ── GatewayConfig getters/setters ───────────────────────────────

    /// Gateway port to compete for. ``0`` disables the gateway.
    #[getter]
    fn gateway_port(&self) -> u16 {
        self.inner.gateway.gateway_port
    }

    /// Gateway port to compete for. ``0`` disables the gateway.
    #[setter]
    fn set_gateway_port(&mut self, v: u16) {
        self.inner.gateway.gateway_port = v;
    }

    /// Seconds without heartbeat before an instance is stale.
    #[getter]
    fn stale_timeout_secs(&self) -> u64 {
        self.inner.gateway.stale_timeout_secs
    }

    /// Seconds without heartbeat before an instance is stale.
    #[setter]
    fn set_stale_timeout_secs(&mut self, v: u64) {
        self.inner.gateway.stale_timeout_secs = v;
    }

    /// Heartbeat interval in seconds. ``0`` disables.
    #[getter]
    fn heartbeat_secs(&self) -> u64 {
        self.inner.gateway.heartbeat_secs
    }

    /// Heartbeat interval in seconds. ``0`` disables.
    #[setter]
    fn set_heartbeat_secs(&mut self, v: u64) {
        self.inner.gateway.heartbeat_secs = v;
    }

    /// Per-backend gateway fan-out timeout in milliseconds (issue #314).
    #[getter]
    fn backend_timeout_ms(&self) -> u64 {
        self.inner.gateway.backend_timeout_ms
    }

    /// Per-backend gateway fan-out timeout in milliseconds (issue #314).
    #[setter]
    fn set_backend_timeout_ms(&mut self, v: u64) {
        self.inner.gateway.backend_timeout_ms = v;
    }

    /// Gateway timeout (ms) for async-dispatch ``tools/call`` (issue #321).
    #[getter]
    fn gateway_async_dispatch_timeout_ms(&self) -> u64 {
        self.inner.gateway.gateway_async_dispatch_timeout_ms
    }

    /// Gateway timeout (ms) for async-dispatch ``tools/call`` (issue #321).
    #[setter]
    fn set_gateway_async_dispatch_timeout_ms(&mut self, v: u64) {
        self.inner.gateway.gateway_async_dispatch_timeout_ms = v;
    }

    /// Gateway timeout (ms) for wait-for-terminal mode (issue #321).
    #[getter]
    fn gateway_wait_terminal_timeout_ms(&self) -> u64 {
        self.inner.gateway.gateway_wait_terminal_timeout_ms
    }

    /// Gateway timeout (ms) for wait-for-terminal mode (issue #321).
    #[setter]
    fn set_gateway_wait_terminal_timeout_ms(&mut self, v: u64) {
        self.inner.gateway.gateway_wait_terminal_timeout_ms = v;
    }

    /// Gateway routing-cache TTL (seconds, issue #322).
    #[getter]
    fn gateway_route_ttl_secs(&self) -> u64 {
        self.inner.gateway.gateway_route_ttl_secs
    }

    /// Gateway routing-cache TTL (seconds, issue #322).
    #[setter]
    fn set_gateway_route_ttl_secs(&mut self, v: u64) {
        self.inner.gateway.gateway_route_ttl_secs = v;
    }

    /// Per-session ceiling on concurrent live gateway routes (issue #322).
    #[getter]
    fn gateway_max_routes_per_session(&self) -> u64 {
        self.inner.gateway.gateway_max_routes_per_session
    }

    /// Per-session ceiling on concurrent live gateway routes (issue #322).
    #[setter]
    fn set_gateway_max_routes_per_session(&mut self, v: u64) {
        self.inner.gateway.gateway_max_routes_per_session = v;
    }

    /// Whether the gateway emits Cursor-safe prompt names (#656).
    #[getter]
    fn gateway_cursor_safe_tool_names(&self) -> bool {
        self.inner.gateway.gateway_cursor_safe_tool_names
    }

    /// Whether the gateway emits Cursor-safe prompt names (#656).
    #[setter]
    fn set_gateway_cursor_safe_tool_names(&mut self, v: bool) {
        self.inner.gateway.gateway_cursor_safe_tool_names = v;
    }

    /// Adapter package version for gateway election (issue maya#137).
    #[getter]
    fn adapter_version(&self) -> Option<String> {
        self.inner.gateway.adapter_version.clone()
    }

    /// Adapter package version for gateway election (issue maya#137).
    #[setter]
    fn set_adapter_version(&mut self, v: Option<String>) {
        self.inner.gateway.adapter_version = v;
    }

    /// DCC type the adapter is bound to for gateway election (issue maya#137).
    #[getter]
    fn adapter_dcc(&self) -> Option<String> {
        self.inner.gateway.adapter_dcc.clone()
    }

    /// DCC type the adapter is bound to for gateway election (issue maya#137).
    #[setter]
    fn set_adapter_dcc(&mut self, v: Option<String>) {
        self.inner.gateway.adapter_dcc = v;
    }

    /// Allow instances with ``dcc_type == "unknown"`` to expose tools (#555).
    #[getter]
    fn allow_unknown_tools(&self) -> bool {
        self.inner.gateway.allow_unknown_tools
    }

    /// Allow instances with ``dcc_type == "unknown"`` to expose tools (#555).
    #[setter]
    fn set_allow_unknown_tools(&mut self, v: bool) {
        self.inner.gateway.allow_unknown_tools = v;
    }

    // ── QueueConfig getters/setters ──────────────────────────────────

    /// Capacity of the HTTP → DccExecutor mpsc channel (issue #715).
    #[getter]
    fn deferred_queue_depth(&self) -> usize {
        self.inner.queue.deferred_queue_depth
    }

    /// Capacity of the HTTP → DccExecutor mpsc channel (issue #715).
    #[setter]
    fn set_deferred_queue_depth(&mut self, v: usize) {
        self.inner.queue.deferred_queue_depth = v;
    }

    /// Capacity of the DeferredExecutor → host_bridge mpsc channel (#715).
    #[getter]
    fn bridge_queue_depth(&self) -> usize {
        self.inner.queue.bridge_queue_depth
    }

    /// Capacity of the DeferredExecutor → host_bridge mpsc channel (#715).
    #[setter]
    fn set_bridge_queue_depth(&mut self, v: usize) {
        self.inner.queue.bridge_queue_depth = v;
    }

    /// Capacity of the host-side QueueDispatcher (#715). ``0`` = unbounded.
    #[getter]
    fn host_queue_depth(&self) -> usize {
        self.inner.queue.host_queue_depth
    }

    /// Capacity of the host-side QueueDispatcher (#715). ``0`` = unbounded.
    #[setter]
    fn set_host_queue_depth(&mut self, v: usize) {
        self.inner.queue.host_queue_depth = v;
    }

    /// How long an HTTP worker blocks on a full channel before erroring (#715).
    #[getter]
    fn queue_send_timeout_ms(&self) -> u64 {
        self.inner.queue.queue_send_timeout_ms
    }

    /// How long an HTTP worker blocks on a full channel before erroring (#715).
    #[setter]
    fn set_queue_send_timeout_ms(&mut self, v: u64) {
        self.inner.queue.queue_send_timeout_ms = v;
    }

    // ── InstanceConfig getters/setters ───────────────────────────────

    /// DCC application type (e.g. ``"maya"``).
    #[getter]
    fn dcc_type(&self) -> Option<String> {
        self.inner.instance.dcc_type.clone()
    }

    /// DCC application type (e.g. ``"maya"``).
    #[setter]
    fn set_dcc_type(&mut self, v: Option<String>) {
        self.inner.instance.dcc_type = v;
    }

    /// DCC application version (e.g. ``"2025.1"``).
    #[getter]
    fn dcc_version(&self) -> Option<String> {
        self.inner.instance.dcc_version.clone()
    }

    /// DCC application version (e.g. ``"2025.1"``).
    #[setter]
    fn set_dcc_version(&mut self, v: Option<String>) {
        self.inner.instance.dcc_version = v;
    }

    /// Currently open scene/file.
    #[getter]
    fn scene(&self) -> Option<String> {
        self.inner.instance.scene.clone()
    }

    /// Currently open scene/file.
    #[setter]
    fn set_scene(&mut self, v: Option<String>) {
        self.inner.instance.scene = v;
    }

    /// DCC capabilities this adapter provides (issue #354).
    #[getter]
    fn declared_capabilities(&self) -> Vec<String> {
        self.inner.instance.declared_capabilities.clone()
    }

    /// DCC capabilities this adapter provides (issue #354).
    #[setter]
    fn set_declared_capabilities(&mut self, v: Vec<String>) {
        self.inner.instance.declared_capabilities = v;
    }

    // ── WorkflowConfig getters/setters ──────────────────────────────

    /// Enable the built-in ``workflows.*`` tools (issue #348).
    #[getter]
    fn enable_scheduler(&self) -> bool {
        self.inner.workflow.enable_scheduler
    }

    /// Enable the built-in ``workflows.*`` tools (issue #348).
    #[setter]
    fn set_enable_scheduler(&mut self, v: bool) {
        self.inner.workflow.enable_scheduler = v;
    }

    /// Directory holding ``*.schedules.yaml`` files (issue #352).
    #[getter]
    fn schedules_dir(&self) -> Option<String> {
        self.inner
            .workflow
            .schedules_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    /// Directory holding ``*.schedules.yaml`` files (issue #352).
    #[setter]
    fn set_schedules_dir(&mut self, v: Option<String>) {
        self.inner.workflow.schedules_dir = v.map(std::path::PathBuf::from);
    }

    // ── Hand-written special accessors ──────────────────────────────

    /// Optional filesystem path to a SQLite database for job persistence (issue #328).
    #[getter]
    fn job_storage_path(&self) -> Option<String> {
        self.inner
            .job
            .job_storage_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    /// Optional filesystem path to a SQLite database for job persistence (issue #328).
    #[setter]
    fn set_job_storage_path(&mut self, path: Option<String>) {
        self.inner.job.job_storage_path = path.map(std::path::PathBuf::from);
    }

    /// Shared FileRegistry directory path. ``None`` uses a system temp dir.
    #[getter]
    fn registry_dir(&self) -> Option<String> {
        self.inner
            .gateway
            .registry_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    /// Shared FileRegistry directory path. ``None`` uses a system temp dir.
    #[setter]
    fn set_registry_dir(&mut self, dir: Option<String>) {
        self.inner.gateway.registry_dir = dir.map(std::path::PathBuf::from);
    }

    /// Arbitrary FileRegistry metadata for this running instance.
    #[getter]
    fn instance_metadata(&self) -> HashMap<String, String> {
        self.inner.instance.instance_metadata.clone()
    }

    /// Arbitrary FileRegistry metadata for this running instance.
    #[setter]
    fn set_instance_metadata(&mut self, metadata: HashMap<String, String>) {
        self.inner.instance.instance_metadata = metadata;
    }

    /// Listener spawn strategy (issue #303).
    #[getter]
    fn spawn_mode(&self) -> &'static str {
        match self.inner.server.spawn_mode {
            ServerSpawnMode::Ambient => "ambient",
            ServerSpawnMode::Dedicated => "dedicated",
        }
    }

    /// Listener spawn strategy (issue #303).
    #[setter]
    fn set_spawn_mode(&mut self, mode: &str) -> PyResult<()> {
        self.inner.server.spawn_mode = match mode {
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

    /// Self-probe timeout in milliseconds. ``0`` disables (issue #303).
    #[getter]
    fn self_probe_timeout_ms(&self) -> u64 {
        self.inner.server.self_probe_timeout_ms
    }

    /// Self-probe timeout in milliseconds. ``0`` disables (issue #303).
    #[setter]
    fn set_self_probe_timeout_ms(&mut self, v: u64) {
        self.inner.server.self_probe_timeout_ms = v;
    }

    /// Lower-case wire identifier of the configured job-recovery policy (issue #567).
    #[getter]
    fn job_recovery(&self) -> &'static str {
        self.inner.job.job_recovery.as_str()
    }

    /// Lower-case wire identifier of the configured job-recovery policy (issue #567).
    #[setter]
    fn set_job_recovery(&mut self, policy: &str) -> PyResult<()> {
        self.inner.job.job_recovery = crate::config::JobRecoveryPolicy::parse(policy)
            .map_err(pyo3::exceptions::PyValueError::new_err)?;
        Ok(())
    }

    // ── __repr__ ────────────────────────────────────────────────────

    fn __repr__(&self) -> String {
        dcc_mcp_pybridge::repr_pairs!(
            "McpHttpConfig",
            [
                ("port", self.inner.server.port),
                ("name", self.inner.server.server_name),
                ("gateway_port", self.inner.gateway.gateway_port),
            ]
        )
    }
}

// ── Drift-detection tests ─────────────────────────────────────────────────────
//
// Every field in `McpHttpConfig` that Python callers should be able to read
// must have a matching getter on `PyMcpHttpConfig`. All are now hand-written
// in the `#[pymethods]` block above.
//
// When you add a new field:
//   1. Add a hand-written getter (and setter if needed) in the `#[pymethods]` block.
//   2. Add `let _ = cfg.<getter>();` to the test below.
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

        // ── ServerConfig (read-only) ────────────────────────────────
        let _ = cfg.port();
        let _ = cfg.host();
        let _ = cfg.endpoint_path();
        let _ = cfg.server_name();
        let _ = cfg.server_version();
        let _ = cfg.max_sessions();
        let _ = cfg.request_timeout_ms();
        let _ = cfg.enable_cors();

        // ── ServerConfig (read-write, hand-written) ──────────────────
        let _ = cfg.spawn_mode();
        let _ = cfg.self_probe_timeout_ms();

        // ── SessionConfig ────────────────────────────────────────────
        let _ = cfg.session_ttl_secs();
        let _ = cfg.enable_tool_cache();

        // ── TelemetryConfig ──────────────────────────────────────────
        let _ = cfg.enable_prometheus();
        let _ = cfg.prometheus_basic_auth();

        // ── FeatureFlags ────────────────────────────────────────────
        let _ = cfg.lazy_actions();
        let _ = cfg.enable_workflows();
        let _ = cfg.shutdown_on_drop();
        let _ = cfg.enable_job_notifications();
        let _ = cfg.bare_tool_names();
        let _ = cfg.enable_resources();
        let _ = cfg.enable_artefact_resources();
        let _ = cfg.enable_prompts();

        // ── GatewayConfig ────────────────────────────────────────────
        let _ = cfg.gateway_port();
        let _ = cfg.registry_dir();
        let _ = cfg.stale_timeout_secs();
        let _ = cfg.heartbeat_secs();
        let _ = cfg.backend_timeout_ms();
        let _ = cfg.gateway_async_dispatch_timeout_ms();
        let _ = cfg.gateway_wait_terminal_timeout_ms();
        let _ = cfg.gateway_route_ttl_secs();
        let _ = cfg.gateway_max_routes_per_session();
        let _ = cfg.gateway_cursor_safe_tool_names();
        let _ = cfg.adapter_version();
        let _ = cfg.adapter_dcc();
        let _ = cfg.allow_unknown_tools();

        // ── QueueConfig ─────────────────────────────────────────────
        let _ = cfg.deferred_queue_depth();
        let _ = cfg.bridge_queue_depth();
        let _ = cfg.host_queue_depth();
        let _ = cfg.queue_send_timeout_ms();

        // ── InstanceConfig ──────────────────────────────────────────
        let _ = cfg.dcc_type();
        let _ = cfg.dcc_version();
        let _ = cfg.scene();
        let _ = cfg.instance_metadata();
        let _ = cfg.declared_capabilities();

        // ── WorkflowConfig ──────────────────────────────────────────
        let _ = cfg.enable_scheduler();
        let _ = cfg.schedules_dir();

        // ── JobConfig ───────────────────────────────────────────────
        let _ = cfg.job_storage_path();
        let _ = cfg.job_recovery();
    }

    #[test]
    fn repr_contains_port() {
        let cfg = PyMcpHttpConfig {
            inner: McpHttpConfig::default().with_port(1234),
        };
        assert!(cfg.__repr__().contains("1234"));
    }

    #[test]
    fn repr_contains_class_name() {
        let cfg = default_cfg();
        assert!(cfg.__repr__().contains("McpHttpConfig"));
    }
}
