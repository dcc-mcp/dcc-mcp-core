//! Standalone `dcc-mcp-server` binary for bridge-mode DCCs.
//!
//! Starts the MCP Streamable HTTP server and (optionally) a WebSocket bridge
//! server so that DCC plugins written in non-Python languages (JavaScript/UXP,
//! C++, C#, GDScript, …) can connect without a local Python installation.
//!
//! ## Simplified deployment
//!
//! ```text
//! DCC Plugin (any language)
//!     ↕  WebSocket :9001  (JSON-RPC 2.0 bridge protocol)
//! dcc-mcp-server  ← this binary, zero deps
//!     ↕  HTTP :8765
//! MCP Client (Claude/Cursor)
//! ```
//!
//! ## Usage
//!
//! ```bash
//! # Auto-discover skills and start both servers
//! dcc-mcp-server
//!
//! # Explicit configuration
//! dcc-mcp-server --mcp-port 8765 --ws-port 9001 --dcc photoshop \
//!   --skill-paths /path/to/skills --server-name "photoshop-mcp"
//!
//! # No WebSocket bridge (MCP HTTP only)
//! dcc-mcp-server --no-bridge
//! ```
//!
//! ## Environment variables
//!
//! | Variable                      | Description                              |
//! |-------------------------------|------------------------------------------|
//! | `DCC_MCP_SKILL_PATHS`         | Colon/semicolon-separated skill dirs     |
//! | `DCC_MCP_MCP_PORT`            | MCP HTTP server port (default 8765)      |
//! | `DCC_MCP_WS_PORT`             | WebSocket bridge port (default 9001)     |
//! | `DCC_MCP_DCC`                 | DCC name hint (e.g. "photoshop")         |
//! | `DCC_MCP_SERVER_NAME`         | Server name advertised to MCP client     |
//! | `DCC_MCP_PID_FILE`            | Override PID file path                   |
//! | `DCC_MCP_RECONNECT_TIMEOUT`   | Seconds to wait for DCC reconnect        |
//! | `DCC_MCP_HEARTBEAT_SECS`      | WebSocket heartbeat interval (seconds)   |
//! | `RUST_LOG`                    | Log level filter (e.g. "debug")          |

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use clap::Parser;
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_utils::filesystem;
use tokio::sync::Mutex;

// ── CLI arguments ─────────────────────────────────────────────────────────────

/// Standalone MCP server for bridge-mode DCCs.
#[derive(Debug, Parser)]
#[command(name = "dcc-mcp-server", about, version)]
struct Args {
    /// MCP Streamable HTTP server port.
    #[arg(long, env = "DCC_MCP_MCP_PORT", default_value = "8765")]
    mcp_port: u16,

    /// WebSocket bridge server port (for DCC plugin connections).
    #[arg(long, env = "DCC_MCP_WS_PORT", default_value = "9001")]
    ws_port: u16,

    /// DCC name hint (e.g. "photoshop", "zbrush", "unreal").
    /// Used to resolve DCC-specific skill environment variables.
    #[arg(long, env = "DCC_MCP_DCC", default_value = "")]
    dcc: String,

    /// Additional skill search paths (repeatable).
    #[arg(long, value_name = "PATH", num_args = 1..)]
    skill_paths: Vec<PathBuf>,

    /// Server name advertised to MCP clients.
    #[arg(long, env = "DCC_MCP_SERVER_NAME", default_value = "dcc-mcp-server")]
    server_name: String,

    /// Disable the WebSocket bridge server (MCP HTTP only).
    #[arg(long, default_value = "false")]
    no_bridge: bool,

    /// MCP server host to bind to.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Path for the PID file. Defaults to $TMPDIR/dcc-mcp-server-<port>.pid.
    /// Pass an empty string "" to disable PID file management.
    #[arg(long, env = "DCC_MCP_PID_FILE")]
    pid_file: Option<String>,

    /// Force start even if a PID file already exists (overwrite stale lock).
    #[arg(long, default_value = "false")]
    force: bool,

    /// Seconds to wait for a DCC plugin to (re-)connect after a disconnect
    /// before automatically exiting. 0 = wait indefinitely.
    /// Only effective when the WebSocket bridge is enabled.
    #[arg(long, env = "DCC_MCP_RECONNECT_TIMEOUT", default_value = "0")]
    reconnect_timeout_secs: u64,

    /// WebSocket heartbeat interval in seconds (0 = disabled).
    /// A Ping frame is sent to the DCC plugin on this interval; if the send
    /// fails the connection is considered dead and the handler exits.
    #[arg(long, env = "DCC_MCP_HEARTBEAT_SECS", default_value = "30")]
    heartbeat_secs: u64,
}

// ── Bridge connection state ────────────────────────────────────────────────────

/// Shared state tracking live DCC plugin WebSocket connections.
///
/// Passed to both `run_ws_bridge` and the reconnect-timeout watchdog so
/// that either side can observe connection health without locks on the hot
/// path.
#[derive(Debug)]
struct BridgeState {
    /// Number of currently connected DCC plugin WebSocket connections.
    connected_count: AtomicU32,
    /// Set to `true` once at least one DCC plugin has ever connected.
    ever_connected: AtomicBool,
    /// Wall-clock time of the most recent disconnect event.
    last_disconnect: Mutex<Option<Instant>>,
}

impl BridgeState {
    fn new() -> Self {
        Self {
            connected_count: AtomicU32::new(0),
            ever_connected: AtomicBool::new(false),
            last_disconnect: Mutex::new(None),
        }
    }

    fn on_connect(&self) {
        self.connected_count.fetch_add(1, Ordering::Relaxed);
        self.ever_connected.store(true, Ordering::Relaxed);
    }

    async fn on_disconnect(&self) {
        self.connected_count.fetch_sub(1, Ordering::Relaxed);
        *self.last_disconnect.lock().await = Some(Instant::now());
    }

    fn is_connected(&self) -> bool {
        self.connected_count.load(Ordering::Relaxed) > 0
    }
}

// ── PID file helpers ───────────────────────────────────────────────────────────

/// Resolve the PID file path from CLI args.
/// Returns `None` when PID file management is explicitly disabled.
fn resolve_pid_path(args: &Args) -> Option<PathBuf> {
    match &args.pid_file {
        Some(p) if p.is_empty() => None,
        Some(p) => Some(PathBuf::from(p)),
        None => Some(std::env::temp_dir().join(format!("dcc-mcp-server-{}.pid", args.mcp_port))),
    }
}

/// Check for a live duplicate process, then write the current PID to `pid_path`.
///
/// Returns `Err` when another live instance is detected and `--force` is not set.
fn check_and_write_pid(pid_path: &PathBuf, force: bool) -> anyhow::Result<()> {
    if pid_path.exists() {
        let existing_pid_str = std::fs::read_to_string(pid_path)
            .unwrap_or_default()
            .trim()
            .to_string();

        if let Ok(existing_pid) = existing_pid_str.parse::<u32>() {
            if is_process_alive(existing_pid) {
                if force {
                    tracing::warn!(
                        pid = existing_pid,
                        "Another dcc-mcp-server (pid {existing_pid}) appears to be running. \
                         --force is set; overwriting PID file."
                    );
                } else {
                    anyhow::bail!(
                        "dcc-mcp-server is already running (pid {existing_pid}, \
                         pid file: {}). Use --force to override.",
                        pid_path.display()
                    );
                }
            } else {
                tracing::debug!(
                    "Stale PID file found (pid {existing_pid} is not running). Overwriting."
                );
            }
        }
    }

    let my_pid = std::process::id();
    std::fs::write(pid_path, my_pid.to_string())
        .map_err(|e| anyhow::anyhow!("Failed to write PID file {}: {e}", pid_path.display()))?;
    tracing::info!(pid = my_pid, path = %pid_path.display(), "PID file written");
    Ok(())
}

/// Cross-platform liveness check for an OS process.
///
/// Uses `sysinfo` (already a transitive dep via `dcc-mcp-process`) so no
/// extra platform-specific syscall crates are needed.
fn is_process_alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from(pid as usize)]), false);
    sys.process(Pid::from(pid as usize)).is_some()
}

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    // ── PID file ─────────────────────────────────────────────────────────────

    let pid_path = resolve_pid_path(&args);
    if let Some(ref p) = pid_path {
        check_and_write_pid(p, args.force)?;
    }

    // ── Collect skill paths ───────────────────────────────────────────────────

    let mut skill_paths: Vec<PathBuf> = args.skill_paths.clone();

    let env_paths = filesystem::get_skill_paths_from_env();
    skill_paths.extend(env_paths.into_iter().map(PathBuf::from));

    if !args.dcc.is_empty() {
        let app_paths = filesystem::get_app_skill_paths_from_env(&args.dcc);
        skill_paths.extend(app_paths.into_iter().map(PathBuf::from));
    }

    if let Ok(bundled) = filesystem::get_skills_dir(None) {
        let bundled_path = PathBuf::from(bundled);
        if bundled_path.exists() {
            skill_paths.push(bundled_path);
        }
    }

    tracing::info!(
        "Skill search paths: {:?}",
        skill_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    );

    // ── Build registry + catalog ──────────────────────────────────────────────

    let registry = Arc::new(ActionRegistry::new());
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        registry.clone(),
        dispatcher.clone(),
    ));

    // Discover skills into the catalog so they appear as stubs in tools/list.
    let dcc_hint = if args.dcc.is_empty() {
        None
    } else {
        Some(args.dcc.as_str())
    };
    let extra_dirs: Option<Vec<String>> = if skill_paths.is_empty() {
        None
    } else {
        Some(
            skill_paths
                .iter()
                .filter(|p| p.exists())
                .map(|p| p.display().to_string())
                .collect(),
        )
    };
    let n = catalog.discover(extra_dirs.as_deref(), dcc_hint);
    tracing::info!("Discovered {} skill(s) in catalog", n);

    // ── Start MCP HTTP server ─────────────────────────────────────────────────

    let config = McpHttpConfig::new(args.mcp_port)
        .with_name(args.server_name.clone())
        .with_cors();

    let mcp_server = McpHttpServer::with_catalog(registry.clone(), catalog.clone(), config)
        .with_dispatcher(dispatcher.clone());

    let handle = mcp_server.start().await?;

    tracing::info!(
        "MCP HTTP server listening on http://{}:{}",
        args.host,
        handle.port,
    );

    // ── Start WebSocket bridge server ─────────────────────────────────────────

    let bridge_state: Arc<BridgeState> = Arc::new(BridgeState::new());

    if !args.no_bridge {
        let ws_port = args.ws_port;
        let server_name = args.server_name.clone();
        let server_version = env!("CARGO_PKG_VERSION").to_string();
        let heartbeat_secs = args.heartbeat_secs;
        let state = bridge_state.clone();

        tokio::spawn(async move {
            run_ws_bridge(ws_port, server_name, server_version, heartbeat_secs, state).await;
        });

        tracing::info!(
            "WebSocket bridge listening on ws://127.0.0.1:{}  (heartbeat: {}s)",
            args.ws_port,
            args.heartbeat_secs,
        );
    }

    // ── Reconnect-timeout watchdog ────────────────────────────────────────────
    //
    // Once the DCC plugin has connected at least once, if it stays disconnected
    // for longer than `--reconnect-timeout-secs` the server exits automatically.
    // This allows sidecar supervisors (systemd, launchd, etc.) to decide
    // whether to restart the whole DCC+server pair.

    if !args.no_bridge && args.reconnect_timeout_secs > 0 {
        let timeout = Duration::from_secs(args.reconnect_timeout_secs);
        let state = bridge_state.clone();
        let timeout_secs = args.reconnect_timeout_secs;
        tokio::spawn(async move {
            let poll = Duration::from_secs(10);
            loop {
                tokio::time::sleep(poll).await;
                if !state.ever_connected.load(Ordering::Relaxed) {
                    continue; // haven't connected yet — don't start the clock
                }
                if state.is_connected() {
                    continue; // still connected — nothing to do
                }
                let elapsed = state
                    .last_disconnect
                    .lock()
                    .await
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);
                if elapsed >= timeout {
                    tracing::warn!(
                        timeout_secs,
                        "DCC plugin disconnected for {elapsed:.0?} — exiting (reconnect timeout)."
                    );
                    std::process::exit(1);
                }
            }
        });
    }

    // ── Wait for shutdown signal ──────────────────────────────────────────────

    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down…");

    // Remove PID file before exiting.
    if let Some(ref p) = pid_path {
        if let Err(e) = std::fs::remove_file(p) {
            tracing::warn!("Failed to remove PID file {}: {e}", p.display());
        } else {
            tracing::debug!("PID file removed: {}", p.display());
        }
    }

    handle.shutdown().await;
    Ok(())
}

// ── WebSocket bridge ──────────────────────────────────────────────────────────

/// Accept DCC plugin WebSocket connections and dispatch them to handler tasks.
async fn run_ws_bridge(
    port: u16,
    server_name: String,
    server_version: String,
    heartbeat_secs: u64,
    bridge_state: Arc<BridgeState>,
) {
    use tokio::net::TcpListener;

    let listener = match TcpListener::bind(format!("127.0.0.1:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind WebSocket bridge on port {port}: {e}");
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let sn = server_name.clone();
                let sv = server_version.clone();
                let state = bridge_state.clone();
                tracing::debug!("DCC plugin connecting from {addr}");
                tokio::spawn(async move {
                    handle_ws_connection(stream, addr, sn, sv, heartbeat_secs, state).await;
                });
            }
            Err(e) => {
                tracing::warn!("WS bridge accept error: {e}");
            }
        }
    }
}

/// Handle a single WebSocket connection from a DCC plugin.
///
/// Responsibilities:
/// - Heartbeat: sends a Ping every `heartbeat_secs` seconds via a shared
///   `Arc<Mutex<SplitSink>>`.  If the send fails the task exits, which also
///   causes the main receive loop to detect a dead connection on next recv.
/// - Responds to Ping frames from the DCC plugin with Pong (RFC 6455 §5.5.2).
/// - Tracks connect/disconnect in `BridgeState` for the watchdog.
async fn handle_ws_connection(
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
    server_name: String,
    server_version: String,
    heartbeat_secs: u64,
    bridge_state: Arc<BridgeState>,
) {
    use dcc_mcp_protocols::bridge::{BridgeHelloAck, BridgeMessage};
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!("WS handshake failed for {addr}: {e}");
            return;
        }
    };

    bridge_state.on_connect();
    tracing::info!(
        "DCC plugin connected from {addr}  (active: {})",
        bridge_state.connected_count.load(Ordering::Relaxed)
    );

    let (sink, mut stream) = ws_stream.split();
    // Wrap write half in Arc<Mutex> so the heartbeat task can share it.
    let sink = Arc::new(Mutex::new(sink));

    // Heartbeat task — periodically pings the DCC plugin.
    let heartbeat_handle = if heartbeat_secs > 0 {
        let tx = sink.clone();
        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(heartbeat_secs));
            interval.tick().await; // skip the immediate first tick
            loop {
                interval.tick().await;
                if tx
                    .lock()
                    .await
                    .send(Message::Ping(vec![].into()))
                    .await
                    .is_err()
                {
                    break; // write failed — connection is gone
                }
            }
        }))
    } else {
        None
    };

    let mut greeted = false;

    while let Some(msg_result) = stream.next().await {
        let raw = match msg_result {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Ping(data)) => {
                // RFC 6455 §5.5.2 — respond with Pong immediately.
                let _ = sink.lock().await.send(Message::Pong(data)).await;
                continue;
            }
            Ok(Message::Pong(_)) => {
                tracing::trace!("Pong received from {addr}");
                continue;
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(e) => {
                tracing::debug!("WS receive error from {addr}: {e}");
                break;
            }
        };

        match serde_json::from_str::<BridgeMessage>(&raw) {
            Ok(BridgeMessage::Hello(hello)) => {
                tracing::info!(
                    "DCC hello from {addr}: client={} version={}",
                    hello.client,
                    hello.version
                );
                let ack = BridgeMessage::HelloAck(BridgeHelloAck {
                    server: server_name.clone(),
                    version: server_version.clone(),
                    session_id: uuid::Uuid::new_v4().to_string(),
                });
                let text = serde_json::to_string(&ack).unwrap_or_default();
                let _ = sink.lock().await.send(Message::Text(text.into())).await;
                greeted = true;
            }
            Ok(BridgeMessage::Response(resp)) => {
                tracing::debug!("DCC response (id={}): {:?}", resp.id, resp.result);
            }
            Ok(BridgeMessage::Event(evt)) => {
                tracing::debug!("DCC event: {} {:?}", evt.event, evt.data);
            }
            Ok(BridgeMessage::Disconnect(_)) => {
                tracing::debug!("DCC plugin {addr} sent disconnect");
                break;
            }
            Ok(other) => {
                tracing::debug!("Unhandled bridge message from {addr}: {other:?}");
            }
            Err(e) => {
                let parse_err =
                    BridgeMessage::ParseError(dcc_mcp_protocols::bridge::BridgeParseError {
                        message: e.to_string(),
                    });
                let text = serde_json::to_string(&parse_err).unwrap_or_default();
                let _ = sink.lock().await.send(Message::Text(text.into())).await;
            }
        }
    }

    // Cancel heartbeat before returning.
    if let Some(h) = heartbeat_handle {
        h.abort();
    }

    bridge_state.on_disconnect().await;
    tracing::info!(
        "DCC plugin {addr} disconnected (greeted={greeted}, remaining: {})",
        bridge_state.connected_count.load(Ordering::Relaxed)
    );
}
