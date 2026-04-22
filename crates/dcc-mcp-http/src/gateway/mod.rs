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

pub mod aggregator;
pub mod backend_client;
pub mod handlers;
pub mod namespace;
pub mod proxy;
pub mod router;
pub mod sse_subscriber;
pub mod state;
pub mod tools;

pub use router::build_gateway_router;
pub use state::{GatewayState, entry_to_json};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, broadcast, watch};
use tokio::task::AbortHandle;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceKey};

// ── Version utilities ─────────────────────────────────────────────────────────
//
// `GATEWAY_SENTINEL_DCC_TYPE` now lives in `dcc-mcp-transport::discovery::types`
// so the low-level `FileRegistry` can special-case it in `cleanup_stale`.

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
/// Returns a bound listener on success, or a detailed `io::Error` on failure.
/// Used by both the initial gateway competition and the challenger retry loop.
///
/// Unlike earlier revisions that returned `Option<TcpListener>` via `.ok()?`,
/// this surface preserves the real cause — `EADDRINUSE`, `EACCES`, a Windows
/// overlapped-I/O registration error from `TcpListener::from_std`, etc. —
/// so callers can log it and distinguish "port in use" from "socket setup
/// failed" (issue #303, suggestion D).
async fn try_bind_port(host: &str, port: u16) -> std::io::Result<tokio::net::TcpListener> {
    use socket2::{Domain, Socket, Type};

    let addr: std::net::SocketAddr =
        format!("{host}:{port}")
            .parse()
            .map_err(|e: std::net::AddrParseError| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
            })?;
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, None)?;
    socket.set_reuse_address(false)?;
    #[cfg(unix)]
    socket.set_reuse_port(false)?;
    socket.bind(&addr.into())?;
    socket.listen(128)?;
    socket.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(std::net::TcpListener::from(socket))
}

/// `Option`-returning wrapper kept for the call sites that only care about
/// win/lose semantics. Non-`AddrInUse` errors are still logged so they are
/// never silently discarded (fixes the "silent bind error" leg of #303).
async fn try_bind_port_opt(host: &str, port: u16) -> Option<tokio::net::TcpListener> {
    match try_bind_port(host, port).await {
        Ok(l) => Some(l),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => None,
        Err(e) => {
            tracing::warn!(
                host = %host,
                port = port,
                error = %e,
                kind = ?e.kind(),
                "gateway bind failed (non-AddrInUse) — treating as lost election"
            );
            None
        }
    }
}

// ── Self-probe: confirm the listener is actually accepting connections ───────
//
// Implemented as `self_probe_listener` below (called from `start_gateway_tasks`
// after the listener is spawned). Retained here as a reference for the
// two failure modes it guards against (issue #303):
// - Run A: TIMEOUT (bound but accept loop starved of scheduling time)
// - Run B: REFUSED (listener task dropped before the kernel finished setup)

// ── Helper: does the sentinel advertise a newer gateway version than us? ─────
//
// Issue #228: the old implementation scanned every DCC instance entry and
// compared its `version` field (which is the DCC host version — e.g. Maya
// `"2024"`) against our crate version (e.g. `"0.14.3"`), causing semver
// comparison to flag every running DCC as a "newer challenger" and trigger
// a self-yield within 15 s of startup.
//
// A newer gateway instance will always rewrite the `__gateway__` sentinel with
// its own crate version — so that sentinel row is the **only** reliable source
// of "is there a newer gateway challenger on the network". Any comparison must
// therefore be restricted to the sentinel row, and it must ignore our own
// sentinel write (same version, same host, same port).
fn has_newer_sentinel(reg: &FileRegistry, own_version: &str, stale_timeout: Duration) -> bool {
    reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE)
        .into_iter()
        .any(|e| {
            !e.is_stale(stale_timeout)
                && e.version
                    .as_deref()
                    .map(|v| is_newer_version(v, own_version))
                    .unwrap_or(false)
        })
}

// ── Gateway task setup (shared between winner and challenger paths) ────────────

/// Outcome of [`start_gateway_tasks`] for the ambient (shared-runtime) path.
pub(crate) struct GatewayTasks {
    /// AbortHandle for the combined supervisor task (cleanup + watcher +
    /// tools watcher + serve).
    pub(crate) abort: AbortHandle,
    /// JoinHandle for the combined supervisor task. Retained by
    /// `GatewayHandle` so the task is not silently detached — this is the
    /// fix for the "Run A: TIMEOUT" leg of issue #303.
    pub(crate) supervisor: tokio::task::JoinHandle<()>,
    /// Yield signal used by the caller to trigger graceful shutdown.
    #[allow(dead_code)]
    pub(crate) yield_tx: Arc<watch::Sender<bool>>,
}

/// Build and run the gateway HTTP server with graceful-yield and live-push support.
///
/// Returns a [`GatewayTasks`] handle holding both the `AbortHandle` and the
/// supervisor task's `JoinHandle`, so the caller (typically a
/// [`GatewayHandle`]) can keep the task alive for its own lifetime.
///
/// `sentinel_key` is the registry key of the `__gateway__` sentinel row that
/// the caller registered; the cleanup loop heartbeats it (issue #229).
#[allow(clippy::too_many_arguments)]
async fn start_gateway_tasks(
    listener: tokio::net::TcpListener,
    registry: Arc<RwLock<FileRegistry>>,
    stale_timeout: Duration,
    backend_timeout: Duration,
    async_dispatch_timeout: Duration,
    wait_terminal_timeout: Duration,
    route_ttl: Duration,
    max_routes_per_session: usize,
    server_name: String,
    server_version: String,
    sentinel_key: ServiceKey,
) -> Result<GatewayTasks, Box<dyn std::error::Error + Send + Sync>> {
    // ── Yield channel ─────────────────────────────────────────────────────
    let (yield_tx, mut yield_rx) = watch::channel(false);
    let yield_tx = Arc::new(yield_tx);

    // ── SSE broadcast channel ──────────────────────────────────────────────
    // All MCP notifications (resources/list_changed, tools/list_changed) are
    // sent here. Capacity 128 is generous; watchers fire at most every 2-3 s.
    let (events_tx, _) = broadcast::channel::<String>(128);
    let events_tx = Arc::new(events_tx);

    // ── Shared HTTP client for backend fan-out ─────────────────────────────
    // Reused by both the tools-list watcher task and the facade /mcp handler
    // via GatewayState so connection pooling is shared across all consumers.
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    // ── Stale cleanup + sentinel heartbeat + dead-PID pruning (every 15 s) ─
    //
    // Issue #229: the sentinel row is heartbeated here — without this, it
    // would be considered stale 30 s after startup and challengers could not
    // distinguish a live gateway from a crashed one.
    //
    // Issue #227: dead-PID pruning reaps ghost rows left behind when a DCC
    // plugin crashes after registering but before its own heartbeat starts.
    let reg_cleanup = registry.clone();
    let own_version = server_version.clone();
    let yield_tx_cleanup = yield_tx.clone();
    let sentinel_key_cleanup = sentinel_key.clone();
    let cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            let r = reg_cleanup.read().await;

            // Keep the sentinel fresh first — it's what `has_newer_sentinel`
            // and every consumer of `list_instances("__gateway__")` rely on.
            let _ = r.heartbeat(&sentinel_key_cleanup);

            match r.cleanup_stale(stale_timeout) {
                Ok(n) if n > 0 => tracing::info!("Gateway: evicted {} stale instance(s)", n),
                Err(e) => tracing::warn!("Gateway: stale cleanup error: {e}"),
                _ => {}
            }

            match r.prune_dead_pids() {
                Ok(n) if n > 0 => tracing::info!("Gateway: reaped {} ghost entry/entries", n),
                Err(e) => tracing::warn!("Gateway: ghost-entry reap error: {e}"),
                _ => {}
            }

            if has_newer_sentinel(&r, &own_version, stale_timeout) {
                tracing::info!(
                    current = %own_version,
                    "Gateway: newer-version sentinel detected — initiating voluntary yield"
                );
                let _ = yield_tx_cleanup.send(true);
                break;
            }
        }
    });

    // ── Instance-change watcher (every 2 s) ───────────────────────────────
    // Detects when DCC instances join or leave and broadcasts
    // `notifications/resources/list_changed` to all connected SSE clients.
    let reg_watch = registry.clone();
    let events_tx_watch = events_tx.clone();
    let watcher_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        // Fingerprint = sorted "dcc_type:instance_id" strings of live instances.
        let mut last_fingerprint = String::new();

        loop {
            interval.tick().await;

            let fingerprint = {
                let r = reg_watch.read().await;
                let mut keys: Vec<String> = r
                    .list_all()
                    .into_iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE && !e.is_stale(stale_timeout)
                    })
                    .map(|e| format!("{}:{}", e.dcc_type, e.instance_id))
                    .collect();
                keys.sort_unstable();
                keys.join("|")
            };

            if fingerprint != last_fingerprint {
                tracing::debug!(
                    "Gateway: instance set changed — broadcasting resources/list_changed"
                );
                // Only send if there are active SSE subscribers.
                if events_tx_watch.receiver_count() > 0 {
                    let notif = serde_json::to_string(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/resources/list_changed",
                        "params": {}
                    }))
                    .unwrap_or_default();
                    let _ = events_tx_watch.send(notif);
                }
                last_fingerprint = fingerprint;
            }
        }
    });

    // ── Aggregated tools/list_changed watcher (every 3 s) ─────────────────
    // Polls every live backend's `tools/list`, computes a set-fingerprint of
    // "{instance_id}:{tool_name}" tuples, and broadcasts one
    // `notifications/tools/list_changed` to gateway SSE subscribers when the
    // aggregated set changes (skill loaded / unloaded on any DCC).
    //
    // Polling (vs. real SSE subscription to each backend) keeps the gateway
    // decoupled from backend session lifecycles and works uniformly even when
    // instances come and go. 3-second granularity is well within the latency
    // budget for interactive skill loading.
    let reg_tools = registry.clone();
    let events_tx_tools = events_tx.clone();
    let http_client_tools = http_client.clone();
    let tools_watcher_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_fingerprint = String::new();

        loop {
            interval.tick().await;
            // Skip polling when no clients are listening — keeps idle gateways
            // from hammering backends.
            if events_tx_tools.receiver_count() == 0 {
                continue;
            }

            let fingerprint = aggregator::compute_tools_fingerprint(
                &reg_tools,
                stale_timeout,
                &http_client_tools,
                backend_timeout,
            )
            .await;

            if fingerprint != last_fingerprint {
                // First tick always "changes" from empty-string → don't push
                // on initial startup unless there are actually tools.
                if !last_fingerprint.is_empty() || !fingerprint.is_empty() {
                    tracing::debug!(
                        "Gateway: aggregated tool set changed — broadcasting tools/list_changed"
                    );
                    let notif = serde_json::to_string(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/tools/list_changed",
                        "params": {}
                    }))
                    .unwrap_or_default();
                    let _ = events_tx_tools.send(notif);
                }
                last_fingerprint = fingerprint;
            }
        }
    });

    // ── Backend SSE subscriber manager (#320) ─────────────────────────────
    // Multiplexes per-backend SSE notifications back to originating client
    // sessions. Each `ensure_subscribed` spawns a reconnecting task.
    let subscriber = sse_subscriber::SubscriberManager::with_limits(
        http_client.clone(),
        route_ttl,
        max_routes_per_session,
    );
    // #322: GC loop — evicts stale (non-terminal) routes that outlive
    // their TTL. Terminal jobs are auto-evicted in `deliver`.
    let route_gc_handle = subscriber.spawn_route_gc();

    // Periodically ensure every live backend has an active subscription.
    // The subscriber's internal DashMap makes repeat calls cheap, so we
    // just poll the instance registry at the same cadence as the
    // instance-change watcher.
    let reg_sub = registry.clone();
    let sub_for_task = subscriber.clone();
    let backend_sub_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let urls: Vec<String> = {
                let r = reg_sub.read().await;
                r.list_all()
                    .into_iter()
                    .filter(|e| {
                        e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE && !e.is_stale(stale_timeout)
                    })
                    .map(|e| format!("http://{}:{}/mcp", e.host, e.port))
                    .collect()
            };
            for url in urls {
                sub_for_task.ensure_subscribed(&url);
            }
        }
    });

    // ── Gateway HTTP server ────────────────────────────────────────────────
    let gw_state = GatewayState {
        registry,
        stale_timeout,
        backend_timeout,
        async_dispatch_timeout,
        wait_terminal_timeout,
        server_name,
        server_version,
        http_client,
        yield_tx: yield_tx.clone(),
        events_tx,
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(HashMap::new())),
        pending_calls: Arc::new(RwLock::new(HashMap::new())),
        subscriber,
    };
    let gw_router = build_gateway_router(gw_state);
    let actual = listener.local_addr()?;
    tracing::info!(
        "Gateway listening on http://{}  (GET /mcp = SSE stream, POST /mcp = MCP endpoint)",
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

    // Combine all tasks under one abort handle.
    let combined = tokio::spawn(async move {
        let _ = tokio::join!(
            cleanup_handle,
            watcher_handle,
            tools_watcher_handle,
            backend_sub_handle,
            route_gc_handle,
            gw_handle
        );
    });

    // ── Post-spawn self-probe (issue #303) ────────────────────────────────
    //
    // `bind()` succeeding does not guarantee the accept-loop is actually
    // running — under PyO3-embedded hosts (e.g. mayapy on Windows) a freshly
    // spawned Tokio task can be starved long enough that the caller is told
    // `is_gateway = true` while clients see `CONNECTION REFUSED` or
    // `CONNECTION TIMED OUT` on the gateway port.
    //
    // Connecting to our own address forces the runtime to drive the accept
    // loop at least once; if that fails within the budget we trigger a yield
    // so the listener is dropped, then propagate an error so the caller can
    // fall back to plain-instance mode.
    if let Err(e) = self_probe_listener(actual).await {
        tracing::warn!(
            addr = %actual,
            error = %e,
            "Gateway self-probe failed — aborting gateway role and releasing port"
        );
        // Trigger graceful shutdown of the listener task.
        let _ = yield_tx.send(true);
        // Give the shutdown a brief moment to run so the port is released
        // before the caller decides what to do next. We do NOT await the
        // task's JoinHandle here because the runtime may be starved — we
        // rely on `combined.abort_handle()` / `yield_tx` for cleanup.
        tokio::time::sleep(Duration::from_millis(50)).await;
        return Err(format!("gateway listener self-probe failed at {actual}: {e}").into());
    }

    Ok(GatewayTasks {
        abort: combined.abort_handle(),
        supervisor: combined,
        yield_tx,
    })
}

/// Verify that the gateway accept-loop is actually running by connecting to it.
///
/// Retries a small number of times with short back-off to give the Tokio
/// runtime a chance to schedule the `axum::serve` task — necessary under
/// PyO3-embedded hosts where workers are slow to pick up newly spawned tasks
/// (issue #303).
async fn self_probe_listener(addr: std::net::SocketAddr) -> Result<(), std::io::Error> {
    const MAX_ATTEMPTS: u32 = 10;
    const ATTEMPT_TIMEOUT: Duration = Duration::from_millis(200);
    const BACKOFF: Duration = Duration::from_millis(100);

    let mut last_err: Option<std::io::Error> = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match tokio::time::timeout(ATTEMPT_TIMEOUT, tokio::net::TcpStream::connect(addr)).await {
            Ok(Ok(_stream)) => {
                tracing::debug!(addr = %addr, attempt, "Gateway self-probe succeeded");
                return Ok(());
            }
            Ok(Err(e)) => {
                tracing::debug!(addr = %addr, attempt, error = %e, "Gateway self-probe: connect error");
                last_err = Some(e);
            }
            Err(_) => {
                tracing::debug!(addr = %addr, attempt, "Gateway self-probe: connect timed out");
                last_err = Some(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "self-probe connect timed out",
                ));
            }
        }
        tokio::time::sleep(BACKOFF).await;
    }

    Err(last_err.unwrap_or_else(|| std::io::Error::other("self-probe failed with no error")))
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
    /// [`JobRoute`]: sse_subscriber::JobRoute
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

/// Returned by [`GatewayRunner::start`]. Dropping this handle aborts the
/// heartbeat and stale-cleanup background tasks.
///
/// # Task retention (issue #303 fix)
///
/// In earlier versions only the `AbortHandle` for the gateway's combined
/// supervisor task was stored here, and the supervisor's `JoinHandle` was
/// dropped at the end of `start_gateway_tasks`. Dropping a `JoinHandle`
/// *detaches* the task — in theory that is fine, but under PyO3-embedded
/// hosts on Windows the detached gateway accept loop can be starved of
/// scheduling time by the parent runtime (cf. issue #303, Run A symptom:
/// `bind()` succeeded, clients see `TIMEOUT`). Keeping the `JoinHandle`
/// alive here pins the task to its original runtime via
/// [`Runtime::enter`]-style ownership so it cannot be silently reclaimed,
/// giving downstream callers a handle they can actually `await`.
///
/// For the `ServerSpawnMode::Dedicated` path the listener runs on an OS
/// thread with its own `current_thread` runtime; [`Self::gateway_thread`]
/// holds its join handle so the Drop impl can block briefly for cleanup.
pub struct GatewayHandle {
    /// `true` if this instance won the gateway port at startup.
    pub is_gateway: bool,
    /// The `ServiceKey` this instance was registered under.
    pub service_key: ServiceKey,
    heartbeat_abort: Option<AbortHandle>,
    /// Combined gateway-HTTP + cleanup abort handle (set on the winner path).
    gateway_abort: Option<AbortHandle>,
    /// JoinHandle of the combined supervisor task, kept alive so the task
    /// is not detached (issue #303).
    #[allow(dead_code)]
    gateway_supervisor: Option<tokio::task::JoinHandle<()>>,
    /// OS thread running the dedicated-mode gateway accept loop.
    /// Only populated when `ServerSpawnMode::Dedicated` is used.
    gateway_thread: Option<std::thread::JoinHandle<()>>,
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
        // Drop supervisor JoinHandle after aborting — this detaches the
        // underlying task cleanly. The AbortHandle above has already
        // cancelled its work; joining is optional.
        drop(self.gateway_supervisor.take());

        // Dedicated-mode OS thread: we *do not* join here to avoid
        // blocking Drop indefinitely if shutdown is in flight. The thread
        // observes the same yield signal and exits on its own.
        if let Some(h) = self.gateway_thread.take() {
            // Best-effort: detach; the thread is daemon-like and cleans
            // itself up once its yield signal fires.
            drop(h);
        }
    }
}

/// Result of [`GatewayRunner::run_election`].
///
/// Packages the election outcome together with the supervisor join
/// handle and optional OS-thread handle that must be kept alive for
/// the lifetime of the gateway role (issue #303).
#[allow(dead_code)]
struct ElectionOutcome {
    is_gateway: bool,
    gateway_abort: Option<AbortHandle>,
    challenger_abort: Option<AbortHandle>,
    gateway_supervisor: Option<tokio::task::JoinHandle<()>>,
    gateway_thread: Option<std::thread::JoinHandle<()>>,
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
        let (is_gateway, gateway_abort, challenger_abort, gateway_supervisor, gateway_thread) =
            if self.config.gateway_port > 0 {
                let outcome = self.run_election().await?;
                (
                    outcome.is_gateway,
                    outcome.gateway_abort,
                    outcome.challenger_abort,
                    outcome.gateway_supervisor,
                    outcome.gateway_thread,
                )
            } else {
                (false, None, None, None, None)
            };

        Ok(GatewayHandle {
            is_gateway,
            service_key,
            heartbeat_abort,
            gateway_abort,
            gateway_supervisor,
            gateway_thread,
            challenger_abort,
        })
    }

    /// Core version-aware election logic, extracted for clarity.
    async fn run_election(
        &self,
    ) -> Result<ElectionOutcome, Box<dyn std::error::Error + Send + Sync>> {
        let stale_timeout = Duration::from_secs(self.config.stale_timeout_secs);
        let backend_timeout = Duration::from_millis(self.config.backend_timeout_ms);
        let async_dispatch_timeout = Duration::from_millis(self.config.async_dispatch_timeout_ms);
        let wait_terminal_timeout = Duration::from_millis(self.config.wait_terminal_timeout_ms);
        let route_ttl = Duration::from_secs(self.config.route_ttl_secs);
        let max_routes_per_session = self.config.max_routes_per_session as usize;
        let own_version = self.config.server_version.clone();

        match try_bind_port_opt(&self.config.host, self.config.gateway_port).await {
            // ── We won the port ───────────────────────────────────────────
            Some(listener) => {
                // Write a sentinel entry so challengers can read our version.
                // `ServiceEntry::new` auto-populates `pid` with our process id,
                // so a crash of *this* process makes the sentinel prunable by
                // `prune_dead_pids` on other peers (issue #227).
                let mut sentinel = ServiceEntry::new(
                    GATEWAY_SENTINEL_DCC_TYPE,
                    &self.config.host,
                    self.config.gateway_port,
                );
                sentinel.version = Some(own_version.clone());
                let sentinel_key = sentinel.key();
                {
                    let reg = self.registry.read().await;
                    let _ = reg.register(sentinel);
                }

                match start_gateway_tasks(
                    listener,
                    self.registry.clone(),
                    stale_timeout,
                    backend_timeout,
                    async_dispatch_timeout,
                    wait_terminal_timeout,
                    route_ttl,
                    max_routes_per_session,
                    format!("{} (gateway)", self.config.server_name),
                    own_version.clone(),
                    sentinel_key,
                )
                .await
                {
                    Ok(tasks) => {
                        tracing::info!(version = %own_version, "Won gateway election");
                        Ok(ElectionOutcome {
                            is_gateway: true,
                            gateway_abort: Some(tasks.abort),
                            challenger_abort: None,
                            gateway_supervisor: Some(tasks.supervisor),
                            gateway_thread: None,
                        })
                    }
                    // Issue #303: bind() succeeded but the accept-loop never
                    // came up (or the self-probe timed out). Fall back to
                    // plain-instance mode instead of failing the whole
                    // server start — the instance listener is unaffected.
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            version = %own_version,
                            "Gateway tasks failed to become healthy — falling back to plain-instance mode"
                        );
                        Ok(ElectionOutcome {
                            is_gateway: false,
                            gateway_abort: None,
                            challenger_abort: None,
                            gateway_supervisor: None,
                            gateway_thread: None,
                        })
                    }
                }
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
                    Ok(ElectionOutcome {
                        is_gateway: false,
                        gateway_abort: None,
                        challenger_abort: Some(challenger_abort),
                        gateway_supervisor: None,
                        gateway_thread: None,
                    })
                } else {
                    tracing::info!(
                        port = self.config.gateway_port,
                        gateway_version = %gw_version,
                        own_version = %own_version,
                        "Gateway port taken by same-or-newer version — running as plain DCC instance"
                    );
                    Ok(ElectionOutcome {
                        is_gateway: false,
                        gateway_abort: None,
                        challenger_abort: None,
                        gateway_supervisor: None,
                        gateway_thread: None,
                    })
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
        let backend_timeout = Duration::from_millis(self.config.backend_timeout_ms);
        let async_dispatch_timeout = Duration::from_millis(self.config.async_dispatch_timeout_ms);
        let wait_terminal_timeout = Duration::from_millis(self.config.wait_terminal_timeout_ms);
        let route_ttl = Duration::from_secs(self.config.route_ttl_secs);
        let max_routes_per_session = self.config.max_routes_per_session as usize;
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

                if let Some(listener) = try_bind_port_opt(&host, port).await {
                    tracing::info!(
                        attempt = attempt,
                        version = %own_ver,
                        "Challenger: won gateway port — starting gateway tasks"
                    );

                    // Update sentinel with our version.
                    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, &host, port);
                    sentinel.version = Some(own_ver.clone());
                    let sentinel_key = sentinel.key();
                    {
                        let reg = registry.read().await;
                        let _ = reg.register(sentinel);
                    }

                    if let Err(e) = start_gateway_tasks(
                        listener,
                        registry.clone(),
                        stale_timeout,
                        backend_timeout,
                        async_dispatch_timeout,
                        wait_terminal_timeout,
                        route_ttl,
                        max_routes_per_session,
                        format!("{server_name} (gateway)"),
                        own_ver.clone(),
                        sentinel_key,
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

#[cfg(test)]
mod tests;
