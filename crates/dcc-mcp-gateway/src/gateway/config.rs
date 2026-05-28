use std::path::PathBuf;
use std::sync::Arc;

use dcc_mcp_gateway_core::policy::GatewayPolicy;

pub use super::relay_registration::RelaySourceConfig;

/// Admin persistence configuration (SQLite + skill-path reload hook).
///
/// Grouped to satisfy the Open/Closed Principle: adding new admin-persist
/// knobs extends this struct without changing `start_gateway_tasks` or
/// `GatewayRunner` call sites.
pub struct AdminPersistConfig {
    /// Optional explicit path for the gateway admin SQLite database
    /// (traces, audits, custom skill paths). When `None`, uses
    /// `DCC_MCP_GATEWAY_ADMIN_DB` or `<registry>/gateway_admin.sqlite`.
    pub sqlite_path: Option<PathBuf>,

    /// Retention window for rows in the admin SQLite database (days).
    ///
    /// Default: `30`. Override with `DCC_MCP_GATEWAY_ADMIN_RETENTION_DAYS`.
    pub sqlite_retention_days: u32,

    /// Snapshot of skill search paths (for the admin UI); populated by embedders.
    pub skill_paths_snapshot: Vec<super::SkillPathEntry>,

    /// When set, invoked after admin API adds/removes a custom SQLite skill path
    /// so embedders can re-run `SkillCatalog::discover` without restarting the process.
    pub skill_paths_reload: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Default for AdminPersistConfig {
    fn default() -> Self {
        Self {
            sqlite_path: None,
            sqlite_retention_days: 30,
            skill_paths_snapshot: Vec::new(),
            skill_paths_reload: None,
        }
    }
}

impl Clone for AdminPersistConfig {
    fn clone(&self) -> Self {
        Self {
            sqlite_path: self.sqlite_path.clone(),
            sqlite_retention_days: self.sqlite_retention_days,
            skill_paths_snapshot: self.skill_paths_snapshot.clone(),
            skill_paths_reload: self.skill_paths_reload.clone(),
        }
    }
}

/// Configuration for the optional gateway.
pub struct GatewayConfig {
    /// Host to bind the gateway port on (default: `"127.0.0.1"`).
    pub host: String,
    /// Well-known port to compete for. `0` disables the gateway.
    pub gateway_port: u16,
    /// Optional second gateway listener for remote/LAN clients.
    ///
    /// This listener does not participate in gateway election; the local
    /// `host:gateway_port` bind remains the single authority.
    pub remote_host: Option<String>,
    /// Optional second gateway port for remote/LAN clients. `0` disables it.
    pub remote_gateway_port: u16,
    /// Seconds without heartbeat before an instance is considered stale.
    pub stale_timeout_secs: u64,
    /// Heartbeat interval in seconds. `0` disables the heartbeat task.
    pub heartbeat_secs: u64,
    /// Server name advertised in gateway `initialize` responses.
    pub server_name: String,
    /// Human-readable identity for the process currently competing for or
    /// serving the gateway role. Written to the `__gateway__` sentinel so
    /// operators can tell which peer owns the well-known port.
    pub gateway_name: Option<String>,
    /// Server version advertised in gateway `initialize` responses.
    pub server_version: String,
    /// Shared `FileRegistry` directory. `None` falls back to a temp dir.
    pub registry_dir: Option<PathBuf>,
    /// How many seconds a newer-version challenger waits for the old gateway
    /// to yield before giving up and running as a plain instance.
    ///
    /// Default: `120` seconds (12 × 10-second retry intervals).
    pub challenger_timeout_secs: u64,
    /// Seconds between challenger bind attempts after an incumbent yields.
    ///
    /// Default: `10` seconds. Tests and embedded adapters may lower this
    /// when they need faster failover without changing the overall timeout.
    pub challenger_poll_interval_secs: u64,
    /// Per-backend request timeout (milliseconds) used for fan-out calls
    /// from the gateway to each live DCC instance. Default: `10_000`.
    /// Issue #314.
    pub backend_timeout_ms: u64,
    /// Longer timeout applied when the outbound `tools/call` is async-
    /// opted-in (issue #321). Default: `60_000`.
    pub async_dispatch_timeout_ms: u64,
    /// Gateway wait-for-terminal passthrough timeout (issue #321).
    /// Default: `600_000` (10 minutes).
    pub wait_terminal_timeout_ms: u64,
    /// TTL (seconds) for cached [`JobRoute`] entries in the gateway
    /// routing cache (issue #322). Routes older than this are evicted
    /// by a background GC task even if no terminal event was observed.
    /// Default: `86_400` (24 hours).
    ///
    /// [`JobRoute`]: super::sse_subscriber::JobRoute
    pub route_ttl_secs: u64,
    /// Per-session ceiling on concurrent live routes (issue #322). `0`
    /// disables the cap. Default: `1_000`.
    pub max_routes_per_session: u64,
    /// Allow instances with `dcc_type == "unknown"` to expose their tools
    /// via the gateway's `tools/list` and be reachable through `connect_to_dcc`.
    ///
    /// Default: `false`. Standalone `dcc-mcp-server` binaries that register
    /// with `dcc_type: "unknown"` should not leak tools into the gateway
    /// façade unless this is explicitly enabled for development (issue #555).
    pub allow_unknown_tools: bool,
    /// Enable LAN-local mDNS/DNS-SD browsing for `_dcc-mcp._tcp.local`.
    ///
    /// Default: `false`. This is advisory discovery only; a resolved endpoint
    /// must still answer the HTTP health probe before it is surfaced.
    #[cfg(feature = "mdns")]
    pub discover_mdns: bool,
    /// Remote tunnel relays to poll for active DCC backends.
    ///
    /// Each source needs a private/admin URL for `GET /tunnels` and a public
    /// HTTP(S) frontend base URL that proxies `/tunnel/{id}/...` to the local
    /// MCP server behind the relay. Default: empty.
    pub relay_sources: Vec<RelaySourceConfig>,
    /// Adapter package version recorded on the `__gateway__` sentinel
    /// (e.g. `dcc_mcp_maya = "0.3.0"`). Used by the second tier of the
    /// election comparison (issue maya#137).
    pub adapter_version: Option<String>,
    /// DCC type the adapter is bound to (e.g. `"maya"`). Used by the
    /// third-tier real-DCC tiebreaker (issue maya#137).
    pub adapter_dcc: Option<String>,

    /// Pre-registered middleware chain applied to every `tools/call` (issue #770).
    pub middleware_chain: super::middleware::MiddlewareChain,

    /// Policy applied to gateway dynamic-capability discovery, describe,
    /// skill loading, and calls.
    pub policy: GatewayPolicy,

    /// Enable the read-only `/admin` web UI (issue #772).
    ///
    /// Default: `true`. Disable explicitly for locked-down deployments.
    /// Only the process that wins the gateway election serves admin, so
    /// multi-instance launches still expose exactly one dashboard.
    pub admin_enabled: bool,

    /// URL prefix for the admin dashboard (issue #772).
    ///
    /// Default: `"/admin"`. The gateway mounts all admin routes under
    /// this prefix.
    pub admin_path: String,

    /// Interval in seconds between backend health-check probes (issue #854).
    ///
    /// Default: `5`. Override at runtime with
    /// `DCC_MCP_GATEWAY_HEALTH_INTERVAL_SECS`.
    pub health_check_interval_secs: u64,

    /// Number of consecutive health-check failures before a backend is
    /// auto-deregistered (issue #854).
    ///
    /// Default: `2`. Override at runtime with
    /// `DCC_MCP_GATEWAY_HEALTH_FAILURES`.
    pub health_check_failures: u32,

    /// Admin persistence settings (SQLite, skill-path snapshot, reload hook).
    pub admin_persist: AdminPersistConfig,

    /// Bearer-token authentication for the HTTP registration plane (#1365).
    ///
    /// Defaults to [`super::security::GatewayAuth::disabled()`] — every
    /// request is accepted, matching the historical local-trust model.
    /// Populate this with one or more [`super::security::GatewayAuthToken`]
    /// values when running the daemon mode over a network the operator
    /// does not fully trust.
    pub auth: super::security::GatewayAuth,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            gateway_port: 9765,
            remote_host: None,
            remote_gateway_port: 0,
            stale_timeout_secs: 30,
            heartbeat_secs: 5,
            server_name: "dcc-mcp-gateway".to_string(),
            gateway_name: None,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            registry_dir: None,
            challenger_timeout_secs: 120,
            challenger_poll_interval_secs: 10,
            backend_timeout_ms: 10_000,
            async_dispatch_timeout_ms: 60_000,
            wait_terminal_timeout_ms: 600_000,
            route_ttl_secs: 60 * 60 * 24,
            max_routes_per_session: 1_000,
            allow_unknown_tools: false,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: Vec::new(),
            adapter_version: None,
            adapter_dcc: None,
            middleware_chain: super::middleware::MiddlewareChain::new(),
            policy: GatewayPolicy::default(),
            admin_enabled: true,
            admin_path: "/admin".to_string(),
            health_check_interval_secs: 5,
            health_check_failures: 2,
            admin_persist: AdminPersistConfig::default(),
            auth: super::security::GatewayAuth::disabled(),
        }
    }
}
