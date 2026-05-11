//! Configuration value types exposed on the HTTP server wire surface.
//!
//! These are the enums and small value types that the Python binding,
//! CLI flags, and environment-variable plumbing branch on. They live
//! here (rather than in `dcc-mcp-http::config`) so external Rust
//! tooling — CLI drivers, config validators, adapter orchestrators —
//! can depend on just the enumeration contract without dragging in
//! `axum` / `tokio` / `reqwest` / `pyo3`.
//!
//! The full `McpHttpConfig` aggregate stays in `dcc-mcp-http::config`
//! until every sub-struct has migrated; this module captures the
//! self-contained pieces one at a time.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── ServerConfig ───────────────────────────────────────────────────────────

/// Core server identity & transport configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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

// ── ServerSpawnMode ────────────────────────────────────────────────────────

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

// ── JobRecoveryPolicy ──────────────────────────────────────────────────────

/// What `McpHttpServer::start` does with rows that the previous process
/// left in `Pending` / `Running` after a crash or restart (issue #567).
///
/// | Variant | Behaviour |
/// |---------|-----------|
/// | [`JobRecoveryPolicy::Drop`]    | Each in-flight row is rewritten to `JobStatus::Interrupted` with `error = "server restart"`. Clients re-subscribing after reconnect see one clean terminal transition. **This is today's behaviour and the default.** |
/// | [`JobRecoveryPolicy::Requeue`] | Reserved for a future release that persists the original tool arguments alongside the `jobs` row. Until that lands the variant is **accepted but treated as `Drop`** — the server logs a `WARN` at startup so operators know the requested policy is not yet active, but startup itself never fails. The accepted-but-degraded contract gives DCC adapters (`dcc-mcp-maya`, `dcc-mcp-houdini`) a stable knob to plumb through today without forcing a config-shape break when the real implementation lands. |
///
/// String form (used by the Python binding and the `--job-recovery` CLI
/// flag): `"drop"` / `"requeue"`. Defaults to `Drop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Requeue => "requeue",
        }
    }

    /// Parse the wire identifier. `&str` is matched case-insensitively to
    /// be tolerant of env-var plumbing (`DCC_MCP_*_JOB_RECOVERY=Requeue`).
    ///
    /// # Errors
    ///
    /// Returns a descriptive `Err` when `value` does not match any known
    /// variant, naming the rejected value and the accepted set.
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

// ── JobConfig ──────────────────────────────────────────────────────────────

/// Job persistence & recovery configuration.
///
/// One of the orthogonal sub-configs that compose `McpHttpConfig`
/// (issue #852). Captured here as a pure value type so external
/// tooling (config validators, CLI inspectors) can depend on the
/// shape without pulling in the rest of the HTTP server crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobConfig {
    /// Path to a SQLite database file for persisting tracked jobs
    /// (issue #328). `None` means in-memory storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_storage_path: Option<PathBuf>,

    /// What to do with rows the previous process left in `Pending` /
    /// `Running` after a crash or restart (issue #567).
    #[serde(default)]
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

// ── WorkflowConfig ─────────────────────────────────────────────────────────

/// Workflow & scheduler configuration.
///
/// Captures the three opt-in switches that turn on the workflow
/// (`workflows.*` MCP tools, issue #348) and scheduler (issue #352)
/// subsystems. Both default to off so a pristine `McpHttpConfig`
/// boots the minimal surface and operators opt into the heavier
/// subsystems consciously.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// Enable the built-in `workflows.*` tools (issue #348).
    #[serde(default)]
    pub enable_workflows: bool,

    /// Enable the cron + webhook scheduler subsystem (issue #352).
    #[serde(default)]
    pub enable_scheduler: bool,

    /// Directory holding `*.schedules.yaml` files for the scheduler
    /// subsystem (issue #352).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedules_dir: Option<PathBuf>,
}

// ── TelemetryConfig ────────────────────────────────────────────────────────

/// Prometheus metrics configuration.
///
/// One of the orthogonal sub-configs that compose `McpHttpConfig`
/// (issue #852). The `prometheus` Cargo feature on `dcc-mcp-http`
/// gates the actual `/metrics` endpoint; this struct only carries
/// the user-facing knobs so external config validators can
/// round-trip the shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Enable the Prometheus `/metrics` endpoint (issue #331).
    ///
    /// Requires the `prometheus` Cargo feature on both `dcc-mcp-http`
    /// and `dcc-mcp-telemetry`. When `true`, the server mounts a
    /// `GET /metrics` route on the same Axum router that serves
    /// `/mcp`; the body is a standard Prometheus text-exposition
    /// payload (`text/plain; version=0.0.4`).
    ///
    /// Defaults to `false`: the endpoint is opt-in, and when the
    /// feature is compiled out this flag has no effect.
    #[serde(default)]
    pub enable_prometheus: bool,

    /// Optional HTTP Basic auth guard for `/metrics` (issue #331).
    ///
    /// When `Some((user, pass))`, scrapers must present a matching
    /// `Authorization: Basic ...` header or the endpoint responds
    /// with `401 Unauthorized`. When `None` (default), the endpoint
    /// is unauthenticated — acceptable for localhost-only
    /// development but strongly discouraged in production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prometheus_basic_auth: Option<(String, String)>,
}

// ── FeatureFlags ───────────────────────────────────────────────────────────

/// Opt-in capability switches.
///
/// One of the orthogonal sub-configs that compose `McpHttpConfig`
/// (issue #852). Each field is a single boolean knob; the defaults
/// are split — some default `true` because they are the documented
/// shape today (`bare_tool_names`, `enable_resources`, …) — so this
/// struct intentionally provides a hand-written `Default` impl
/// rather than `#[derive(Default)]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    /// Enable the opt-in lazy-actions meta-tools: ``list_actions``,
    /// ``describe_action`` and ``call_action``.
    ///
    /// When `true`, `tools/list` additionally surfaces these three
    /// meta-tools so agents with tight context budgets can drive an
    /// arbitrarily large action catalog through a single page of 3
    /// stubs instead of paging through every loaded skill's tools.
    /// Default: `false`.
    #[serde(default)]
    pub lazy_actions: bool,

    /// Publish skill-scoped tools under their **bare action name**
    /// when no collision exists on this instance (#307).
    ///
    /// When `true` (default), `tools/list` emits `execute_python`
    /// rather than `maya-scripting.execute_python` whenever the bare
    /// name is unique within the instance's loaded skills.
    /// Collisions fall back to the full `<skill>.<action>` form, and
    /// `tools/call` accepts both shapes for one release cycle.
    #[serde(default = "default_true")]
    pub bare_tool_names: bool,

    /// Advertise the MCP Resources primitive (issue #350).
    #[serde(default = "default_true")]
    pub enable_resources: bool,

    /// Advertise the MCP Prompts primitive (issues #351, #355).
    #[serde(default = "default_true")]
    pub enable_prompts: bool,

    /// Expose `artefact://` resources (issue #349).
    #[serde(default)]
    pub enable_artefact_resources: bool,

    /// Emit the `notifications/$/dcc.jobUpdated` and
    /// `notifications/$/dcc.workflowUpdated` SSE channels (issue #326).
    #[serde(default = "default_true")]
    pub enable_job_notifications: bool,

    /// Best-effort safety net for Python callers that drop a
    /// `McpServerHandle` without calling `shutdown()`.
    #[serde(default)]
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

/// Helper for `#[serde(default = ...)]` on the boolean fields whose
/// pre-#852 default was `true`. The function form is required because
/// serde's attribute parser does not accept inline literals here.
fn default_true() -> bool {
    true
}

// ── SessionConfig / GatewayConfig ─────────────────────────────────────────

/// Session lifecycle & tool-cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Idle session TTL in seconds. Sessions that have not received any
    /// request within this window are automatically evicted by a background
    /// task started by the HTTP server. Default: 3600 (1 hour).
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    /// [`ServerConfig::request_timeout_ms`] instead. Fixes issue #314.
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

// ── QueueConfig ────────────────────────────────────────────────────────────

/// Queue depth & backpressure configuration (issue #715).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueConfig {
    /// Capacity of the HTTP → `DccExecutor` mpsc channel (issue #715).
    ///
    /// Controls how many outstanding `tools/call` submissions may queue
    /// up before the backpressure path kicks in. When the channel is
    /// full, the HTTP worker blocks for up to [`Self::queue_send_timeout_ms`]
    /// waiting for the DCC main thread to drain; if the drain does not
    /// happen in time, the call returns a structured
    /// `HttpError::QueueOverloaded`.
    ///
    /// Default: `16`. Override via
    /// `--queue-deferred-cap=<N>` / `MCP_QUEUE_DEFERRED_CAP`.
    pub deferred_queue_depth: usize,

    /// Capacity of the `DeferredExecutor` → `host_bridge` mpsc channel
    /// (issue #715).
    ///
    /// Previously hard-coded to `16`. Exposed as a config knob so
    /// operators can tune the bridge depth without patching the crate.
    /// Set to `0` to fall back to `DEFAULT_BRIDGE_QUEUE_DEPTH`
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
    /// before returning `HttpError::QueueOverloaded`
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
    #[must_use]
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
    #[must_use]
    pub fn with_max_request_body_bytes(mut self, bytes: usize) -> Self {
        self.max_request_body_bytes = bytes;
        self
    }

    /// Builder: set the maximum response content size before truncation
    /// (issue #771).
    ///
    /// Resource, prompt and tool-call responses larger than `bytes` are
    /// wrapped in a `TruncationEnvelope`.
    #[must_use]
    pub fn with_max_response_content_bytes(mut self, bytes: usize) -> Self {
        self.max_response_content_bytes = bytes;
        self
    }

    /// Builder: set the SSE chunking threshold (issue #771).
    ///
    /// SSE events larger than `bytes` are split into sequential
    /// `chunk` / `chunk_end` frames. Set to `0` to disable chunking.
    #[must_use]
    pub fn with_sse_chunk_size_bytes(mut self, bytes: usize) -> Self {
        self.sse_chunk_size_bytes = bytes;
        self
    }
}

// ── InstanceConfig ─────────────────────────────────────────────────────────

/// DCC instance registration metadata.
///
/// One of the orthogonal sub-configs that compose `McpHttpConfig`
/// (issue #852). Reported into the shared `FileRegistry` so the
/// gateway can route by DCC type / version / scene. Captured here
/// as a pure value type — every field is plain string-shaped,
/// nothing carries runtime state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstanceConfig {
    /// DCC application type (e.g. `"maya"`, `"blender"`). Reported in
    /// the shared `FileRegistry` so the gateway can route by DCC
    /// type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,

    /// DCC application version (e.g. `"2025.1"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcc_version: Option<String>,

    /// Currently open scene/file. Improves routing accuracy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,

    /// Arbitrary instance metadata recorded in `FileRegistry`.
    ///
    /// Rez/package launchers use this for context-bundle fields such
    /// as `context_bundle`, `production_domain`, `context_kind`,
    /// `project`, `task`, `toolset_profile`, and `package_provenance`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub instance_metadata: HashMap<String, String>,

    /// Capabilities declared by the DCC adapter hosting this server
    /// (issue #354).
    ///
    /// Each tool may list `required_capabilities` in its sibling
    /// `tools.yaml`; on `tools/call` the server intersects the
    /// tool's requirements against this declared set. Missing
    /// capabilities surface as a `-32001 capability_missing` MCP
    /// error. Tools with unmet capabilities still appear in
    /// `tools/list` but carry `_meta.dcc.missing_capabilities = [...]`
    /// so clients can filter.
    ///
    /// The list is freeform — conventionally lowercase dotted
    /// identifiers like `"usd"`, `"scene.mutate"`,
    /// `"filesystem.read"`. Adapters hard-code it at construction
    /// time; there is no runtime introspection of the DCC.
    ///
    /// Default: empty (no capabilities declared — any tool with
    /// declared requirements will report them as missing).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_capabilities: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ServerSpawnMode ────────────────────────────────────────────────

    #[test]
    fn server_spawn_mode_defaults_to_ambient() {
        assert_eq!(ServerSpawnMode::default(), ServerSpawnMode::Ambient);
    }

    #[test]
    fn server_spawn_mode_wire_is_snake_case() {
        // `ambient` / `dedicated` is the wire form the Python binding
        // and env-var plumbing round-trip. Pin it so a future derive
        // tweak cannot silently break downstream consumers.
        assert_eq!(
            serde_json::to_string(&ServerSpawnMode::Ambient).unwrap(),
            "\"ambient\""
        );
        assert_eq!(
            serde_json::to_string(&ServerSpawnMode::Dedicated).unwrap(),
            "\"dedicated\""
        );

        let back: ServerSpawnMode = serde_json::from_str("\"dedicated\"").unwrap();
        assert_eq!(back, ServerSpawnMode::Dedicated);
    }

    // ── JobRecoveryPolicy ──────────────────────────────────────────────

    /// Issue #567: the policy enum defaults to `Drop` so existing callers
    /// inherit today's behaviour without touching their config.
    #[test]
    fn job_recovery_default_is_drop() {
        assert_eq!(JobRecoveryPolicy::default(), JobRecoveryPolicy::Drop);
    }

    /// Issue #567: the wire identifier round-trips to the same shape the
    /// Python binding exposes.
    #[test]
    fn job_recovery_as_str_matches_wire() {
        assert_eq!(JobRecoveryPolicy::Drop.as_str(), "drop");
        assert_eq!(JobRecoveryPolicy::Requeue.as_str(), "requeue");
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

    /// The snake_case JSON form matches the CLI / env-var string form,
    /// so operators can read either serialisation interchangeably.
    #[test]
    fn job_recovery_wire_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&JobRecoveryPolicy::Drop).unwrap(),
            "\"drop\""
        );
        assert_eq!(
            serde_json::to_string(&JobRecoveryPolicy::Requeue).unwrap(),
            "\"requeue\""
        );

        let back: JobRecoveryPolicy = serde_json::from_str("\"requeue\"").unwrap();
        assert_eq!(back, JobRecoveryPolicy::Requeue);
    }

    // ── JobConfig ──────────────────────────────────────────────────────

    #[test]
    fn job_config_default_is_in_memory_with_drop_policy() {
        let cfg = JobConfig::default();
        assert!(cfg.job_storage_path.is_none());
        assert_eq!(cfg.job_recovery, JobRecoveryPolicy::Drop);
    }

    #[test]
    fn job_config_serialises_skip_none_storage() {
        // `job_storage_path: None` is the default — keeping it out of
        // the JSON serialisation keeps round-tripped configs compact
        // and matches the CLI default (no `--job-storage-path` flag).
        let cfg = JobConfig::default();
        let s = serde_json::to_string(&cfg).unwrap();
        assert!(!s.contains("job_storage_path"), "got: {s}");
        assert!(s.contains("\"job_recovery\":\"drop\""), "got: {s}");
    }

    #[test]
    fn job_config_round_trips_with_storage_path() {
        let cfg = JobConfig {
            job_storage_path: Some(PathBuf::from("/var/lib/dcc/jobs.sqlite")),
            job_recovery: JobRecoveryPolicy::Requeue,
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let back: JobConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.job_storage_path, cfg.job_storage_path);
        assert_eq!(back.job_recovery, cfg.job_recovery);
    }

    #[test]
    fn job_config_accepts_minimal_body() {
        // Operators frequently send a 2-key partial in env-var
        // configs. Both fields default, so a `{}` body must still
        // deserialise to the documented defaults — anything else
        // would surprise CLI / Python plumbing.
        let cfg: JobConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.job_storage_path.is_none());
        assert_eq!(cfg.job_recovery, JobRecoveryPolicy::Drop);
    }

    // ── WorkflowConfig ─────────────────────────────────────────────────

    #[test]
    fn workflow_config_default_disables_both_subsystems() {
        // Pristine boot must surface only the minimal MCP tools, so
        // both opt-in switches default to `false`. Operators flip
        // them on consciously when they are ready to pay the
        // workflow / scheduler runtime cost.
        let cfg = WorkflowConfig::default();
        assert!(!cfg.enable_workflows);
        assert!(!cfg.enable_scheduler);
        assert!(cfg.schedules_dir.is_none());
    }

    #[test]
    fn workflow_config_round_trips() {
        let cfg = WorkflowConfig {
            enable_workflows: true,
            enable_scheduler: true,
            schedules_dir: Some(PathBuf::from("/etc/dcc/schedules")),
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let back: WorkflowConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.enable_workflows, cfg.enable_workflows);
        assert_eq!(back.enable_scheduler, cfg.enable_scheduler);
        assert_eq!(back.schedules_dir, cfg.schedules_dir);
    }

    #[test]
    fn workflow_config_skip_none_schedules_dir() {
        let cfg = WorkflowConfig::default();
        let s = serde_json::to_string(&cfg).unwrap();
        assert!(!s.contains("schedules_dir"), "got: {s}");
    }

    #[test]
    fn workflow_config_accepts_minimal_body() {
        let cfg: WorkflowConfig = serde_json::from_str("{}").unwrap();
        assert!(!cfg.enable_workflows);
        assert!(!cfg.enable_scheduler);
        assert!(cfg.schedules_dir.is_none());
    }

    // ── TelemetryConfig ────────────────────────────────────────────────

    #[test]
    fn telemetry_config_default_is_disabled_and_unauth() {
        // Pristine boot must NOT expose `/metrics` and must NOT
        // accept arbitrary scrapers — operators flip both knobs on
        // consciously.
        let cfg = TelemetryConfig::default();
        assert!(!cfg.enable_prometheus);
        assert!(cfg.prometheus_basic_auth.is_none());
    }

    #[test]
    fn telemetry_config_round_trips_with_basic_auth() {
        let cfg = TelemetryConfig {
            enable_prometheus: true,
            prometheus_basic_auth: Some(("scraper".into(), "s3cret".into())),
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let back: TelemetryConfig = serde_json::from_str(&s).unwrap();
        assert!(back.enable_prometheus);
        assert_eq!(
            back.prometheus_basic_auth,
            Some(("scraper".to_owned(), "s3cret".to_owned()))
        );
    }

    #[test]
    fn telemetry_config_skips_none_basic_auth() {
        let cfg = TelemetryConfig::default();
        let s = serde_json::to_string(&cfg).unwrap();
        // Default config must not leak `prometheus_basic_auth: null`
        // into the wire form — keeps env-var/config-file dumps tidy.
        assert!(!s.contains("prometheus_basic_auth"), "got: {s}");
    }

    #[test]
    fn telemetry_config_accepts_minimal_body() {
        let cfg: TelemetryConfig = serde_json::from_str("{}").unwrap();
        assert!(!cfg.enable_prometheus);
        assert!(cfg.prometheus_basic_auth.is_none());
    }

    // ── FeatureFlags ───────────────────────────────────────────────────

    /// Pin every default boolean of [`FeatureFlags`]. Most are `false`,
    /// but `bare_tool_names`, `enable_resources`, `enable_prompts`,
    /// and `enable_job_notifications` default to `true` because that
    /// is the documented pre-#852 surface the wheel ships with.
    /// A future change to any of these defaults must be conscious;
    /// this test is the regression guard.
    #[test]
    fn feature_flags_default_matches_documented_pre_852_surface() {
        let f = FeatureFlags::default();
        assert!(!f.lazy_actions);
        assert!(f.bare_tool_names);
        assert!(f.enable_resources);
        assert!(f.enable_prompts);
        assert!(!f.enable_artefact_resources);
        assert!(f.enable_job_notifications);
        assert!(!f.shutdown_on_drop);
    }

    #[test]
    fn feature_flags_round_trip() {
        let f = FeatureFlags::default();
        let s = serde_json::to_string(&f).unwrap();
        let back: FeatureFlags = serde_json::from_str(&s).unwrap();
        assert_eq!(back.lazy_actions, f.lazy_actions);
        assert_eq!(back.bare_tool_names, f.bare_tool_names);
        assert_eq!(back.enable_resources, f.enable_resources);
        assert_eq!(back.enable_prompts, f.enable_prompts);
        assert_eq!(back.enable_artefact_resources, f.enable_artefact_resources);
        assert_eq!(back.enable_job_notifications, f.enable_job_notifications);
        assert_eq!(back.shutdown_on_drop, f.shutdown_on_drop);
    }

    /// Critical contract: an empty `{}` body must deserialise into
    /// the documented Default surface, NOT into "every flag is
    /// `false`". The four `default = "default_true"` annotations
    /// are what keep this guarantee — drop one and the wheel
    /// silently regresses to a different `tools/list` shape.
    #[test]
    fn feature_flags_minimal_body_uses_per_field_defaults() {
        let f: FeatureFlags = serde_json::from_str("{}").unwrap();
        let d = FeatureFlags::default();
        assert_eq!(f.lazy_actions, d.lazy_actions);
        assert_eq!(f.bare_tool_names, d.bare_tool_names);
        assert_eq!(f.enable_resources, d.enable_resources);
        assert_eq!(f.enable_prompts, d.enable_prompts);
        assert_eq!(f.enable_artefact_resources, d.enable_artefact_resources);
        assert_eq!(f.enable_job_notifications, d.enable_job_notifications);
        assert_eq!(f.shutdown_on_drop, d.shutdown_on_drop);
    }

    #[test]
    fn feature_flags_partial_body_inherits_other_defaults() {
        // Operators only flip `lazy_actions` on; every other knob
        // must keep its documented default.
        let f: FeatureFlags = serde_json::from_str(r#"{"lazy_actions": true}"#).unwrap();
        assert!(f.lazy_actions);
        // The defaults still hold for unmentioned fields:
        assert!(f.bare_tool_names);
        assert!(f.enable_resources);
        assert!(f.enable_prompts);
        assert!(f.enable_job_notifications);
        // And the `false`-by-default ones stay `false`:
        assert!(!f.enable_artefact_resources);
        assert!(!f.shutdown_on_drop);
    }

    // ── InstanceConfig ─────────────────────────────────────────────────

    #[test]
    fn instance_config_default_is_anonymous() {
        // A pristine InstanceConfig must reveal nothing about the
        // host adapter — every field is None / empty so that
        // `FileRegistry` rows from a misconfigured launcher do not
        // accidentally claim to be Maya / Blender / etc.
        let cfg = InstanceConfig::default();
        assert!(cfg.dcc_type.is_none());
        assert!(cfg.dcc_version.is_none());
        assert!(cfg.scene.is_none());
        assert!(cfg.instance_metadata.is_empty());
        assert!(cfg.declared_capabilities.is_empty());
    }

    #[test]
    fn instance_config_round_trips() {
        let mut metadata = HashMap::new();
        metadata.insert("project".to_owned(), "shotpack".to_owned());
        metadata.insert("task".to_owned(), "lighting".to_owned());

        let cfg = InstanceConfig {
            dcc_type: Some("maya".into()),
            dcc_version: Some("2025.1".into()),
            scene: Some("/tmp/scene.ma".into()),
            instance_metadata: metadata.clone(),
            declared_capabilities: vec!["usd".into(), "scene.mutate".into()],
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let back: InstanceConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.dcc_type, cfg.dcc_type);
        assert_eq!(back.dcc_version, cfg.dcc_version);
        assert_eq!(back.scene, cfg.scene);
        assert_eq!(back.instance_metadata, metadata);
        assert_eq!(back.declared_capabilities, cfg.declared_capabilities);
    }

    /// Every optional / collection field carries
    /// `skip_serializing_if = ...` so a pristine `InstanceConfig`
    /// serialises to the literal `"{}"`. Pin this so a future field
    /// addition does not silently bloat config dumps with `null`s
    /// and empty arrays.
    #[test]
    fn instance_config_default_serialises_empty_object() {
        let cfg = InstanceConfig::default();
        let s = serde_json::to_string(&cfg).unwrap();
        assert_eq!(s, "{}", "default config must serialise to empty object");
    }

    #[test]
    fn instance_config_accepts_minimal_body() {
        // `{}` must deserialise to defaults. Operators routinely
        // boot a server without any of these fields set, then patch
        // the registry row in via subsequent calls; if `{}` failed
        // to deserialise, the boot sequence would break.
        let cfg: InstanceConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.dcc_type.is_none());
        assert!(cfg.declared_capabilities.is_empty());
    }
}
