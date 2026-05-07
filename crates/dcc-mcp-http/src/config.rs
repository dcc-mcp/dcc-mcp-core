//! Server configuration.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;

/// How the server and gateway HTTP listeners are driven.
///
/// Fixes **issue #303** — under PyO3-embedded interpreters (Maya on Windows),
/// `tokio::spawn` onto a multi-threaded runtime that no longer has an active
/// driver can cause background accept loops (specifically the gateway
/// listener) to be starved of scheduling time. The per-instance listener
/// survives because its accept loop is "warmed up" during the initial
/// `block_on`, but the gateway listener — spawned via an extra `tokio::spawn`
/// + `tokio::join!` layer — never gets its turn.
///
/// `ServerSpawnMode::Dedicated` avoids the failure mode entirely by running
/// each HTTP listener on its own OS thread that owns a `current_thread`
/// Tokio runtime. That thread is scheduled by the OS, not by a shared
/// worker pool, and cannot be starved by a hanging block_on elsewhere.
///
/// | Mode | When to use | Behaviour |
/// |------|-------------|-----------|
/// | `Ambient`   | Standalone binary (`dcc-mcp-server`, library tests) | Spawns `axum::serve` onto the caller's Tokio runtime via `tokio::spawn`. |
/// | `Dedicated` | Python bindings (`PyMcpHttpServer`) / embedded DCC hosts | Each listener gets its own OS thread + `current_thread` runtime. Immune to PyO3 worker starvation. |
///
/// Defaults: `Ambient`. The Python bindings override this to `Dedicated`
/// automatically when constructing `McpHttpServer` via `PyMcpHttpServer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerSpawnMode {
    /// Spawn listeners as background tasks on the caller's Tokio runtime.
    /// Correct for `#[tokio::main]` binaries that keep a thread in the
    /// runtime for the process lifetime.
    #[default]
    Ambient,

    /// Spawn each listener on a dedicated OS thread with its own
    /// `current_thread` runtime. Correct for PyO3-embedded interpreters
    /// where the parent runtime's worker pool cannot be relied upon after
    /// `block_on` returns.
    Dedicated,
}

/// What [`McpHttpServer::start`](crate::McpHttpServer::start) does with rows
/// that the previous process left in `Pending` / `Running` after a crash or
/// restart (issue #567).
///
/// | Variant | Behaviour |
/// |---------|-----------|
/// | [`JobRecoveryPolicy::Drop`]    | Each in-flight row is rewritten to [`JobStatus::Interrupted`](crate::job::JobStatus::Interrupted) with `error = "server restart"`. Clients re-subscribing after reconnect see one clean terminal transition. **This is today's behaviour and the default.** |
/// | [`JobRecoveryPolicy::Requeue`] | Reserved for a future release that persists the original tool arguments alongside the `jobs` row. Until that lands the variant is **accepted but treated as `Drop`** — the server logs a `WARN` at startup so operators know the requested policy is not yet active, but startup itself never fails. The accepted-but-degraded contract gives DCC adapters (`dcc-mcp-maya`, `dcc-mcp-houdini`) a stable knob to plumb through today without forcing a config-shape break when the real implementation lands. |
///
/// String form (used by the Python binding and the upcoming `--job-recovery`
/// CLI flag): `"drop"` / `"requeue"`. Defaults to `Drop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JobRecoveryPolicy {
    /// Rewrite every `Pending` / `Running` row to `Interrupted` on startup.
    /// Always safe; never re-runs a partially-applied tool.
    #[default]
    Drop,
    /// Reserved policy: would re-submit idempotent in-flight jobs from the
    /// persisted spec. Accepted today but treated as [`Self::Drop`] with a
    /// `WARN` log at startup until tool-arg persistence lands.
    Requeue,
}

impl JobRecoveryPolicy {
    /// Lower-case wire identifier used by docs, the Python binding, and the
    /// `--job-recovery` CLI flag.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Requeue => "requeue",
        }
    }

    /// Parse the wire identifier. `&str` is matched case-insensitively to
    /// be tolerant of env-var plumbing (`DCC_MCP_*_JOB_RECOVERY=Requeue`).
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "drop" => Ok(Self::Drop),
            "requeue" => Ok(Self::Requeue),
            other => Err(format!(
                "unknown job_recovery policy {other:?}; expected \"drop\" or \"requeue\""
            )),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Sub-config structs — one per orthogonal concern
// ─────────────────────────────────────────────────────────────────────────────

/// Core server identity & transport configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Port to listen on. Default: 8765.
    pub port: u16,

    /// IP address to bind. Default: 127.0.0.1 (localhost only, per MCP security spec).
    pub host: IpAddr,

    /// MCP endpoint path. Default: `/mcp`.
    pub endpoint_path: String,

    /// Server name reported in MCP `initialize` response.
    pub server_name: String,

    /// Server version reported in MCP `initialize` response.
    pub server_version: String,

    /// Maximum concurrent SSE sessions. Default: 100.
    pub max_sessions: usize,

    /// Request timeout in milliseconds. Default: 30_000.
    pub request_timeout_ms: u64,

    /// Whether to enable CORS for browser-based MCP clients. Default: false.
    pub enable_cors: bool,

    /// How listener tasks (per-instance MCP endpoint and the optional
    /// gateway) are driven. See [`ServerSpawnMode`] for the tradeoffs.
    ///
    /// Default: [`ServerSpawnMode::Ambient`]. PyO3-embedded users should
    /// set this to [`ServerSpawnMode::Dedicated`] (the Python bindings do
    /// so automatically). Fixes issue #303.
    pub spawn_mode: ServerSpawnMode,

    /// Maximum time to wait when self-probing a freshly bound listener to
    /// confirm it is actually accepting connections before reporting
    /// success. Applied per attempt; up to 5 attempts are made. Set to 0
    /// to disable self-probing (not recommended). Default: 200.
    pub self_probe_timeout_ms: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 8765,
            host: IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            endpoint_path: "/mcp".to_string(),
            server_name: "dcc-mcp".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            max_sessions: 100,
            request_timeout_ms: 30_000,
            enable_cors: false,
            spawn_mode: ServerSpawnMode::Ambient,
            self_probe_timeout_ms: 200,
        }
    }
}

// ── Pass-through getters/setters so PyWrapper macro-generated ──
//  `self.inner.port()` etc. call these methods and compile.
impl McpHttpConfig {
    pub fn port(&self) -> u16 {
        self.server.port
    }
    pub fn set_port(&mut self, v: u16) {
        self.server.port = v;
    }
    pub fn host(&self) -> String {
        self.server.host.to_string()
    }
    pub fn set_host(&mut self, v: &str) -> Result<&mut Self, std::net::AddrParseError> {
        self.server.host = v.parse()?;
        Ok(self)
    }
    pub fn endpoint_path(&self) -> String {
        self.server.endpoint_path.clone()
    }
    pub fn set_endpoint_path(&mut self, v: String) {
        self.server.endpoint_path = v;
    }
    pub fn server_name(&self) -> String {
        self.server.server_name.clone()
    }
    pub fn set_server_name(&mut self, v: String) {
        self.server.server_name = v;
    }
    pub fn server_version(&self) -> String {
        self.server.server_version.clone()
    }
    pub fn set_server_version(&mut self, v: String) {
        self.server.server_version = v;
    }
    pub fn max_sessions(&self) -> usize {
        self.server.max_sessions
    }
    pub fn set_max_sessions(&mut self, v: usize) {
        self.server.max_sessions = v;
    }
    pub fn request_timeout_ms(&self) -> u64 {
        self.server.request_timeout_ms
    }
    pub fn set_request_timeout_ms(&mut self, v: u64) {
        self.server.request_timeout_ms = v;
    }
    pub fn enable_cors(&self) -> bool {
        self.server.enable_cors
    }
    pub fn set_enable_cors(&mut self, v: bool) {
        self.server.enable_cors = v;
    }
    pub fn self_probe_timeout_ms(&self) -> u64 {
        self.server.self_probe_timeout_ms
    }
    pub fn set_self_probe_timeout_ms(&mut self, v: u64) {
        self.server.self_probe_timeout_ms = v;
    }
    pub fn spawn_mode(&self) -> ServerSpawnMode {
        self.server.spawn_mode
    }
    pub fn set_spawn_mode(&mut self, v: ServerSpawnMode) {
        self.server.spawn_mode = v;
    }
    pub fn session_ttl_secs(&self) -> u64 {
        self.session.session_ttl_secs
    }
    pub fn set_session_ttl_secs(&mut self, v: u64) {
        self.session.session_ttl_secs = v;
    }
    pub fn enable_tool_cache(&self) -> bool {
        self.session.enable_tool_cache
    }
    pub fn set_enable_tool_cache(&mut self, v: bool) {
        self.session.enable_tool_cache = v;
    }
    pub fn gateway_port(&self) -> u16 {
        self.gateway.gateway_port
    }
    pub fn set_gateway_port(&mut self, v: u16) {
        self.gateway.gateway_port = v;
    }
    pub fn admin_enabled(&self) -> bool {
        self.gateway.admin_enabled
    }
    pub fn set_admin_enabled(&mut self, v: bool) {
        self.gateway.admin_enabled = v;
    }
    pub fn admin_path(&self) -> String {
        self.gateway.admin_path.clone()
    }
    pub fn set_admin_path(&mut self, v: String) {
        self.gateway.admin_path = v;
    }
    pub fn stale_timeout_secs(&self) -> u64 {
        self.gateway.stale_timeout_secs
    }
    pub fn set_stale_timeout_secs(&mut self, v: u64) {
        self.gateway.stale_timeout_secs = v;
    }
    pub fn heartbeat_secs(&self) -> u64 {
        self.gateway.heartbeat_secs
    }
    pub fn set_heartbeat_secs(&mut self, v: u64) {
        self.gateway.heartbeat_secs = v;
    }
    pub fn backend_timeout_ms(&self) -> u64 {
        self.gateway.backend_timeout_ms
    }
    pub fn set_backend_timeout_ms(&mut self, v: u64) {
        self.gateway.backend_timeout_ms = v;
    }
    pub fn gateway_async_dispatch_timeout_ms(&self) -> u64 {
        self.gateway.gateway_async_dispatch_timeout_ms
    }
    pub fn set_gateway_async_dispatch_timeout_ms(&mut self, v: u64) {
        self.gateway.gateway_async_dispatch_timeout_ms = v;
    }
    pub fn gateway_wait_terminal_timeout_ms(&self) -> u64 {
        self.gateway.gateway_wait_terminal_timeout_ms
    }
    pub fn set_gateway_wait_terminal_timeout_ms(&mut self, v: u64) {
        self.gateway.gateway_wait_terminal_timeout_ms = v;
    }
    pub fn gateway_route_ttl_secs(&self) -> u64 {
        self.gateway.gateway_route_ttl_secs
    }
    pub fn set_gateway_route_ttl_secs(&mut self, v: u64) {
        self.gateway.gateway_route_ttl_secs = v;
    }
    pub fn gateway_max_routes_per_session(&self) -> u64 {
        self.gateway.gateway_max_routes_per_session
    }
    pub fn set_gateway_max_routes_per_session(&mut self, v: u64) {
        self.gateway.gateway_max_routes_per_session = v;
    }
    pub fn gateway_cursor_safe_tool_names(&self) -> bool {
        self.gateway.gateway_cursor_safe_tool_names
    }
    pub fn set_gateway_cursor_safe_tool_names(&mut self, v: bool) {
        self.gateway.gateway_cursor_safe_tool_names = v;
    }
    pub fn adapter_version(&self) -> Option<String> {
        self.gateway.adapter_version.clone()
    }
    pub fn set_adapter_version(&mut self, v: Option<String>) {
        self.gateway.adapter_version = v;
    }
    pub fn adapter_dcc(&self) -> Option<String> {
        self.gateway.adapter_dcc.clone()
    }
    pub fn set_adapter_dcc(&mut self, v: Option<String>) {
        self.gateway.adapter_dcc = v;
    }
    pub fn allow_unknown_tools(&self) -> bool {
        self.gateway.allow_unknown_tools
    }
    pub fn set_allow_unknown_tools(&mut self, v: bool) {
        self.gateway.allow_unknown_tools = v;
    }
    pub fn deferred_queue_depth(&self) -> usize {
        self.queue.deferred_queue_depth
    }
    pub fn set_deferred_queue_depth(&mut self, v: usize) {
        self.queue.deferred_queue_depth = v;
    }
    pub fn bridge_queue_depth(&self) -> usize {
        self.queue.bridge_queue_depth
    }
    pub fn set_bridge_queue_depth(&mut self, v: usize) {
        self.queue.bridge_queue_depth = v;
    }
    pub fn host_queue_depth(&self) -> usize {
        self.queue.host_queue_depth
    }
    pub fn set_host_queue_depth(&mut self, v: usize) {
        self.queue.host_queue_depth = v;
    }
    pub fn queue_send_timeout_ms(&self) -> u64 {
        self.queue.queue_send_timeout_ms
    }
    pub fn set_queue_send_timeout_ms(&mut self, v: u64) {
        self.queue.queue_send_timeout_ms = v;
    }
    pub fn enable_prometheus(&self) -> bool {
        self.telemetry.enable_prometheus
    }
    pub fn set_enable_prometheus(&mut self, v: bool) {
        self.telemetry.enable_prometheus = v;
    }
    pub fn prometheus_basic_auth(&self) -> Option<(String, String)> {
        self.telemetry.prometheus_basic_auth.clone()
    }
    pub fn set_prometheus_basic_auth(&mut self, v: Option<(String, String)>) {
        self.telemetry.prometheus_basic_auth = v;
    }
    pub fn lazy_actions(&self) -> bool {
        self.features.lazy_actions
    }
    pub fn set_lazy_actions(&mut self, v: bool) {
        self.features.lazy_actions = v;
    }
    pub fn enable_workflows(&self) -> bool {
        self.workflow.enable_workflows
    }
    pub fn set_enable_workflows(&mut self, v: bool) {
        self.workflow.enable_workflows = v;
    }
    pub fn shutdown_on_drop(&self) -> bool {
        self.features.shutdown_on_drop
    }
    pub fn set_shutdown_on_drop(&mut self, v: bool) {
        self.features.shutdown_on_drop = v;
    }
    pub fn enable_job_notifications(&self) -> bool {
        self.features.enable_job_notifications
    }
    pub fn set_enable_job_notifications(&mut self, v: bool) {
        self.features.enable_job_notifications = v;
    }
    pub fn enable_resources(&self) -> bool {
        self.features.enable_resources
    }
    pub fn set_enable_resources(&mut self, v: bool) {
        self.features.enable_resources = v;
    }
    pub fn enable_prompts(&self) -> bool {
        self.features.enable_prompts
    }
    pub fn set_enable_prompts(&mut self, v: bool) {
        self.features.enable_prompts = v;
    }
    pub fn enable_artefact_resources(&self) -> bool {
        self.features.enable_artefact_resources
    }
    pub fn set_enable_artefact_resources(&mut self, v: bool) {
        self.features.enable_artefact_resources = v;
    }
    pub fn dcc_type(&self) -> Option<String> {
        self.instance.dcc_type.clone()
    }
    pub fn set_dcc_type(&mut self, v: Option<String>) {
        self.instance.dcc_type = v;
    }
    pub fn dcc_version(&self) -> Option<String> {
        self.instance.dcc_version.clone()
    }
    pub fn set_dcc_version(&mut self, v: Option<String>) {
        self.instance.dcc_version = v;
    }
    pub fn scene(&self) -> Option<String> {
        self.instance.scene.clone()
    }
    pub fn set_scene(&mut self, v: Option<String>) {
        self.instance.scene = v;
    }
    pub fn instance_metadata(&self) -> HashMap<String, String> {
        self.instance.instance_metadata.clone()
    }
    pub fn set_instance_metadata(&mut self, v: HashMap<String, String>) {
        self.instance.instance_metadata = v;
    }
    pub fn declared_capabilities(&self) -> Vec<String> {
        self.instance.declared_capabilities.clone()
    }
    pub fn set_declared_capabilities(&mut self, v: Vec<String>) {
        self.instance.declared_capabilities = v;
    }
    pub fn job_storage_path(&self) -> Option<PathBuf> {
        self.job.job_storage_path.clone()
    }
    pub fn set_job_storage_path(&mut self, v: Option<PathBuf>) {
        self.job.job_storage_path = v;
    }
    pub fn job_recovery(&self) -> JobRecoveryPolicy {
        self.job.job_recovery
    }
    pub fn set_job_recovery(&mut self, v: JobRecoveryPolicy) {
        self.job.job_recovery = v;
    }
    pub fn enable_scheduler(&self) -> bool {
        self.workflow.enable_scheduler
    }
    pub fn set_enable_scheduler(&mut self, v: bool) {
        self.workflow.enable_scheduler = v;
    }
    pub fn schedules_dir(&self) -> Option<PathBuf> {
        self.workflow.schedules_dir.clone()
    }
    pub fn set_schedules_dir(&mut self, v: Option<PathBuf>) {
        self.workflow.schedules_dir = v;
    }
}

/// DCC instance registration metadata.
#[derive(Debug, Clone, Default)]
pub struct InstanceConfig {
    /// DCC application type (e.g. `"maya"`, `"blender"`). Reported in the
    /// shared `FileRegistry` so the gateway can route by DCC type.
    pub dcc_type: Option<String>,

    /// DCC application version (e.g. `"2025.1"`).
    pub dcc_version: Option<String>,

    /// Currently open scene/file. Improves routing accuracy.
    pub scene: Option<String>,

    /// Arbitrary instance metadata recorded in FileRegistry.
    ///
    /// Rez/package launchers use this for context-bundle fields such as
    /// `context_bundle`, `production_domain`, `context_kind`, `project`,
    /// `task`, `toolset_profile`, and `package_provenance`.
    pub instance_metadata: HashMap<String, String>,

    /// Capabilities declared by the DCC adapter hosting this server (issue #354).
    ///
    /// Each tool may list [`required_capabilities`] in its sibling
    /// `tools.yaml`; on `tools/call` the server intersects the tool's
    /// requirements against this declared set. Missing capabilities
    /// surface as a `-32001 capability_missing` MCP error. Tools with
    /// unmet capabilities still appear in `tools/list` but carry
    /// `_meta.dcc.missing_capabilities = [...]` so clients can filter.
    ///
    /// The list is freeform — conventionally lowercase dotted identifiers
    /// like `"usd"`, `"scene.mutate"`, `"filesystem.read"`. Adapters hard-code
    /// it at construction time; there is no runtime introspection of the DCC.
    ///
    /// Default: empty (no capabilities declared — any tool with declared
    /// requirements will report them as missing).
    pub declared_capabilities: Vec<String>,
}

/// Session lifecycle & tool-cache configuration.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Idle session TTL in seconds. Sessions that have not received any
    /// request within this window are automatically evicted by a background
    /// task started in [`McpHttpServer::start`]. Default: 3600 (1 hour).
    /// Set to 0 to disable automatic eviction.
    pub session_ttl_secs: u64,

    /// Enable connection-scoped tool-list caching (issue #438).
    ///
    /// When `true` (default), `tools/list` stores a per-session snapshot
    /// of the full tool list. On subsequent `tools/list` calls within the
    /// same session, if the registry generation has not changed (no skill
    /// load/unload, no group activation/deactivation), the cached
    /// snapshot is returned directly — avoiding redundant registry scans,
    /// bare-name resolution, and `McpTool` construction.
    ///
    /// The cache is automatically invalidated when:
    /// - A skill is loaded or unloaded
    /// - A tool group is activated or deactivated
    /// - The session is evicted (TTL expiry)
    /// - The client sends `tools/list` with `_meta.dcc.refresh = true`
    ///
    /// Set to `false` to disable caching (every `tools/list` call
    /// rebuilds the full list from scratch).
    pub enable_tool_cache: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            session_ttl_secs: 3_600,
            enable_tool_cache: true,
        }
    }
}

/// Gateway election, routing, and discovery configuration.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Gateway port to compete for. First process to bind wins the gateway
    /// and starts serving `/instances`, `/mcp`, `/mcp/{id}`, `/mcp/dcc/{type}`.
    /// `0` disables the gateway entirely. Default: 0 (disabled).
    pub gateway_port: u16,

    /// Shared `FileRegistry` directory. `None` uses a system temp dir.
    pub registry_dir: Option<PathBuf>,

    /// Seconds without a heartbeat before an instance is considered stale.
    /// Default: 30.
    pub stale_timeout_secs: u64,

    /// Heartbeat interval in seconds. `0` disables the heartbeat task.
    /// Default: 5.
    pub heartbeat_secs: u64,

    /// Per-backend request timeout (milliseconds) used by the gateway when
    /// fanning out `tools/list` / `tools/call` to live DCC instances.
    ///
    /// Default: `120_000` (120 seconds / 2 minutes). DCC scene operations
    /// (mesh import, simulation bake, render, complex keyframe setup) regularly
    /// take tens of seconds. The previous default of 10 s caused the gateway to
    /// cancel legitimate tool calls while the backend was still working, logging
    /// "tool call cancelled cooperatively" on the DCC side at exactly 10 s.
    ///
    /// For truly long-running operations (renders, heavy simulations) prefer
    /// async dispatch (`_meta.dcc.async = true`) which returns a `job_id`
    /// immediately and lets the client poll via `jobs.get_status`.
    ///
    /// Only the gateway fan-out uses this value — per-instance servers
    /// bound to a DCC execute inline and are governed by
    /// [`Self::request_timeout_ms`] instead. Fixes issue #314.
    pub backend_timeout_ms: u64,

    /// Per-backend request timeout (milliseconds) applied by the gateway
    /// when the client has opted into **async dispatch** (issue #321).
    pub gateway_async_dispatch_timeout_ms: u64,

    /// Gateway timeout (milliseconds) for the opt-in wait-for-terminal
    /// response-stitching mode (issue #321).
    pub gateway_wait_terminal_timeout_ms: u64,

    /// TTL (seconds) for the gateway's per-job routing cache (issue #322).
    pub gateway_route_ttl_secs: u64,

    /// Per-session ceiling on concurrent live routes in the gateway
    /// routing cache (issue #322). `0` disables the cap.
    pub gateway_max_routes_per_session: u64,

    /// Emit Cursor-safe gateway prompt names (`i_<id8>__<escaped>`)
    /// instead of the SEP-986 dotted form (`<id8>.<name>`).
    ///
    /// Default: `true`. The gateway no longer fans out backend tools
    /// into `tools/list` — its MCP surface is converged to discovery +
    /// dispatch primitives — but it still fans out `prompts/list` so
    /// clients can address prompts across multiple DCCs. Cursor and
    /// several other MCP clients only accept names matching
    /// `^[A-Za-z0-9_]+$`, so the cursor-safe form stays the default.
    /// Setting this to `false` emits the SEP-986 dotted form for
    /// diagnostic parity with a single-instance server that publishes
    /// dotted names directly.
    pub gateway_cursor_safe_tool_names: bool,

    /// Adapter package version (e.g. `dcc_mcp_maya = "0.3.0"`) recorded
    /// on the `__gateway__` sentinel and used as the second tier of the
    /// version-aware gateway election (issue maya#137).
    pub adapter_version: Option<String>,

    /// DCC type the adapter is bound to (e.g. `"maya"`). Drives the
    /// third-tier "real DCC over generic standalone" tiebreaker in
    /// gateway election (issue maya#137).
    pub adapter_dcc: Option<String>,

    /// Allow instances with `dcc_type == "unknown"` to expose their tools
    /// via the gateway (issue #555).
    ///
    /// Default: `false`. When `false`, the gateway's `tools/list` and
    /// `connect_to_dcc` ignore any instance whose `dcc_type` is
    /// `"unknown"` (case-insensitive). Set to `true` only for development
    /// or when intentionally running a standalone server without a real DCC.
    pub allow_unknown_tools: bool,

    /// Enable the read-only gateway admin dashboard.
    ///
    /// Default: `true`. Only the elected gateway process mounts this path,
    /// so a multi-instance process group still exposes a single admin UI.
    pub admin_enabled: bool,

    /// URL prefix for the admin dashboard. Default: `"/admin"`.
    pub admin_path: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            gateway_port: 0,
            registry_dir: None,
            stale_timeout_secs: 30,
            heartbeat_secs: 5,
            backend_timeout_ms: 120_000,
            gateway_async_dispatch_timeout_ms: 60_000,
            gateway_wait_terminal_timeout_ms: 600_000,
            gateway_route_ttl_secs: 60 * 60 * 24,
            gateway_max_routes_per_session: 1_000,
            gateway_cursor_safe_tool_names: true,
            adapter_version: None,
            adapter_dcc: None,
            allow_unknown_tools: false,
            admin_enabled: true,
            admin_path: "/admin".to_string(),
        }
    }
}

/// Queue depth & backpressure configuration (issue #715).
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Capacity of the HTTP → `DccExecutor` mpsc channel (issue #715).
    ///
    /// Controls how many outstanding `tools/call` submissions may queue
    /// up before the backpressure path kicks in. When the channel is
    /// full, the HTTP worker blocks for up to [`Self::queue_send_timeout_ms`]
    /// waiting for the DCC main thread to drain; if the drain does not
    /// happen in time, the call returns a structured
    /// [`crate::error::HttpError::QueueOverloaded`].
    ///
    /// Default: `16`. Override via
    /// `--queue-deferred-cap=<N>` / `MCP_QUEUE_DEFERRED_CAP`.
    pub deferred_queue_depth: usize,

    /// Capacity of the `DeferredExecutor` → `host_bridge` mpsc channel
    /// (issue #715).
    ///
    /// Previously hard-coded to `16`. Exposed as a config knob so
    /// operators can tune the bridge depth without patching the crate.
    /// Set to `0` to fall back to [`crate::host_bridge::DEFAULT_BRIDGE_QUEUE_DEPTH`]
    /// — this keeps misconfigured env-vars (`MCP_QUEUE_BRIDGE_CAP=0`)
    /// from silently disabling backpressure.
    ///
    /// Default: `16`. Override via
    /// `--queue-bridge-cap=<N>` / `MCP_QUEUE_BRIDGE_CAP`.
    pub bridge_queue_depth: usize,

    /// Capacity of the host-side `QueueDispatcher` (issue #715).
    ///
    /// When non-zero, applies to
    /// [`dcc_mcp_host::QueueDispatcher::with_capacity`] so posts that
    /// pile up past `N` surface
    /// [`dcc_mcp_host::DispatchError::QueueOverloaded`] instead of
    /// growing an unbounded queue. When zero (default), the dispatcher
    /// stays unbounded — matches today's behaviour.
    ///
    /// Default: `0` (unbounded). Override via
    /// `--queue-dispatcher-cap=<N>` / `MCP_QUEUE_DISPATCHER_CAP`.
    pub host_queue_depth: usize,

    /// How long an HTTP worker will block on a full executor channel
    /// before returning [`crate::error::HttpError::QueueOverloaded`]
    /// (issue #715).
    ///
    /// Chose "block with timeout" over "immediate error" so healthy
    /// bursty workloads still make progress when the main thread
    /// yields momentarily — the typed `QueueOverloaded` only fires on
    /// sustained saturation. Same strategy across all three layers.
    ///
    /// Default: `2_000` ms. Override via
    /// `--queue-send-timeout-ms=<N>` / `MCP_QUEUE_SEND_TIMEOUT_MS`.
    pub queue_send_timeout_ms: u64,

    // ── Issue #771: framework-enforced payload size limits ────────────────
    /// Maximum allowed size (bytes) for an incoming request body (issue #771).
    ///
    /// Enforced via `tower_http::limit::RequestBodyLimitLayer` at the axum
    /// router level. Requests exceeding this size are rejected with
    /// `413 Payload Too Large` before the JSON-RPC layer even sees the body.
    ///
    /// Default: `4_194_304` (4 MiB). Override via
    /// `MCP_MAX_REQUEST_BODY_BYTES`.
    pub max_request_body_bytes: usize,

    /// Maximum content size (bytes) for a single resource, prompt, or tool
    /// call result before the response is truncated (issue #771).
    ///
    /// When a response payload exceeds this limit the server wraps it in a
    /// `TruncationEnvelope`:
    /// ```json
    /// {"content": "...", "truncated": true, "original_size": N, "truncated_size": M}
    /// ```
    ///
    /// Default: `1_048_576` (1 MiB). Override via
    /// `MCP_MAX_RESPONSE_CONTENT_BYTES`.
    pub max_response_content_bytes: usize,

    /// Target chunk size (bytes) for SSE event payloads (issue #771).
    ///
    /// Large SSE events are automatically split into sequential `chunk` /
    /// `chunk_end` events so that a single oversized event cannot stall the
    /// connection's read loop on the client side.
    ///
    /// Set to `0` to disable SSE chunking.
    ///
    /// Default: `65_536` (64 KiB). Override via
    /// `MCP_SSE_CHUNK_SIZE_BYTES`.
    pub sse_chunk_size_bytes: usize,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            deferred_queue_depth: 16,
            bridge_queue_depth: 16,
            host_queue_depth: 0,
            queue_send_timeout_ms: 2_000,
            // #771: payload size limits and SSE chunking
            max_request_body_bytes: 4 * 1024 * 1024, // 4 MiB
            max_response_content_bytes: 1024 * 1024, // 1 MiB
            sse_chunk_size_bytes: 64 * 1024,         // 64 KiB
        }
    }
}

impl QueueConfig {
    /// Load the queue-stack knobs from the environment (issue #715).
    ///
    /// Honours four optional env-vars:
    /// - `MCP_QUEUE_DEFERRED_CAP`
    /// - `MCP_QUEUE_BRIDGE_CAP`
    /// - `MCP_QUEUE_DISPATCHER_CAP`
    /// - `MCP_QUEUE_SEND_TIMEOUT_MS`
    ///
    /// Unset or unparsable values leave the existing field untouched
    /// so a typo never silently flips the cap.
    pub fn apply_env_overrides(self) -> Self {
        fn load_usize(key: &str) -> Option<usize> {
            std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
        }
        fn load_u64(key: &str) -> Option<u64> {
            std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
        }
        let mut s = self;
        if let Some(v) = load_usize("MCP_QUEUE_DEFERRED_CAP") {
            s.deferred_queue_depth = v.max(1);
        }
        if let Some(v) = load_usize("MCP_QUEUE_BRIDGE_CAP") {
            s.bridge_queue_depth = v;
        }
        if let Some(v) = load_usize("MCP_QUEUE_DISPATCHER_CAP") {
            s.host_queue_depth = v;
        }
        if let Some(v) = load_u64("MCP_QUEUE_SEND_TIMEOUT_MS") {
            s.queue_send_timeout_ms = v;
        }
        s
    }

    /// Builder: set the maximum request body size (issue #771).
    ///
    /// Requests larger than `bytes` are rejected with 413 at the axum router
    /// level via `tower_http::limit::RequestBodyLimitLayer`.
    pub fn with_max_request_body_bytes(mut self, bytes: usize) -> Self {
        self.max_request_body_bytes = bytes;
        self
    }

    /// Builder: set the maximum response content size before truncation
    /// (issue #771).
    ///
    /// Resource, prompt and tool-call responses larger than `bytes` are
    /// wrapped in a [`TruncationEnvelope`](crate::payload::TruncationEnvelope).
    pub fn with_max_response_content_bytes(mut self, bytes: usize) -> Self {
        self.max_response_content_bytes = bytes;
        self
    }

    /// Builder: set the SSE chunking threshold (issue #771).
    ///
    /// SSE events larger than `bytes` are split into sequential
    /// `chunk` / `chunk_end` frames. Set to `0` to disable chunking.
    pub fn with_sse_chunk_size_bytes(mut self, bytes: usize) -> Self {
        self.sse_chunk_size_bytes = bytes;
        self
    }
}

/// Prometheus metrics configuration.
#[derive(Debug, Clone, Default)]
pub struct TelemetryConfig {
    /// Enable the Prometheus `/metrics` endpoint (issue #331).
    ///
    /// Requires the `prometheus` Cargo feature on both `dcc-mcp-http`
    /// and `dcc-mcp-telemetry`. When `true`, [`McpHttpServer::start`]
    /// mounts a `GET /metrics` route on the same Axum router that
    /// serves `/mcp`; the body is a standard Prometheus text-exposition
    /// payload (`text/plain; version=0.0.4`).
    ///
    /// Defaults to `false`: the endpoint is opt-in, and when the
    /// feature is compiled out this flag has no effect.
    pub enable_prometheus: bool,

    /// Optional HTTP Basic auth guard for `/metrics` (issue #331).
    ///
    /// When `Some((user, pass))`, scrapers must present a matching
    /// `Authorization: Basic ...` header or the endpoint responds with
    /// `401 Unauthorized`. When `None` (default), the endpoint is
    /// unauthenticated — acceptable for localhost-only development but
    /// strongly discouraged in production.
    pub prometheus_basic_auth: Option<(String, String)>,
}

/// Opt-in capability switches.
#[derive(Debug, Clone)]
pub struct FeatureFlags {
    /// Enable the opt-in lazy-actions meta-tools: ``list_actions``,
    /// ``describe_action`` and ``call_action``.
    ///
    /// When `true`, `tools/list` additionally surfaces these three meta-tools
    /// so agents with tight context budgets can drive an arbitrarily large
    /// action catalog through a single page of 3 stubs instead of paging
    /// through every loaded skill's tools. Default: `false`.
    pub lazy_actions: bool,

    /// Publish skill-scoped tools under their **bare action name** when no
    /// collision exists on this instance (#307).
    ///
    /// When `true` (default), `tools/list` emits `execute_python` rather than
    /// `maya-scripting.execute_python` whenever the bare name is unique
    /// within the instance's loaded skills. Collisions fall back to the
    /// full `<skill>.<action>` form, and `tools/call` accepts both shapes
    /// for one release cycle.
    pub bare_tool_names: bool,

    /// Advertise the MCP Resources primitive (issue #350).
    pub enable_resources: bool,

    /// Advertise the MCP Prompts primitive (issues #351, #355).
    pub enable_prompts: bool,

    /// Expose `artefact://` resources (issue #349).
    pub enable_artefact_resources: bool,

    /// Emit the `notifications/$/dcc.jobUpdated` and
    /// `notifications/$/dcc.workflowUpdated` SSE channels (issue #326).
    pub enable_job_notifications: bool,

    /// Best-effort safety net for Python callers that drop a
    /// `McpServerHandle` without calling `shutdown()`.
    pub shutdown_on_drop: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            lazy_actions: false,
            bare_tool_names: true,
            enable_resources: true,
            enable_prompts: true,
            enable_artefact_resources: false,
            enable_job_notifications: true,
            shutdown_on_drop: false,
        }
    }
}

/// Workflow & scheduler configuration.
#[derive(Debug, Clone, Default)]
pub struct WorkflowConfig {
    /// Enable the built-in `workflows.*` tools (issue #348).
    pub enable_workflows: bool,

    /// Enable the cron + webhook scheduler subsystem (issue #352).
    pub enable_scheduler: bool,

    /// Directory holding `*.schedules.yaml` files for the scheduler
    /// subsystem (issue #352).
    pub schedules_dir: Option<PathBuf>,
}

/// Job persistence & recovery configuration.
#[derive(Debug, Clone)]
pub struct JobConfig {
    /// Path to a SQLite database file for persisting tracked jobs
    /// (issue #328).
    pub job_storage_path: Option<PathBuf>,

    /// What to do with rows the previous process left in `Pending` /
    /// `Running` after a crash or restart (issue #567).
    pub job_recovery: JobRecoveryPolicy,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self {
            job_storage_path: None,
            job_recovery: JobRecoveryPolicy::Drop,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// McpHttpConfig — thin aggregate of the 9 sub-configs above.
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for [`McpHttpServer`](crate::McpHttpServer).
///
/// This is a **thin aggregate** of 9 cohesive sub-config structs,
/// each owning one orthogonal concern:
///
/// | Field | Struct | Concern |
/// |-------|--------|----------|
/// | `server` | [`ServerConfig`] | Core server identity & transport |
/// | `instance` | [`InstanceConfig`] | DCC registration metadata |
/// | `session` | [`SessionConfig`] | Session lifecycle & tool-cache |
/// | `gateway` | [`GatewayConfig`] | Gateway election, routing, discovery |
/// | `queue` | [`QueueConfig`] | Queue depth & backpressure |
/// | `telemetry` | [`TelemetryConfig`] | Prometheus metrics |
/// | `features` | [`FeatureFlags`] | Opt-in capability switches |
/// | `workflow` | [`WorkflowConfig`] | Workflow & scheduler |
/// | `job` | [`JobConfig`] | Job persistence & recovery |
///
/// Use [`McpHttpConfig::default()`] for sensible defaults, or the
/// builder-pattern methods (`.with_*()`) for customization.
///
/// `#[deprecated]` `::new(port)` is kept for one minor release to avoid
/// breaking existing callers.
#[derive(Debug, Clone, Default)]
pub struct McpHttpConfig {
    /// Core server identity & transport.
    pub server: ServerConfig,

    /// DCC instance registration metadata.
    pub instance: InstanceConfig,

    /// Session lifecycle & tool-cache.
    pub session: SessionConfig,

    /// Gateway election, routing, and discovery.
    pub gateway: GatewayConfig,

    /// Queue depth & backpressure.
    pub queue: QueueConfig,

    /// Prometheus metrics.
    pub telemetry: TelemetryConfig,

    /// Opt-in capability switches.
    pub features: FeatureFlags,

    /// Workflow & scheduler.
    pub workflow: WorkflowConfig,

    /// Job persistence & recovery.
    pub job: JobConfig,
}

impl McpHttpConfig {
    /// Create a config with the given port and sensible defaults.
    ///
    /// # Deprecated
    ///
    /// Use [`McpHttpConfig::default()`] or the builder pattern instead.
    /// This method will be removed in a future minor release.
    #[deprecated(note = "use McpHttpConfig::default() or the builder pattern")]
    pub fn new(port: u16) -> Self {
        let mut cfg = Self::default();
        cfg.server.port = port;
        cfg
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.server.port = port;
        self
    }

    /// Builder: stamp the adapter package version onto the gateway
    /// sentinel for version-aware election (issue maya#137).
    pub fn with_adapter_version(mut self, version: impl Into<String>) -> Self {
        self.gateway.adapter_version = Some(version.into());
        self
    }

    /// Builder: declare the DCC type this adapter is bound to so the
    /// gateway election can prefer real DCCs over generic standalone
    /// servers (issue maya#137).
    pub fn with_adapter_dcc(mut self, dcc: impl Into<String>) -> Self {
        self.gateway.adapter_dcc = Some(dcc.into());
        self
    }

    /// Builder: choose the gateway tool-name wire form (issue #656).
    pub fn with_gateway_cursor_safe_tool_names(mut self, enabled: bool) -> Self {
        self.gateway.gateway_cursor_safe_tool_names = enabled;
        self
    }

    /// Builder: attach context/provenance metadata to the FileRegistry row.
    pub fn with_instance_metadata<I, K, V>(mut self, metadata: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.instance.instance_metadata = metadata
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        self
    }

    /// Builder: enable the scheduler subsystem and point at a directory
    /// of `*.schedules.yaml` files (issue #352).
    pub fn with_scheduler(mut self, dir: impl Into<PathBuf>) -> Self {
        self.workflow.enable_scheduler = true;
        self.workflow.schedules_dir = Some(dir.into());
        self
    }

    /// Builder: persist tracked jobs in a SQLite database at `path`
    /// (issue #328).
    pub fn with_job_storage_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.job.job_storage_path = Some(path.into());
        self
    }

    /// Builder: choose how the next [`McpHttpServer::start`](crate::McpHttpServer::start)
    /// reacts to in-flight rows persisted by a previous run (issue #567).
    pub fn with_job_recovery(mut self, policy: JobRecoveryPolicy) -> Self {
        self.job.job_recovery = policy;
        self
    }

    /// Builder: declare the DCC capabilities this host provides (issue #354).
    pub fn with_declared_capabilities<I, S>(mut self, caps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.instance.declared_capabilities = caps.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: enable the built-in `workflows.*` tools (issue #348).
    pub fn with_workflows(mut self) -> Self {
        self.workflow.enable_workflows = true;
        self
    }

    /// Builder: enable the lazy-actions fast-path (#254).
    pub fn with_lazy_actions(mut self) -> Self {
        self.features.lazy_actions = true;
        self
    }

    /// Builder: force the legacy `<skill>.<action>` form even when bare
    /// names would be unique (#307).
    pub fn without_bare_tool_names(mut self) -> Self {
        self.features.bare_tool_names = false;
        self
    }

    /// Returns the full socket address string, e.g. `127.0.0.1:8765`.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }

    /// Builder: set server name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.server.server_name = name.into();
        self
    }

    /// Builder: set server version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.server.server_version = version.into();
        self
    }

    /// Builder: allow all interfaces (0.0.0.0). Use with caution.
    pub fn with_all_interfaces(mut self) -> Self {
        self.server.host = IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);
        self
    }

    /// Builder: enable CORS (for browser clients).
    pub fn with_cors(mut self) -> Self {
        self.server.enable_cors = true;
        self
    }

    /// Builder: set request timeout.
    pub fn with_timeout_ms(mut self, ms: u64) -> Self {
        self.server.request_timeout_ms = ms;
        self
    }

    /// Builder: set the idle session TTL. 0 disables background eviction.
    pub fn with_session_ttl_secs(mut self, secs: u64) -> Self {
        self.session.session_ttl_secs = secs;
        self
    }

    /// Builder: enable gateway competition on the given port.
    pub fn with_gateway(mut self, port: u16) -> Self {
        self.gateway.gateway_port = port;
        self
    }

    /// Builder: set the shared FileRegistry directory.
    pub fn with_registry_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.gateway.registry_dir = Some(dir.into());
        self
    }

    /// Builder: set the DCC application type (e.g. `"maya"`).
    pub fn with_dcc_type(mut self, dcc_type: impl Into<String>) -> Self {
        self.instance.dcc_type = Some(dcc_type.into());
        self
    }

    /// Builder: select the listener spawn strategy (issue #303).
    pub fn with_spawn_mode(mut self, mode: ServerSpawnMode) -> Self {
        self.server.spawn_mode = mode;
        self
    }

    /// Builder: override the per-backend gateway fan-out timeout (issue #314).
    pub fn with_backend_timeout_ms(mut self, ms: u64) -> Self {
        self.gateway.backend_timeout_ms = ms;
        self
    }

    /// Builder: override the gateway's async-dispatch timeout (issue #321).
    pub fn with_gateway_async_dispatch_timeout_ms(mut self, ms: u64) -> Self {
        self.gateway.gateway_async_dispatch_timeout_ms = ms;
        self
    }

    /// Builder: override the gateway wait-for-terminal timeout (issue #321).
    pub fn with_gateway_wait_terminal_timeout_ms(mut self, ms: u64) -> Self {
        self.gateway.gateway_wait_terminal_timeout_ms = ms;
        self
    }

    /// Builder: override the gateway's routing-cache TTL (issue #322).
    pub fn with_gateway_route_ttl_secs(mut self, secs: u64) -> Self {
        self.gateway.gateway_route_ttl_secs = secs;
        self
    }

    /// Builder: override the gateway's per-session route cap (issue #322).
    pub fn with_gateway_max_routes_per_session(mut self, cap: u64) -> Self {
        self.gateway.gateway_max_routes_per_session = cap;
        self
    }

    /// Builder: disable the connection-scoped tool-list cache (issue #438).
    pub fn without_tool_cache(mut self) -> Self {
        self.session.enable_tool_cache = false;
        self
    }

    /// Builder: set the deferred-executor queue capacity (issue #715).
    pub fn with_deferred_queue_depth(mut self, depth: usize) -> Self {
        self.queue.deferred_queue_depth = depth;
        self
    }

    /// Builder: set the host-bridge queue capacity (issue #715).
    pub fn with_bridge_queue_depth(mut self, depth: usize) -> Self {
        self.queue.bridge_queue_depth = depth;
        self
    }

    /// Builder: set the host-side `QueueDispatcher` capacity (issue #715).
    pub fn with_host_queue_depth(mut self, depth: usize) -> Self {
        self.queue.host_queue_depth = depth;
        self
    }

    /// Builder: set the send-timeout applied to the bounded channels
    /// (issue #715).
    pub fn with_queue_send_timeout_ms(mut self, ms: u64) -> Self {
        self.queue.queue_send_timeout_ms = ms;
        self
    }

    /// Load the queue-stack knobs from the environment (issue #715).
    ///
    /// Delegates to [`QueueConfig::apply_env_overrides`].
    pub fn apply_queue_env_overrides(mut self) -> Self {
        self.queue = self.queue.apply_env_overrides();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #567: the policy enum defaults to `Drop` so existing callers
    /// inherit today's behaviour without touching their config.
    #[test]
    fn job_recovery_default_is_drop() {
        let cfg = McpHttpConfig::default();
        assert_eq!(cfg.job.job_recovery, JobRecoveryPolicy::Drop);
    }

    /// Issue #567: the builder takes the policy by value and round-trips
    /// to the same wire identifier the Python binding exposes.
    #[test]
    fn job_recovery_builder_round_trips() {
        let cfg = McpHttpConfig::default().with_job_recovery(JobRecoveryPolicy::Requeue);
        assert_eq!(cfg.job.job_recovery, JobRecoveryPolicy::Requeue);
        assert_eq!(cfg.job.job_recovery.as_str(), "requeue");
    }

    /// Issue #567: env-var plumbing (`DCC_MCP_*_JOB_RECOVERY=Requeue`) and
    /// the Python setter share the same case-insensitive parser.
    #[test]
    fn job_recovery_parse_is_case_insensitive() {
        for raw in ["drop", "Drop", "DROP", "  drop  "] {
            assert_eq!(JobRecoveryPolicy::parse(raw), Ok(JobRecoveryPolicy::Drop));
        }
        for raw in ["requeue", "Requeue", "REQUEUE"] {
            assert_eq!(
                JobRecoveryPolicy::parse(raw),
                Ok(JobRecoveryPolicy::Requeue)
            );
        }
    }

    /// Issue #567: unknown policies surface a descriptive error that
    /// names the rejected value and the accepted set.
    #[test]
    fn job_recovery_parse_rejects_unknown() {
        let err = JobRecoveryPolicy::parse("retry").unwrap_err();
        assert!(err.contains("retry"), "error message: {err}");
        assert!(err.contains("drop"), "error message: {err}");
        assert!(err.contains("requeue"), "error message: {err}");
    }

    /// Issue #715: the three queue caps default to the pre-#715
    /// values so existing callers are unaffected.
    #[test]
    fn queue_caps_default_to_pre_715_values() {
        let cfg = McpHttpConfig::default();
        assert_eq!(cfg.queue.deferred_queue_depth, 16);
        assert_eq!(cfg.queue.bridge_queue_depth, 16);
        assert_eq!(cfg.queue.host_queue_depth, 0);
        assert_eq!(cfg.queue.queue_send_timeout_ms, 2_000);
    }

    /// Issue #715: env-var overrides are applied and bad values are
    /// silently ignored (typo safety).
    #[test]
    fn queue_env_overrides_apply_and_ignore_typos() {
        // Use a unique key prefix so parallel tests do not collide.
        // SAFETY: we only touch the four #715 env-vars here; no other
        // test reads them.
        unsafe {
            std::env::set_var("MCP_QUEUE_DEFERRED_CAP", "32");
            std::env::set_var("MCP_QUEUE_BRIDGE_CAP", "64");
            std::env::set_var("MCP_QUEUE_DISPATCHER_CAP", "128");
            std::env::set_var("MCP_QUEUE_SEND_TIMEOUT_MS", "bogus");
        }
        let cfg = McpHttpConfig::default().apply_queue_env_overrides();
        assert_eq!(cfg.queue.deferred_queue_depth, 32);
        assert_eq!(cfg.queue.bridge_queue_depth, 64);
        assert_eq!(cfg.queue.host_queue_depth, 128);
        assert_eq!(
            cfg.queue.queue_send_timeout_ms, 2_000,
            "unparsable value is ignored (typo safety)"
        );
        unsafe {
            std::env::remove_var("MCP_QUEUE_DEFERRED_CAP");
            std::env::remove_var("MCP_QUEUE_BRIDGE_CAP");
            std::env::remove_var("MCP_QUEUE_DISPATCHER_CAP");
            std::env::remove_var("MCP_QUEUE_SEND_TIMEOUT_MS");
        }
    }

    /// Issue #771: payload size limits have sane defaults.
    #[test]
    fn payload_limits_default_values() {
        let cfg = McpHttpConfig::default();
        assert_eq!(cfg.queue.max_request_body_bytes, 4 * 1024 * 1024);
        assert_eq!(cfg.queue.max_response_content_bytes, 1024 * 1024);
        assert_eq!(cfg.queue.sse_chunk_size_bytes, 64 * 1024);
    }

    /// Issue #771: builder methods override the defaults.
    #[test]
    fn payload_limits_builders_override_defaults() {
        let cfg = McpHttpConfig::default();
        let cfg = McpHttpConfig {
            queue: QueueConfig {
                max_request_body_bytes: 8 * 1024 * 1024,
                max_response_content_bytes: 512 * 1024,
                sse_chunk_size_bytes: 32 * 1024,
                ..cfg.queue
            },
            ..cfg
        };
        assert_eq!(cfg.queue.max_request_body_bytes, 8 * 1024 * 1024);
        assert_eq!(cfg.queue.max_response_content_bytes, 512 * 1024);
        assert_eq!(cfg.queue.sse_chunk_size_bytes, 32 * 1024);
    }

    /// Issue #811: `set_host` returns `Err` on malformed input instead of
    /// panicking. Library/PyO3 callers can surface the error structurally.
    #[test]
    fn set_host_returns_err_on_invalid_input() {
        let mut cfg = McpHttpConfig::default();
        let original = cfg.server.host;
        let err = cfg.set_host("not.an.ip.address").unwrap_err();
        // The error type is the canonical std parse error; we mainly assert
        // (a) we got `Err`, not a panic, and (b) the host field is untouched.
        assert!(!err.to_string().is_empty());
        assert_eq!(cfg.server.host, original);
    }

    /// Issue #811: `set_host` accepts a valid IPv4 literal and updates the
    /// underlying `IpAddr` field.
    #[test]
    fn set_host_accepts_valid_ipv4() {
        let mut cfg = McpHttpConfig::default();
        cfg.set_host("10.0.0.5").expect("valid IPv4");
        assert_eq!(cfg.server.host.to_string(), "10.0.0.5");
    }

    /// Issue #811: `set_host` accepts a valid IPv6 literal too.
    #[test]
    fn set_host_accepts_valid_ipv6() {
        let mut cfg = McpHttpConfig::default();
        cfg.set_host("::1").expect("valid IPv6");
        assert_eq!(cfg.server.host.to_string(), "::1");
    }
}
