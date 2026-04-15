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

use tokio::sync::{RwLock, watch};
use tokio::task::AbortHandle;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceKey};

// ── Version utilities ─────────────────────────────────────────────────────────

/// `dcc_type` used for the gateway sentinel entry in the `FileRegistry`.
///
/// The sentinel entry carries the current gateway's version so that newly
/// started instances can compare themselves against the running gateway and
/// decide whether to enter challenger mode.
pub(crate) const GATEWAY_SENTINEL_DCC_TYPE: &str = "__gateway__";

/// Parse a semver string (`"0.12.29"`, `"v1.2.3-rc1"`) into a comparable triple.
///
/// Handles common variants:
/// - Leading `v` prefix stripped (`"v0.12.29"` → `(0, 12, 29)`)
/// - Pre-release suffixes ignored (`"1.0.0-rc1"` → `(1, 0, 0)`)
/// - Missing components default to `0` (`"1.2"` → `(1, 2, 0)`)
pub(crate) fn parse_semver(v: &str) -> (u64, u64, u64) {
    let parts: Vec<u64> = v
        .trim_start_matches('v')
        .split('.')
        .filter_map(|seg| seg.split('-').next()?.parse::<u64>().ok())
        .collect();
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
}

/// Returns `true` when `candidate` is strictly newer than `current`.
///
/// Uses numeric semver comparison, so `"0.12.29"` > `"0.12.6"`.
pub(crate) fn is_newer_version(candidate: &str, current: &str) -> bool {
    parse_semver(candidate) > parse_semver(current)
}

// ── Free helper: bind a port without SO_REUSEADDR (first-wins semantics) ──────

/// Attempt to bind `host:port` with `SO_REUSEADDR = false`.
///
/// Returns a bound listener on success, or `None` if the port is already taken.
/// Used by both the initial gateway competition and the challenger retry loop.
async fn try_bind_port(host: &str, port: u16) -> Option<tokio::net::TcpListener> {
    use socket2::{Domain, Socket, Type};

    let addr: std::net::SocketAddr = format!("{host}:{port}").parse().ok()?;
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, None).ok()?;
    socket.set_reuse_address(false).ok()?;
    #[cfg(unix)]
    socket.set_reuse_port(false).ok()?;
    socket.bind(&addr.into()).ok()?;
    socket.listen(128).ok()?;
    socket.set_nonblocking(true).ok()?;
    tokio::net::TcpListener::from_std(std::net::TcpListener::from(socket)).ok()
}

// ── Helper: does the registry contain a live instance newer than us? ──────────

fn has_newer_live_instance(reg: &FileRegistry, own_version: &str, stale_timeout: Duration) -> bool {
    reg.list_all().into_iter().any(|e| {
        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
            && !e.is_stale(stale_timeout)
            && e.version
                .as_deref()
                .map(|v| is_newer_version(v, own_version))
                .unwrap_or(false)
    })
}

// ── Gateway task setup (shared between winner and challenger paths) ────────────

/// Build and run the gateway HTTP server with a graceful-yield shutdown hook.
///
/// Returns the combined `AbortHandle` for all gateway background tasks.
async fn start_gateway_tasks(
    listener: tokio::net::TcpListener,
    registry: Arc<RwLock<FileRegistry>>,
    stale_timeout: Duration,
    server_name: String,
    server_version: String,
) -> Result<(AbortHandle, Arc<watch::Sender<bool>>), Box<dyn std::error::Error + Send + Sync>> {
    // Yield channel — sending `true` triggers graceful gateway shutdown.
    let (yield_tx, mut yield_rx) = watch::channel(false);
    let yield_tx = Arc::new(yield_tx);

    // Stale cleanup + challenger detection background task.
    let reg_cleanup = registry.clone();
    let own_version = server_version.clone();
    let yield_tx_cleanup = yield_tx.clone();
    let cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            let r = reg_cleanup.read().await;

            match r.cleanup_stale(stale_timeout) {
                Ok(n) if n > 0 => tracing::info!("Gateway: evicted {} stale instance(s)", n),
                Err(e) => tracing::warn!("Gateway: stale cleanup error: {e}"),
                _ => {}
            }

            if has_newer_live_instance(&r, &own_version, stale_timeout) {
                tracing::info!(
                    current = %own_version,
                    "Gateway: newer-version challenger detected — initiating voluntary yield"
                );
                let _ = yield_tx_cleanup.send(true);
                break;
            }
        }
    });

    // Gateway HTTP server — shuts down when `yield_rx` fires.
    let gw_state = GatewayState {
        registry,
        stale_timeout,
        server_name,
        server_version,
        http_client: reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?,
        yield_tx: yield_tx.clone(),
    };
    let gw_router = build_gateway_router(gw_state);
    let actual = listener.local_addr()?;
    tracing::info!(
        "Gateway listening on http://{}  (instances: /instances, mcp: /mcp)",
        actual
    );

    let gw_handle = tokio::spawn(async move {
        axum::serve(listener, gw_router)
            .with_graceful_shutdown(async move {
                loop {
                    if yield_rx.changed().await.is_err() {
                        break;
                    }
                    if *yield_rx.borrow() {
                        tracing::info!("Gateway: graceful shutdown triggered — releasing port");
                        break;
                    }
                }
            })
            .await
            .ok();
    });

    // Wrap both tasks under one combined abort handle.
    let combined = tokio::spawn(async move {
        let _ = tokio::join!(cleanup_handle, gw_handle);
    });

    Ok((combined.abort_handle(), yield_tx))
}

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
        }
    }
}

/// Returned by [`GatewayRunner::start`]. Dropping this handle aborts the
/// heartbeat and stale-cleanup background tasks.
pub struct GatewayHandle {
    /// `true` if this instance won the gateway port at startup.
    pub is_gateway: bool,
    /// The `ServiceKey` this instance was registered under.
    pub service_key: ServiceKey,
    heartbeat_abort: Option<AbortHandle>,
    /// Combined gateway-HTTP + cleanup abort handle (set on the winner path).
    gateway_abort: Option<AbortHandle>,
    /// Background challenger-loop abort handle (set when we entered challenger mode).
    challenger_abort: Option<AbortHandle>,
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        if let Some(h) = self.heartbeat_abort.take() {
            h.abort();
        }
        if let Some(h) = self.gateway_abort.take() {
            h.abort();
        }
        if let Some(h) = self.challenger_abort.take() {
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

    /// Register `entry`, start heartbeat, and run the **version-aware gateway election**.
    ///
    /// ## Election algorithm
    ///
    /// 1. **Win**: binds the gateway port → becomes gateway immediately.
    ///    - Registers a `__gateway__` sentinel with its own version in FileRegistry.
    ///    - Periodically checks whether any live instance has a *newer* version;
    ///      if so, initiates voluntary yield (graceful shutdown of its listener).
    ///
    /// 2. **Lose + same-or-older version**: registers as a plain DCC instance
    ///    (current `is_gateway = false` behaviour).
    ///
    /// 3. **Lose + newer version** (e.g. `0.12.29` vs `0.12.6` gateway):
    ///    - First tries a cooperative [`POST /gateway/yield`] to the existing
    ///      gateway (works if the gateway supports it, i.e. is also `≥ 0.12.29`).
    ///    - Regardless of the response, enters a **challenger retry loop** that
    ///      polls the port every 10 s for up to `challenger_timeout_secs`.
    ///    - When the port becomes free (old gateway yielded or crashed),
    ///      the challenger binds it and becomes the new gateway.
    pub async fn start(
        &self,
        entry: ServiceEntry,
    ) -> Result<GatewayHandle, Box<dyn std::error::Error + Send + Sync>> {
        let service_key = entry.key();

        // ── Register in FileRegistry ─────────────────────────────────────
        {
            let reg = self.registry.read().await;
            reg.register(entry)?;
        }
        tracing::info!(instance = %service_key.instance_id, "Registered in FileRegistry");

        // ── Heartbeat task ────────────────────────────────────────────────
        let heartbeat_abort = if self.config.heartbeat_secs > 0 {
            let reg = self.registry.clone();
            let key = service_key.clone();
            let secs = self.config.heartbeat_secs;
            let h = tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(secs));
                loop {
                    tick.tick().await;
                    let r = reg.read().await;
                    let _ = r.heartbeat(&key);
                }
            });
            Some(h.abort_handle())
        } else {
            None
        };

        // ── Gateway election ──────────────────────────────────────────────
        let (is_gateway, gateway_abort, challenger_abort) = if self.config.gateway_port > 0 {
            self.run_election().await?
        } else {
            (false, None, None)
        };

        Ok(GatewayHandle {
            is_gateway,
            service_key,
            heartbeat_abort,
            gateway_abort,
            challenger_abort,
        })
    }

    /// Core version-aware election logic, extracted for clarity.
    async fn run_election(
        &self,
    ) -> Result<
        (bool, Option<AbortHandle>, Option<AbortHandle>),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let stale_timeout = Duration::from_secs(self.config.stale_timeout_secs);
        let own_version = self.config.server_version.clone();

        match try_bind_port(&self.config.host, self.config.gateway_port).await {
            // ── We won the port ───────────────────────────────────────────
            Some(listener) => {
                // Write a sentinel entry so challengers can read our version.
                let mut sentinel = ServiceEntry::new(
                    GATEWAY_SENTINEL_DCC_TYPE,
                    &self.config.host,
                    self.config.gateway_port,
                );
                sentinel.version = Some(own_version.clone());
                {
                    let reg = self.registry.read().await;
                    let _ = reg.register(sentinel);
                }

                let (gateway_abort, _yield_tx) = start_gateway_tasks(
                    listener,
                    self.registry.clone(),
                    stale_timeout,
                    format!("{} (gateway)", self.config.server_name),
                    own_version.clone(),
                )
                .await?;

                tracing::info!(version = %own_version, "Won gateway election");
                Ok((true, Some(gateway_abort), None))
            }

            // ── Port is taken — version-aware challenger logic ────────────
            None => {
                // Read the sentinel to discover the current gateway's version.
                let gw_version = {
                    let reg = self.registry.read().await;
                    reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE)
                        .into_iter()
                        .next()
                        .and_then(|e| e.version)
                        .unwrap_or_default()
                };

                if !gw_version.is_empty() && is_newer_version(&own_version, &gw_version) {
                    tracing::info!(
                        own = %own_version,
                        gateway = %gw_version,
                        "We are newer than the current gateway — entering challenger mode"
                    );
                    let challenger_abort = self.spawn_challenger_loop(&own_version, &gw_version);
                    // Return as non-gateway for now; challenger loop will promote us later.
                    Ok((false, None, Some(challenger_abort)))
                } else {
                    tracing::info!(
                        port = self.config.gateway_port,
                        gateway_version = %gw_version,
                        own_version = %own_version,
                        "Gateway port taken by same-or-newer version — running as plain DCC instance"
                    );
                    Ok((false, None, None))
                }
            }
        }
    }

    /// Spawn the background challenger loop.
    ///
    /// 1. Sends a cooperative [`POST /gateway/yield`] to ask the old gateway
    ///    nicely (works if it runs `≥ 0.12.29`; ignored otherwise).
    /// 2. Polls the port every 10 s until it becomes free or the timeout fires.
    /// 3. When the port frees up, calls [`start_gateway_tasks`] to fully take over.
    fn spawn_challenger_loop(&self, own_version: &str, gw_version: &str) -> AbortHandle {
        let host = self.config.host.clone();
        let port = self.config.gateway_port;
        let own_ver = own_version.to_owned();
        let gw_ver = gw_version.to_owned();
        let registry = self.registry.clone();
        let stale_timeout = Duration::from_secs(self.config.stale_timeout_secs);
        let server_name = self.config.server_name.clone();
        let timeout_secs = self.config.challenger_timeout_secs;

        let handle = tokio::spawn(async move {
            // ── Cooperative yield request ─────────────────────────────────
            // If the old gateway also speaks our protocol it will shut down
            // gracefully; if not (e.g. v0.12.6) this is a no-op 404 — fine.
            let yield_url = format!("http://{}:{}/gateway/yield", host, port);
            let body = serde_json::json!({ "challenger_version": own_ver }).to_string();
            if let Ok(resp) = reqwest::Client::new()
                .post(&yield_url)
                .header("content-type", "application/json")
                .body(body)
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                if resp.status().is_success() {
                    tracing::info!(
                        gateway = %gw_ver,
                        "Cooperative yield accepted — waiting for port to free up"
                    );
                } else {
                    tracing::info!(
                        status = %resp.status(),
                        "Cooperative yield not supported by gateway v{gw_ver} \
                         (normal for older versions) — polling for port"
                    );
                }
            }

            // ── Retry loop ────────────────────────────────────────────────
            let max_retries = (timeout_secs / 10).max(1);
            for attempt in 1..=max_retries {
                tokio::time::sleep(Duration::from_secs(10)).await;

                if let Some(listener) = try_bind_port(&host, port).await {
                    tracing::info!(
                        attempt = attempt,
                        version = %own_ver,
                        "Challenger: won gateway port — starting gateway tasks"
                    );

                    // Update sentinel with our version.
                    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, &host, port);
                    sentinel.version = Some(own_ver.clone());
                    {
                        let reg = registry.read().await;
                        let _ = reg.register(sentinel);
                    }

                    if let Err(e) = start_gateway_tasks(
                        listener,
                        registry.clone(),
                        stale_timeout,
                        format!("{server_name} (gateway)"),
                        own_ver.clone(),
                    )
                    .await
                    {
                        tracing::error!("Challenger: failed to start gateway tasks: {e}");
                    }
                    return;
                }

                tracing::debug!("Challenger: port still taken (attempt {attempt}/{max_retries})");
            }

            tracing::warn!(
                own = %own_ver,
                gateway = %gw_ver,
                "Challenger: gave up after {max_retries} retries — staying as plain instance"
            );
        });

        handle.abort_handle()
    }
}
