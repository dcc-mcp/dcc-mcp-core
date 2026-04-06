//! IPC transport layer — low-latency inter-process communication for same-machine DCC connections.
//!
//! Provides a protocol-agnostic transport address abstraction (`TransportAddress`) and automatic
//! transport selection strategy (`TransportScheme`).
//!
//! ## Transport types
//!
//! | Transport      | Platform         | Typical latency | Throughput  |
//! |----------------|-----------------|-----------------|-------------|
//! | TCP            | All             | ~10ms           | ~100MB/s    |
//! | Named Pipe     | Windows         | < 0.5ms         | > 1GB/s     |
//! | Unix Socket    | macOS / Linux   | < 0.1ms         | > 1GB/s     |
//!
//! ## Auto-selection strategy
//!
//! - Same machine → prefer Named Pipe (Windows) or Unix Socket (macOS/Linux)
//! - Cross-machine → TCP with connection pooling
//! - Fallback → always degrade to TCP if IPC unavailable

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// Transport address — protocol-agnostic endpoint for DCC communication.
///
/// Replaces the simple `(host, port)` tuple with a richer address type that
/// supports TCP, Named Pipes (Windows), and Unix Domain Sockets (macOS/Linux).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportAddress {
    /// TCP socket address (cross-platform, cross-machine).
    Tcp {
        /// Host address (IP or hostname).
        host: String,
        /// Port number.
        port: u16,
    },

    /// Windows Named Pipe.
    ///
    /// Path format: `\\.\pipe\<name>`
    /// Same-machine only, sub-millisecond latency.
    NamedPipe {
        /// Full pipe path (e.g. `\\.\pipe\dcc-mcp-maya-12345`).
        path: String,
    },

    /// Unix Domain Socket (macOS / Linux).
    ///
    /// Same-machine only, sub-0.1ms latency.
    UnixSocket {
        /// Socket file path (e.g. `/tmp/dcc-mcp-maya-12345.sock`).
        path: PathBuf,
    },
}

impl TransportAddress {
    /// Create a TCP transport address.
    pub fn tcp(host: impl Into<String>, port: u16) -> Self {
        Self::Tcp {
            host: host.into(),
            port,
        }
    }

    /// Create a Windows Named Pipe transport address.
    ///
    /// Automatically prepends `\\.\pipe\` if not already present.
    pub fn named_pipe(name: impl Into<String>) -> Self {
        let name = name.into();
        let path = if name.starts_with(r"\\.\pipe\") {
            name
        } else {
            format!(r"\\.\pipe\{name}")
        };
        Self::NamedPipe { path }
    }

    /// Create a Unix Domain Socket transport address.
    pub fn unix_socket(path: impl Into<PathBuf>) -> Self {
        Self::UnixSocket { path: path.into() }
    }

    /// Generate a default Named Pipe name for a DCC instance.
    ///
    /// Format: `dcc-mcp-<dcc_type>-<pid>` (e.g. `dcc-mcp-maya-12345`)
    pub fn default_pipe_name(dcc_type: &str, pid: u32) -> Self {
        Self::named_pipe(format!("dcc-mcp-{dcc_type}-{pid}"))
    }

    /// Generate a default Unix Socket path for a DCC instance.
    ///
    /// Format: `/tmp/dcc-mcp-<dcc_type>-<pid>.sock`
    pub fn default_unix_socket(dcc_type: &str, pid: u32) -> Self {
        let path = std::env::temp_dir().join(format!("dcc-mcp-{dcc_type}-{pid}.sock"));
        Self::unix_socket(path)
    }

    /// Generate the optimal local transport address for the current platform.
    ///
    /// - Windows → Named Pipe
    /// - macOS/Linux → Unix Domain Socket
    pub fn default_local(dcc_type: &str, pid: u32) -> Self {
        if cfg!(windows) {
            Self::default_pipe_name(dcc_type, pid)
        } else {
            Self::default_unix_socket(dcc_type, pid)
        }
    }

    /// Check if this address represents a local (same-machine) transport.
    pub fn is_local(&self) -> bool {
        match self {
            Self::Tcp { host, .. } => {
                host == "127.0.0.1" || host == "localhost" || host == "::1" || host == "0.0.0.0"
            }
            Self::NamedPipe { .. } | Self::UnixSocket { .. } => true,
        }
    }

    /// Check if this is a TCP address.
    pub fn is_tcp(&self) -> bool {
        matches!(self, Self::Tcp { .. })
    }

    /// Check if this is a Named Pipe address.
    pub fn is_named_pipe(&self) -> bool {
        matches!(self, Self::NamedPipe { .. })
    }

    /// Check if this is a Unix Domain Socket address.
    pub fn is_unix_socket(&self) -> bool {
        matches!(self, Self::UnixSocket { .. })
    }

    /// Get the transport scheme name.
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::Tcp { .. } => "tcp",
            Self::NamedPipe { .. } => "pipe",
            Self::UnixSocket { .. } => "unix",
        }
    }

    /// Extract host and port for TCP addresses. Returns `None` for IPC addresses.
    pub fn tcp_parts(&self) -> Option<(&str, u16)> {
        match self {
            Self::Tcp { host, port } => Some((host, *port)),
            _ => None,
        }
    }

    /// Extract the IPC path (pipe name or socket path).
    pub fn ipc_path(&self) -> Option<&Path> {
        match self {
            Self::NamedPipe { path } => Some(Path::new(path)),
            Self::UnixSocket { path } => Some(path),
            Self::Tcp { .. } => None,
        }
    }

    /// Convert to a connection string for display/logging.
    pub fn to_connection_string(&self) -> String {
        match self {
            Self::Tcp { host, port } => format!("tcp://{host}:{port}"),
            Self::NamedPipe { path } => format!("pipe://{path}"),
            Self::UnixSocket { path } => format!("unix://{}", path.display()),
        }
    }

    /// Parse a transport address from a URI-style string.
    ///
    /// Supported formats:
    /// - `tcp://host:port` — TCP (all platforms)
    /// - `pipe://name` or `pipe:///path` — Named Pipe (Windows)
    /// - `unix:///path/to/socket` — Unix Domain Socket (macOS/Linux)
    ///
    /// Returns an error string if the format is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use dcc_mcp_transport::ipc::TransportAddress;
    ///
    /// let addr = TransportAddress::parse("tcp://127.0.0.1:9000").unwrap();
    /// assert!(addr.is_tcp());
    /// ```
    pub fn parse(s: &str) -> Result<Self, String> {
        if let Some(rest) = s.strip_prefix("tcp://") {
            // Expected: host:port
            let mut parts = rest.rsplitn(2, ':');
            let port_str = parts
                .next()
                .ok_or_else(|| format!("missing port in '{s}'"))?;
            let host = parts
                .next()
                .ok_or_else(|| format!("missing host in '{s}'"))?;
            let port = port_str
                .parse::<u16>()
                .map_err(|_| format!("invalid port '{port_str}' in '{s}'"))?;
            return Ok(Self::tcp(host, port));
        }

        if let Some(rest) = s.strip_prefix("pipe://") {
            return Ok(Self::named_pipe(rest));
        }

        if let Some(rest) = s.strip_prefix("unix://") {
            return Ok(Self::unix_socket(rest));
        }

        Err(format!(
            "unknown scheme in '{s}'; expected tcp://, pipe://, or unix://"
        ))
    }
}

impl fmt::Display for TransportAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_connection_string())
    }
}

impl std::str::FromStr for TransportAddress {
    type Err = String;

    /// Parse a transport address from a URI string.
    ///
    /// Delegates to [`TransportAddress::parse`], supporting `tcp://`, `pipe://`, and `unix://` schemes.
    ///
    /// # Examples
    ///
    /// ```
    /// use dcc_mcp_transport::ipc::TransportAddress;
    ///
    /// let addr: TransportAddress = "tcp://127.0.0.1:9000".parse().unwrap();
    /// assert!(addr.is_tcp());
    ///
    /// let addr2: TransportAddress = "pipe://dcc-mcp-maya".parse().unwrap();
    /// assert!(addr2.is_named_pipe());
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Transport selection strategy — how to choose the optimal transport for a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportScheme {
    /// Automatically select the best transport based on locality.
    ///
    /// - Same machine (localhost) → Named Pipe (Windows) or Unix Socket (macOS/Linux)
    /// - Remote → TCP
    /// - Fallback → TCP if IPC fails
    #[default]
    Auto,

    /// Always use TCP (cross-platform, works everywhere).
    TcpOnly,

    /// Prefer Named Pipe on Windows, fall back to TCP on other platforms.
    PreferNamedPipe,

    /// Prefer Unix Domain Socket on macOS/Linux, fall back to TCP on Windows.
    PreferUnixSocket,

    /// Use the fastest available IPC, fall back to TCP.
    PreferIpc,
}

impl TransportScheme {
    /// Select the optimal transport address given the scheme and target locality.
    ///
    /// # Arguments
    /// * `dcc_type` — DCC application type (e.g. "maya")
    /// * `host` — Target host address
    /// * `port` — Target port
    /// * `pid` — Optional DCC process PID (needed for IPC naming)
    pub fn select_address(
        &self,
        dcc_type: &str,
        host: &str,
        port: u16,
        pid: Option<u32>,
    ) -> TransportAddress {
        let is_local =
            host == "127.0.0.1" || host == "localhost" || host == "::1" || host == "0.0.0.0";
        let pid = pid.unwrap_or(0);

        match self {
            Self::TcpOnly => TransportAddress::tcp(host, port),

            Self::Auto => {
                if is_local && pid > 0 {
                    TransportAddress::default_local(dcc_type, pid)
                } else {
                    TransportAddress::tcp(host, port)
                }
            }

            Self::PreferNamedPipe => {
                if cfg!(windows) && is_local && pid > 0 {
                    TransportAddress::default_pipe_name(dcc_type, pid)
                } else {
                    TransportAddress::tcp(host, port)
                }
            }

            Self::PreferUnixSocket => {
                if cfg!(unix) && is_local && pid > 0 {
                    TransportAddress::default_unix_socket(dcc_type, pid)
                } else {
                    TransportAddress::tcp(host, port)
                }
            }

            Self::PreferIpc => {
                if is_local && pid > 0 {
                    TransportAddress::default_local(dcc_type, pid)
                } else {
                    TransportAddress::tcp(host, port)
                }
            }
        }
    }
}

impl fmt::Display for TransportScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::TcpOnly => write!(f, "tcp_only"),
            Self::PreferNamedPipe => write!(f, "prefer_named_pipe"),
            Self::PreferUnixSocket => write!(f, "prefer_unix_socket"),
            Self::PreferIpc => write!(f, "prefer_ipc"),
        }
    }
}

/// Configuration for IPC transports.
#[derive(Debug, Clone)]
pub struct IpcConfig {
    /// Pipe name prefix for Named Pipes (default: "dcc-mcp").
    pub pipe_prefix: String,
    /// Base directory for Unix Domain Sockets (default: system temp dir).
    pub socket_dir: PathBuf,
    /// Connection timeout for IPC transports.
    pub connect_timeout: std::time::Duration,
    /// Read/write buffer size in bytes (default: 64KB).
    pub buffer_size: usize,
    /// Transport selection strategy.
    pub scheme: TransportScheme,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            pipe_prefix: "dcc-mcp".to_string(),
            socket_dir: std::env::temp_dir(),
            connect_timeout: std::time::Duration::from_secs(5),
            buffer_size: 64 * 1024,
            scheme: TransportScheme::Auto,
        }
    }
}

impl IpcConfig {
    /// Create a new IPC config with the given scheme.
    pub fn with_scheme(scheme: TransportScheme) -> Self {
        Self {
            scheme,
            ..Default::default()
        }
    }

    /// Generate a Named Pipe path for a DCC instance.
    pub fn pipe_path(&self, dcc_type: &str, pid: u32) -> String {
        format!(r"\\.\pipe\{}-{}-{}", self.pipe_prefix, dcc_type, pid)
    }

    /// Generate a Unix Socket path for a DCC instance.
    pub fn socket_path(&self, dcc_type: &str, pid: u32) -> PathBuf {
        self.socket_dir
            .join(format!("{}-{}-{}.sock", self.pipe_prefix, dcc_type, pid))
    }

    /// Generate the optimal transport address for a DCC instance.
    pub fn address_for(&self, dcc_type: &str, pid: u32) -> TransportAddress {
        if cfg!(windows) {
            TransportAddress::NamedPipe {
                path: self.pipe_path(dcc_type, pid),
            }
        } else {
            TransportAddress::UnixSocket {
                path: self.socket_path(dcc_type, pid),
            }
        }
    }
}

/// Capability flags for what transports are available on this platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformCapabilities {
    /// TCP is always available.
    pub tcp: bool,
    /// Named Pipes available (Windows only).
    pub named_pipe: bool,
    /// Unix Domain Sockets available (macOS/Linux only).
    pub unix_socket: bool,
}

impl PlatformCapabilities {
    /// Detect capabilities for the current platform.
    pub fn detect() -> Self {
        Self {
            tcp: true,
            named_pipe: cfg!(windows),
            unix_socket: cfg!(unix),
        }
    }

    /// Check if any IPC transport is available.
    pub fn has_ipc(&self) -> bool {
        self.named_pipe || self.unix_socket
    }

    /// Get the preferred IPC transport for this platform.
    pub fn preferred_ipc(&self) -> Option<&'static str> {
        if self.named_pipe {
            Some("named_pipe")
        } else if self.unix_socket {
            Some("unix_socket")
        } else {
            None
        }
    }
}

impl fmt::Display for PlatformCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut caps = vec!["tcp"];
        if self.named_pipe {
            caps.push("named_pipe");
        }
        if self.unix_socket {
            caps.push("unix_socket");
        }
        write!(f, "[{}]", caps.join(", "))
    }
}

#[cfg(test)]
mod tests;
