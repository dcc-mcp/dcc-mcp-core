//! Server configuration.

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

/// Configuration for [`McpHttpServer`](crate::McpHttpServer).
#[derive(Debug, Clone)]
pub struct McpHttpConfig {
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

    /// Idle session TTL in seconds. Sessions that have not received any
    /// request within this window are automatically evicted by a background
    /// task started in [`McpHttpServer::start`]. Default: 3600 (1 hour).
    /// Set to 0 to disable automatic eviction.
    pub session_ttl_secs: u64,

    // ── Gateway configuration ──────────────────────────────────────────────
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

    // ── Instance registration metadata ────────────────────────────────────
    /// DCC application type (e.g. `"maya"`, `"blender"`). Reported in the
    /// shared `FileRegistry` so the gateway can route by DCC type.
    pub dcc_type: Option<String>,

    /// DCC application version (e.g. `"2025.1"`).
    pub dcc_version: Option<String>,

    /// Currently open scene/file. Improves routing accuracy.
    pub scene: Option<String>,

    // ── Experimental: lazy-actions fast-path (#254) ───────────────────────
    /// Enable the opt-in lazy-actions meta-tools: ``list_actions``,
    /// ``describe_action`` and ``call_action``.
    ///
    /// When `true`, `tools/list` additionally surfaces these three meta-tools
    /// so agents with tight context budgets can drive an arbitrarily large
    /// action catalog through a single page of 3 stubs instead of paging
    /// through every loaded skill's tools. Default: `false`.
    ///
    /// Clients may also flip this on via
    /// `initialize.capabilities.experimental["dcc_mcp_core/lazyActions"]`
    /// (per-session, negotiated at initialize time).
    pub lazy_actions: bool,

    /// Publish skill-scoped tools under their **bare action name** when no
    /// collision exists on this instance (#307).
    ///
    /// When `true` (default), `tools/list` emits `execute_python` rather than
    /// `maya-scripting.execute_python` whenever the bare name is unique
    /// within the instance's loaded skills. Collisions fall back to the
    /// full `<skill>.<action>` form, and `tools/call` accepts both shapes
    /// for one release cycle.
    ///
    /// Downstream clients that hard-coded the prefixed form can opt out by
    /// setting this to `false` (or flipping `DCC_MCP_BARE_TOOL_NAMES=0`
    /// at the binary level).
    pub bare_tool_names: bool,

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

    /// Advertise the MCP Resources primitive (issue #350).
    ///
    /// When `true` (default), the server:
    /// - Advertises `resources: { subscribe: true, listChanged: true }`
    ///   in its `initialize` response.
    /// - Handles `resources/list`, `resources/read`, `resources/subscribe`
    ///   and `resources/unsubscribe` JSON-RPC methods.
    /// - Surfaces four URI schemes: `scene://current` (JSON scene summary),
    ///   `capture://current_window` (PNG snapshot of the DCC window, only
    ///   enabled when a real window backend is available), `audit://recent`
    ///   (tail of the `AuditLog`), and `artefact://…` (stub reserved for
    ///   issue #349).
    ///
    /// Set to `false` to hide the capability entirely — useful for minimal
    /// servers or when Resources are provided by an external MCP host.
    pub enable_resources: bool,

    /// Advertise the MCP Prompts primitive (issues #351, #355).
    ///
    /// When `true` (default), the server:
    /// - Advertises `prompts: { listChanged: true }` in `initialize`.
    /// - Handles `prompts/list` and `prompts/get` JSON-RPC methods.
    /// - Parses the sibling `prompts.yaml` file referenced by
    ///   `metadata.dcc-mcp.prompts` on each loaded skill and merges its
    ///   `prompts:` and workflow-derived entries into `prompts/list`.
    /// - Emits `notifications/prompts/list_changed` on skill load/unload.
    ///
    /// Set to `false` to hide the capability — appropriate for minimal
    /// servers, or when prompts are provided by an external MCP host.
    pub enable_prompts: bool,

    /// Expose `artefact://` resources (issue #349).
    ///
    /// The full artefact store ships separately in issue #349. When this
    /// flag is `false` (default), `resources/list` omits `artefact://`
    /// entries and `resources/read` returns a
    /// [`protocol::RESOURCE_NOT_ENABLED_ERROR`](crate::protocol::RESOURCE_NOT_ENABLED_ERROR)
    /// JSON-RPC error so callers can distinguish "scheme unknown" from
    /// "scheme recognized but backing store not enabled yet".
    pub enable_artefact_resources: bool,

    /// Per-backend request timeout (milliseconds) used by the gateway when
    /// fanning out `tools/list` / `tools/call` to live DCC instances.
    ///
    /// Default: `10_000` (10 seconds). Increase for DCC workflows that
    /// routinely produce long-running calls (e.g. heavy scene import,
    /// simulation bake) so the gateway does not reply with a transport
    /// timeout error while the backend is still legitimately working.
    ///
    /// Only the gateway fan-out uses this value — per-instance servers
    /// bound to a DCC execute inline and are governed by
    /// [`Self::request_timeout_ms`] instead. Fixes issue #314.
    pub backend_timeout_ms: u64,

    /// Per-backend request timeout (milliseconds) applied by the gateway
    /// when the client has opted into **async dispatch** (issue #321).
    ///
    /// Triggered when any of the following signals are present on the
    /// outbound `tools/call`:
    ///
    /// * `_meta.dcc.async == true` (explicit client opt-in).
    /// * `_meta.progressToken` is set (MCP 2025-03-26 long-running hint).
    /// * The target tool declares `execution: async` or a non-zero
    ///   `timeout_hint_secs` in its [`dcc_mcp_models::ActionMeta`].
    ///
    /// The async dispatch path only has to **queue** the job on the
    /// backend (reply is `{status:"pending", job_id:"..."}`), but cold
    /// starts or heavy imports can still legitimately push the queuing
    /// step past [`Self::backend_timeout_ms`]. This longer timeout
    /// prevents the gateway from returning a spurious transport error
    /// while the backend is still starting the job.
    ///
    /// Default: `60_000` (60 seconds).
    pub gateway_async_dispatch_timeout_ms: u64,

    /// Gateway timeout (milliseconds) for the opt-in wait-for-terminal
    /// response-stitching mode (issue #321).
    ///
    /// When a client sends `_meta.dcc.wait_for_terminal = true` on an
    /// async `tools/call`, the gateway blocks the POST response until
    /// a `notifications/$/dcc.jobUpdated` with a terminal status
    /// (`completed`, `failed`, `cancelled`) arrives over the backend
    /// SSE stream. On timeout the gateway returns the last known job
    /// envelope annotated with `_meta.dcc.timed_out = true` and leaves
    /// the job running on the backend — the caller can keep polling
    /// `jobs.get_status` or reconnect SSE to collect the result later.
    ///
    /// Default: `600_000` (10 minutes).
    pub gateway_wait_terminal_timeout_ms: u64,

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

    /// Enable the built-in `workflows.*` tools (issue #348).
    ///
    /// Default: `false`. When `true`, [`McpHttpServer::start`] registers
    /// `workflows.run` / `workflows.get_status` / `workflows.cancel` /
    /// `workflows.lookup` on the registry before the listener comes up.
    ///
    /// **Skeleton note**: in this release the three execution-facing tools
    /// return a structured `"step execution pending follow-up PR"` error;
    /// `workflows.lookup` is fully functional against the `WorkflowCatalog`.
    /// Surface-area is stable so downstream adapters can wire the tools
    /// today and pick up real execution when the follow-up PR lands.
    pub enable_workflows: bool,

    /// Emit the `notifications/$/dcc.jobUpdated` and
    /// `notifications/$/dcc.workflowUpdated` SSE channels (issue #326).
    ///
    /// Default: `true`. When `false`, the server still emits the
    /// spec-mandated `notifications/progress` channel for callers that
    /// supplied `_meta.progressToken`, but the `$/dcc.*` vendor extensions
    /// are suppressed.
    ///
    /// The flag is checked at server start — disabling it after `start()`
    /// has no effect. Use a capability-gated per-session opt-in (future
    /// work, see #326 amendment) for per-client control.
    pub enable_job_notifications: bool,

    /// Path to a SQLite database file for persisting tracked jobs
    /// (issue #328).
    ///
    /// When set **and** the `job-persist-sqlite` Cargo feature is
    /// enabled, [`McpHttpServer::start`] opens the file, runs schema
    /// migrations, and attaches it to `JobManager` as a write-through
    /// store. On startup, any pre-existing rows whose status is
    /// `Pending` or `Running` are rewritten to
    /// [`JobStatus::Interrupted`](crate::job::JobStatus::Interrupted)
    /// with `error = "server restart"` so clients never see silently
    /// "lost" jobs.
    ///
    /// When set but the feature is **not** compiled in, `start()`
    /// returns a descriptive error — the server refuses to silently
    /// run without the persistence the caller asked for.
    ///
    /// Default: `None` (in-memory storage; no persistence).
    pub job_storage_path: Option<PathBuf>,

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
    ///
    /// [`required_capabilities`]: dcc_mcp_models::ToolDeclaration::required_capabilities
    pub declared_capabilities: Vec<String>,

    /// Enable the cron + webhook scheduler subsystem (issue #352).
    ///
    /// Default: `false`. When `true`, the server loads every
    /// `*.schedules.yaml` file in [`Self::schedules_dir`] at startup,
    /// spawns one Tokio task per enabled cron schedule, and mounts each
    /// declared webhook route on the main Axum router.
    ///
    /// The scheduler is provided by the optional `dcc-mcp-scheduler`
    /// crate (feature-gated at the workspace root). When the crate is not
    /// compiled in, this flag has no effect.
    pub enable_scheduler: bool,

    /// Directory holding `*.schedules.yaml` files for the scheduler
    /// subsystem (issue #352).
    ///
    /// Only consulted when [`Self::enable_scheduler`] is `true`. Paths
    /// are loaded non-recursively. A `None` value pairs with
    /// `enable_scheduler = true` as a no-op (empty schedule set).
    pub schedules_dir: Option<PathBuf>,
}

impl McpHttpConfig {
    /// Create a config with the given port and sensible defaults.
    pub fn new(port: u16) -> Self {
        Self {
            port,
            host: IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            endpoint_path: "/mcp".to_string(),
            server_name: "dcc-mcp".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            max_sessions: 100,
            request_timeout_ms: 30_000,
            enable_cors: false,
            session_ttl_secs: 3_600,
            gateway_port: 0,
            registry_dir: None,
            stale_timeout_secs: 30,
            heartbeat_secs: 5,
            dcc_type: None,
            dcc_version: None,
            scene: None,
            lazy_actions: false,
            bare_tool_names: true,
            spawn_mode: ServerSpawnMode::Ambient,
            self_probe_timeout_ms: 200,
            backend_timeout_ms: 10_000,
            gateway_async_dispatch_timeout_ms: 60_000,
            gateway_wait_terminal_timeout_ms: 600_000,
            enable_resources: true,
            enable_artefact_resources: false,
            enable_prompts: true,
            enable_workflows: false,
            enable_prometheus: false,
            prometheus_basic_auth: None,
            enable_job_notifications: true,
            job_storage_path: None,
            declared_capabilities: Vec::new(),
            enable_scheduler: false,
            schedules_dir: None,
        }
    }

    /// Builder: enable the scheduler subsystem and point at a directory
    /// of `*.schedules.yaml` files (issue #352).
    pub fn with_scheduler(mut self, dir: impl Into<PathBuf>) -> Self {
        self.enable_scheduler = true;
        self.schedules_dir = Some(dir.into());
        self
    }

    /// Builder: persist tracked jobs in a SQLite database at `path`
    /// (issue #328).
    ///
    /// Requires the `job-persist-sqlite` Cargo feature; otherwise
    /// [`McpHttpServer::start`](crate::McpHttpServer::start) fails
    /// with a descriptive error at startup.
    pub fn with_job_storage_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.job_storage_path = Some(path.into());
        self
    }

    /// Builder: declare the DCC capabilities this host provides (issue #354).
    ///
    /// Replaces any existing capability list. Pass freeform string tags like
    /// `"usd"`, `"scene.mutate"`, `"filesystem.read"`.
    pub fn with_declared_capabilities<I, S>(mut self, caps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.declared_capabilities = caps.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: enable the built-in `workflows.*` tools (issue #348).
    ///
    /// See [`Self::enable_workflows`] for the full contract.
    pub fn with_workflows(mut self) -> Self {
        self.enable_workflows = true;
        self
    }

    /// Builder: enable the lazy-actions fast-path (#254).
    ///
    /// Surfaces `list_actions`, `describe_action` and `call_action` as
    /// core MCP tools. Useful for agents whose context budget cannot
    /// afford paging through every skill's full schema.
    pub fn with_lazy_actions(mut self) -> Self {
        self.lazy_actions = true;
        self
    }

    /// Builder: force the legacy `<skill>.<action>` form even when bare
    /// names would be unique (#307).
    ///
    /// Useful for downstream clients that hard-coded the prefixed shape and
    /// cannot be updated in lock-step with the server.
    pub fn without_bare_tool_names(mut self) -> Self {
        self.bare_tool_names = false;
        self
    }

    /// Returns the full socket address string, e.g. `127.0.0.1:8765`.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Builder: set server name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = name.into();
        self
    }

    /// Builder: set server version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.server_version = version.into();
        self
    }

    /// Builder: allow all interfaces (0.0.0.0). Use with caution.
    pub fn with_all_interfaces(mut self) -> Self {
        self.host = IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);
        self
    }

    /// Builder: enable CORS (for browser clients).
    pub fn with_cors(mut self) -> Self {
        self.enable_cors = true;
        self
    }

    /// Builder: set request timeout.
    pub fn with_timeout_ms(mut self, ms: u64) -> Self {
        self.request_timeout_ms = ms;
        self
    }

    /// Builder: set the idle session TTL. 0 disables background eviction.
    pub fn with_session_ttl_secs(mut self, secs: u64) -> Self {
        self.session_ttl_secs = secs;
        self
    }

    /// Builder: enable gateway competition on the given port.
    ///
    /// The first process to bind this port becomes the gateway. Others run as
    /// plain DCC instances and register themselves in the shared `FileRegistry`.
    pub fn with_gateway(mut self, port: u16) -> Self {
        self.gateway_port = port;
        self
    }

    /// Builder: set the shared FileRegistry directory.
    pub fn with_registry_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.registry_dir = Some(dir.into());
        self
    }

    /// Builder: set the DCC application type (e.g. `"maya"`).
    pub fn with_dcc_type(mut self, dcc_type: impl Into<String>) -> Self {
        self.dcc_type = Some(dcc_type.into());
        self
    }

    /// Builder: select the listener spawn strategy (issue #303).
    ///
    /// Defaults to [`ServerSpawnMode::Ambient`]. Use
    /// [`ServerSpawnMode::Dedicated`] for PyO3-embedded callers so that
    /// listener accept loops are not starved of scheduling time when the
    /// parent runtime has no active driver thread.
    pub fn with_spawn_mode(mut self, mode: ServerSpawnMode) -> Self {
        self.spawn_mode = mode;
        self
    }

    /// Builder: override the per-backend gateway fan-out timeout (issue #314).
    ///
    /// Default: `10_000` ms. Raise this for workflows whose backend tools
    /// legitimately run longer than 10 seconds (scene import, simulation
    /// bake, large USD composition) so the gateway does not short-circuit
    /// them with a transport timeout error.
    pub fn with_backend_timeout_ms(mut self, ms: u64) -> Self {
        self.backend_timeout_ms = ms;
        self
    }

    /// Builder: override the gateway's async-dispatch timeout (issue #321).
    ///
    /// See [`Self::gateway_async_dispatch_timeout_ms`] for the full contract.
    pub fn with_gateway_async_dispatch_timeout_ms(mut self, ms: u64) -> Self {
        self.gateway_async_dispatch_timeout_ms = ms;
        self
    }

    /// Builder: override the gateway wait-for-terminal timeout (issue #321).
    ///
    /// See [`Self::gateway_wait_terminal_timeout_ms`] for the full contract.
    pub fn with_gateway_wait_terminal_timeout_ms(mut self, ms: u64) -> Self {
        self.gateway_wait_terminal_timeout_ms = ms;
        self
    }
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self::new(8765)
    }
}
