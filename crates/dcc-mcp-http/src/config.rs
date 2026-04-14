//! Server configuration.

use std::net::IpAddr;
use std::path::PathBuf;

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
        }
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
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self::new(8765)
    }
}
