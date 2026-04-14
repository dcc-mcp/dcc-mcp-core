//! Gateway module — first-wins port competition, instance registry, and HTTP routing.
//!
//! When `McpHttpConfig::gateway_port > 0`, `McpHttpServer::start()` will attempt to
//! become the gateway by binding the well-known gateway port. The first process to
//! bind wins; subsequent processes register themselves as plain DCC instances.
//!
//! # Quick start (Rust)
//!
//! ```rust,no_run
//! use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
//! use dcc_mcp_actions::ActionRegistry;
//! use std::sync::Arc;
//!
//! # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let registry = Arc::new(ActionRegistry::new());
//! let config = McpHttpConfig::new(0)
//!     .with_name("maya")
//!     .with_dcc_type("maya")
//!     .with_gateway(9765);
//!
//! let handle = McpHttpServer::new(registry, config).start().await?;
//! println!("is_gateway = {}", handle.is_gateway);
//! # Ok(())
//! # }
//! ```

pub mod handlers;
pub mod proxy;
pub mod router;
pub mod state;
pub mod tools;

pub use router::build_gateway_router;
pub use state::{GatewayState, entry_to_json};

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::task::AbortHandle;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceKey};

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
        }
    }
}

/// Returned by [`GatewayRunner::start`]. Dropping this handle aborts the
/// heartbeat and stale-cleanup background tasks.
pub struct GatewayHandle {
    /// `true` if this process won the gateway port competition.
    pub is_gateway: bool,
    /// The `ServiceKey` this instance was registered under.
    pub service_key: ServiceKey,
    heartbeat_abort: Option<AbortHandle>,
    cleanup_abort: Option<AbortHandle>,
    gateway_abort: Option<AbortHandle>,
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        if let Some(h) = self.heartbeat_abort.take() {
            h.abort();
        }
        if let Some(h) = self.cleanup_abort.take() {
            h.abort();
        }
        if let Some(h) = self.gateway_abort.take() {
            h.abort();
        }
    }
}

/// Orchestrates FileRegistry registration, heartbeat, stale cleanup, and the
/// optional gateway HTTP server.
pub struct GatewayRunner {
    /// Gateway configuration.
    pub config: GatewayConfig,
    /// Shared file-based service registry.
    pub registry: Arc<RwLock<FileRegistry>>,
}

impl GatewayRunner {
    /// Create a new runner, initializing (or loading) the `FileRegistry` from
    /// `config.registry_dir` (or a system temp dir if `None`).
    pub fn new(config: GatewayConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let dir = config
            .registry_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("dcc-mcp-registry"));
        let registry = FileRegistry::new(&dir)?;
        Ok(Self {
            config,
            registry: Arc::new(RwLock::new(registry)),
        })
    }

    /// Register `entry` in the `FileRegistry`, start background tasks, and
    /// attempt to become the gateway.
    ///
    /// Returns a [`GatewayHandle`] whose `is_gateway` field indicates whether
    /// this process won the port competition.
    pub async fn start(
        &self,
        entry: ServiceEntry,
    ) -> Result<GatewayHandle, Box<dyn std::error::Error + Send + Sync>> {
        let service_key = entry.key();

        // Register in FileRegistry
        {
            let reg = self.registry.read().await;
            reg.register(entry)?;
        }
        tracing::info!(
            instance = %service_key.instance_id,
            "Registered in FileRegistry"
        );

        // Heartbeat background task
        let heartbeat_abort = if self.config.heartbeat_secs > 0 {
            let reg = self.registry.clone();
            let key = service_key.clone();
            let interval_secs = self.config.heartbeat_secs;
            let handle = tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
                loop {
                    tick.tick().await;
                    let r = reg.read().await;
                    let _ = r.heartbeat(&key);
                }
            });
            Some(handle.abort_handle())
        } else {
            None
        };

        // Attempt to become gateway
        let (is_gateway, gateway_abort) = if self.config.gateway_port > 0 {
            match self.try_bind_gateway_port().await {
                Some(listener) => {
                    let stale_timeout = Duration::from_secs(self.config.stale_timeout_secs);

                    // Stale cleanup task
                    let reg_cleanup = self.registry.clone();
                    let cleanup_handle = tokio::spawn(async move {
                        let mut interval = tokio::time::interval(Duration::from_secs(15));
                        loop {
                            interval.tick().await;
                            let r = reg_cleanup.read().await;
                            match r.cleanup_stale(stale_timeout) {
                                Ok(n) if n > 0 => {
                                    tracing::info!("Gateway: evicted {} stale instance(s)", n)
                                }
                                Err(e) => {
                                    tracing::warn!("Gateway cleanup error: {e}")
                                }
                                _ => {}
                            }
                        }
                    });

                    let gw_state = GatewayState {
                        registry: self.registry.clone(),
                        stale_timeout,
                        server_name: format!("{} (gateway)", self.config.server_name),
                        server_version: self.config.server_version.clone(),
                        http_client: reqwest::Client::builder()
                            .timeout(Duration::from_secs(30))
                            .build()?,
                    };

                    let gw_router = build_gateway_router(gw_state);
                    let actual = listener.local_addr()?;
                    tracing::info!(
                        "Gateway listening on http://{}  (instances: /instances, mcp: /mcp)",
                        actual
                    );

                    let gw_handle = tokio::spawn(async move {
                        axum::serve(listener, gw_router)
                            .with_graceful_shutdown(async { std::future::pending::<()>().await })
                            .await
                            .ok();
                    });

                    // Combine both gateway tasks into one abort handle via a wrapper task
                    let combined = tokio::spawn(async move {
                        let _ = tokio::join!(
                            tokio::task::spawn(async move {
                                let _ = cleanup_handle.await;
                            }),
                            tokio::task::spawn(async move {
                                let _ = gw_handle.await;
                            }),
                        );
                    });

                    (true, Some(combined.abort_handle()))
                }
                None => {
                    tracing::info!(
                        "Gateway port {} already taken — running as plain DCC instance",
                        self.config.gateway_port
                    );
                    (false, None)
                }
            }
        } else {
            (false, None)
        };

        Ok(GatewayHandle {
            is_gateway,
            service_key,
            heartbeat_abort,
            cleanup_abort: None, // merged into gateway_abort
            gateway_abort,
        })
    }

    /// Attempt to bind the gateway port with SO_REUSEADDR=false for first-wins semantics.
    async fn try_bind_gateway_port(&self) -> Option<tokio::net::TcpListener> {
        use socket2::{Domain, Socket, Type};

        let addr: std::net::SocketAddr =
            format!("{}:{}", self.config.host, self.config.gateway_port)
                .parse()
                .ok()?;

        let socket = Socket::new(Domain::for_address(addr), Type::STREAM, None).ok()?;
        // Disable address reuse so only one process can bind — first-wins
        socket.set_reuse_address(false).ok()?;
        #[cfg(unix)]
        socket.set_reuse_port(false).ok()?;
        socket.bind(&addr.into()).ok()?;
        socket.listen(128).ok()?;
        socket.set_nonblocking(true).ok()?;
        tokio::net::TcpListener::from_std(std::net::TcpListener::from(socket)).ok()
    }
}
