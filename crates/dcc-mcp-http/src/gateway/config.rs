use super::*;

/// Configuration for the optional gateway.
pub struct GatewayConfig {
    /// Host to bind the gateway port on (default: `"127.0.0.1"`).
    pub host: String,
    /// Well-known port to compete for. `0` disables the gateway.
    pub gateway_port: u16,
    /// Seconds without heartbeat before an instance is considered stale.
    pub stale_timeout_secs: u64,
    /// Heartbeat interval in seconds. `0` disables the heartbeat task.
    pub heartbeat_secs: u64,
    /// Server name advertised in gateway `initialize` responses.
    pub server_name: String,
    /// Server version advertised in gateway `initialize` responses.
    pub server_version: String,
    /// Shared `FileRegistry` directory. `None` falls back to a temp dir.
    pub registry_dir: Option<PathBuf>,
    /// How many seconds a newer-version challenger waits for the old gateway
    /// to yield before giving up and running as a plain instance.
    ///
    /// Default: `120` seconds (12 × 10-second retry intervals).
    pub challenger_timeout_secs: u64,
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
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            gateway_port: 9765,
            stale_timeout_secs: 30,
            heartbeat_secs: 5,
            server_name: "dcc-mcp-gateway".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            registry_dir: None,
            challenger_timeout_secs: 120,
            backend_timeout_ms: 10_000,
            async_dispatch_timeout_ms: 60_000,
            wait_terminal_timeout_ms: 600_000,
            route_ttl_secs: 60 * 60 * 24,
            max_routes_per_session: 1_000,
        }
    }
}
