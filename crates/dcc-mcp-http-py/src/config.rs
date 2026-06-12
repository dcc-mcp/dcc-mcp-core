//! Python-visible MCP HTTP server configuration.

use super::*;
use std::collections::HashMap;

mod build;

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
/// Non-trivial conversions (enum/path/map/repr) stay explicit in the
/// `#[pymethods]` block so Python errors remain precise.
#[pyclass(name = "McpHttpConfig", skip_from_py_object)]
pub struct PyMcpHttpConfig {
    pub(crate) inner: McpHttpConfig,
    /// Optional :class:`SandboxPolicy` forwarded to the in-process executor (issue #1001).
    sandbox_policy: Option<Py<PyAny>>,
}

impl Clone for PyMcpHttpConfig {
    fn clone(&self) -> Self {
        Python::try_attach(|py| Self {
            inner: self.inner.clone(),
            sandbox_policy: self
                .sandbox_policy
                .as_ref()
                .map(|policy| policy.clone_ref(py)),
        })
        .unwrap_or_else(|| Self {
            inner: self.inner.clone(),
            sandbox_policy: None,
        })
    }
}

impl From<McpHttpConfig> for PyMcpHttpConfig {
    fn from(inner: McpHttpConfig) -> Self {
        Self {
            inner,
            sandbox_policy: None,
        }
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
        Self {
            inner: build::build_config(
                port,
                server_name,
                server_version,
                enable_cors,
                request_timeout_ms,
                backend_timeout_ms,
                enable_prometheus,
                prometheus_basic_auth,
                gateway_async_dispatch_timeout_ms,
                gateway_wait_terminal_timeout_ms,
                gateway_route_ttl_secs,
                gateway_max_routes_per_session,
                shutdown_on_drop,
            ),
            sandbox_policy: None,
        }
    }

    /// Optional sandbox policy applied to in-process skill execution (issue #1001).
    #[getter]
    fn sandbox_policy(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.sandbox_policy.as_ref().map(|p| p.clone_ref(py))
    }

    #[setter]
    fn set_sandbox_policy(&mut self, policy: Option<Py<PyAny>>) {
        self.sandbox_policy = policy;
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

    /// Bind address. Accepts any IPv4/IPv6 literal that ``std::net::IpAddr``
    /// can parse. Raises ``ValueError`` for malformed input — never panics.
    #[setter]
    fn set_host(&mut self, v: &str) -> PyResult<()> {
        self.inner.set_host(v).map(|_| ()).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("invalid host {v:?}: {e}"))
        })
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

    /// Enable the built-in ``workflows_*`` tools (issue #348).
    #[getter]
    fn enable_workflows(&self) -> bool {
        self.inner.workflow.enable_workflows
    }

    /// Enable the built-in ``workflows_*`` tools (issue #348).
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

    /// Omit unloaded-skill ``__skill__*`` stubs from ``tools/list`` (#174).
    #[getter]
    fn exclude_skill_stubs_from_tools_list(&self) -> bool {
        self.inner.features.exclude_skill_stubs_from_tools_list
    }

    /// Omit unloaded-skill ``__skill__*`` stubs from ``tools/list`` (#174).
    #[setter]
    fn set_exclude_skill_stubs_from_tools_list(&mut self, v: bool) {
        self.inner.features.exclude_skill_stubs_from_tools_list = v;
    }

    /// Omit inactive-group ``__group__*`` stubs from ``tools/list``.
    #[getter]
    fn exclude_group_stubs_from_tools_list(&self) -> bool {
        self.inner.features.exclude_group_stubs_from_tools_list
    }

    /// Omit inactive-group ``__group__*`` stubs from ``tools/list``.
    #[setter]
    fn set_exclude_group_stubs_from_tools_list(&mut self, v: bool) {
        self.inner.features.exclude_group_stubs_from_tools_list = v;
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

    /// Treat this standalone interpreter as the safe main-thread execution lane.
    #[getter]
    fn standalone_main_thread_execution(&self) -> bool {
        self.inner.features.standalone_main_thread_execution
    }

    /// Treat this standalone interpreter as the safe main-thread execution lane.
    #[setter]
    fn set_standalone_main_thread_execution(&mut self, v: bool) {
        self.inner.features.standalone_main_thread_execution = v;
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

    /// Optional remote/LAN gateway bind host.
    #[getter]
    fn gateway_remote_host(&self) -> Option<String> {
        self.inner.gateway.remote_host.clone()
    }

    /// Optional remote/LAN gateway bind host.
    #[setter]
    fn set_gateway_remote_host(&mut self, v: Option<String>) {
        self.inner.gateway.remote_host = v;
    }

    /// Optional remote/LAN gateway port. ``0`` disables the remote listener.
    #[getter]
    fn gateway_remote_port(&self) -> u16 {
        self.inner.gateway.remote_gateway_port
    }

    /// Optional remote/LAN gateway port. ``0`` disables the remote listener.
    #[setter]
    fn set_gateway_remote_port(&mut self, v: u16) {
        self.inner.gateway.remote_gateway_port = v;
    }

    /// Whether the elected gateway serves the local Admin UI.
    #[getter]
    fn admin_enabled(&self) -> bool {
        self.inner.gateway.admin_enabled
    }

    /// Whether the elected gateway serves the local Admin UI.
    #[setter]
    fn set_admin_enabled(&mut self, v: bool) {
        self.inner.gateway.admin_enabled = v;
    }

    /// URL prefix for the Admin UI (default ``/admin``).
    #[getter]
    fn admin_path(&self) -> String {
        self.inner.gateway.admin_path.clone()
    }

    /// URL prefix for the Admin UI (default ``/admin``).
    #[setter]
    fn set_admin_path(&mut self, v: String) {
        self.inner.gateway.admin_path = v;
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

    /// Human-readable gateway candidate name written to the gateway sentinel.
    #[getter]
    fn gateway_name(&self) -> Option<String> {
        self.inner.gateway.gateway_name.clone()
    }

    /// Human-readable gateway candidate name written to the gateway sentinel.
    #[setter]
    fn set_gateway_name(&mut self, v: Option<String>) {
        self.inner.gateway.gateway_name = v;
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

    /// Enable gateway read-only policy mode. Blocks load_skill and non-read-only calls.
    #[getter]
    fn gateway_read_only(&self) -> bool {
        self.inner.gateway.policy.read_only
    }

    /// Enable gateway read-only policy mode. Blocks load_skill and non-read-only calls.
    #[setter]
    fn set_gateway_read_only(&mut self, v: bool) {
        self.inner.gateway.policy.read_only = v;
    }

    /// Allowed gateway DCC types. Empty means unrestricted.
    #[getter]
    fn allowed_dcc_types(&self) -> Vec<String> {
        self.inner.gateway.policy.allowed_dcc_types.clone()
    }

    /// Allowed gateway DCC types. Empty means unrestricted.
    #[setter]
    fn set_allowed_dcc_types(&mut self, values: Vec<String>) {
        self.inner.gateway.policy.allowed_dcc_types = values;
    }

    /// Exact allowed gateway skill names. Empty with no families means unrestricted.
    #[getter]
    fn allowed_skill_names(&self) -> Vec<String> {
        self.inner.gateway.policy.allowed_skill_names.clone()
    }

    /// Exact allowed gateway skill names. Empty with no families means unrestricted.
    #[setter]
    fn set_allowed_skill_names(&mut self, values: Vec<String>) {
        self.inner.gateway.policy.allowed_skill_names = values;
    }

    /// Allowed gateway skill-family prefixes. Empty with no names means unrestricted.
    #[getter]
    fn allowed_skill_families(&self) -> Vec<String> {
        self.inner.gateway.policy.allowed_skill_families.clone()
    }

    /// Allowed gateway skill-family prefixes. Empty with no names means unrestricted.
    #[setter]
    fn set_allowed_skill_families(&mut self, values: Vec<String>) {
        self.inner.gateway.policy.allowed_skill_families = values;
    }

    /// Exact allowed canonical gateway tool slugs. Empty with no prefixes means unrestricted.
    #[getter]
    fn allowed_tool_slugs(&self) -> Vec<String> {
        self.inner.gateway.policy.allowed_tool_slugs.clone()
    }

    /// Exact allowed canonical gateway tool slugs. Empty with no prefixes means unrestricted.
    #[setter]
    fn set_allowed_tool_slugs(&mut self, values: Vec<String>) {
        self.inner.gateway.policy.allowed_tool_slugs = values;
    }

    /// Allowed canonical gateway tool slug prefixes.
    #[getter]
    fn allowed_tool_slug_prefixes(&self) -> Vec<String> {
        self.inner.gateway.policy.allowed_tool_slug_prefixes.clone()
    }

    /// Allowed canonical gateway tool slug prefixes.
    #[setter]
    fn set_allowed_tool_slug_prefixes(&mut self, values: Vec<String>) {
        self.inner.gateway.policy.allowed_tool_slug_prefixes = values;
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

    /// Enable the built-in ``workflows_*`` tools (issue #348).
    #[getter]
    fn enable_scheduler(&self) -> bool {
        self.inner.workflow.enable_scheduler
    }

    /// Enable the built-in ``workflows_*`` tools (issue #348).
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
        self.inner.job.job_recovery = dcc_mcp_http_types::config::JobRecoveryPolicy::parse(policy)
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

#[cfg(test)]
mod tests;
