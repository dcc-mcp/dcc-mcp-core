//! Standalone `dcc-mcp-server` — DCC MCP server with integrated gateway.
//!
//! Every instance registers itself in a shared `FileRegistry` and **competes**
//! for a single well-known gateway port (default `:9765`).  Whichever process
//! wins the race becomes the **gateway**; the others are plain DCC instances.
//!
//! ## Why this matters
//!
//! You can start N DCC servers without any extra configuration:
//!
//! ```bash
//! # Terminal 1 — Maya, gets OS-assigned port :18812, wins gateway :9765
//! dcc-mcp-server --app maya
//!
//! # Terminal 2 — Maya, gets :18813, gateway port already taken → plain instance
//! dcc-mcp-server --app maya
//!
//! # Terminal 3 — Photoshop, gets :18814, plain instance
//! dcc-mcp-server --app photoshop
//! ```
//!
//! ```bash
//! # Agent always talks to one endpoint regardless of how many DCCs are running
//! curl http://localhost:9765/instances           # → [maya@18812, maya@18813, photoshop@18814]
//! curl -X POST http://localhost:9765/mcp \       # → read the gateway://instances resource
//!      -d '{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"gateway://instances"}}'
//! ```
//!
//! ## Gateway behaviour
//!
//! The gateway publishes the live DCC registry as the
//! `gateway://instances` MCP resource (read it via `resources/read`). Each
//! entry carries `mcp_url`, so a client can connect directly without any
//! follow-up tool call. The dynamic-capability surface
//! (`search_tools` / `describe_tool` / `call_tool`) and lease verbs
//! (`acquire_dcc_instance` / `release_dcc_instance`) are the only
//! gateway-published tools — every per-DCC backend tool is reached through
//! `call_tool` instead of being fanned out into `tools/list`.
//!
//! It also proxies tool calls transparently:
//!
//! ```
//! POST /mcp                    → discovery tools (no proxy)
//! POST /mcp/{instance_id}      → proxy to that DCC instance
//! POST /mcp/dcc/{dcc_type}     → proxy to best instance of that type
//! GET  /instances              → JSON list of all live instances (REST)
//! GET  /health                 → {"ok": true}
//! ```
//!
//! ## Python API
//!
//! The Python `McpHttpServer` gains `gateway_port` config so Maya/Blender
//! plugins can also participate in the gateway:
//!
//! ```python
//! from dcc_mcp_core import McpHttpServer, McpHttpConfig
//! config = McpHttpConfig(port=0, server_name="maya")
//! config.gateway_port = 9765   # join the gateway; 0 = disabled
//! server = McpHttpServer(registry, config)
//! server.start()
//! ```
//!
//! ## Environment variables
//!
//! | Variable                  | Description                                        |
//! |---------------------------|----------------------------------------------------|
//! | `DCC_MCP_SKILL_PATHS`     | Colon/semicolon-separated skill dirs               |
//! | `DCC_MCP_MCP_PORT`        | MCP HTTP server port (default 0 = OS-assigned)     |
//! | `DCC_MCP_WS_PORT`         | WebSocket bridge port (default 9001)               |
//! | `DCC_MCP_APP`             | App name hint (e.g. "maya", "photoshop")           |
//! | `DCC_MCP_SERVER_NAME`     | Server name advertised to MCP clients              |
//! | `DCC_MCP_GATEWAY_PORT`    | Gateway port to run/ensure (default 9765, 0=off)   |
//! | `DCC_MCP_GATEWAY_HOST`    | Gateway bind host (default follows `--host`)       |
//! | `DCC_MCP_GATEWAY_NAME`    | Human-readable gateway candidate/owner label       |
//! | `DCC_MCP_GATEWAY_REMOTE_HOST` | Optional remote gateway bind host (default 0.0.0.0) |
//! | `DCC_MCP_GATEWAY_REMOTE_PORT` | Optional remote gateway port (default 59765, 0=off) |
//! | `DCC_MCP_NO_ADMIN`        | Disable read-only `/admin` on the elected gateway  |
//! | `DCC_MCP_ADMIN_PATH`      | Admin URL prefix (default `/admin`)                |
//! | `DCC_MCP_GATEWAY_ADMIN_DB` | Override path for admin SQLite (traces / skill paths) |
//! | `DCC_MCP_GATEWAY_ADMIN_RETENTION_DAYS` | Admin SQLite retention in days (default 30, max 3650) |
//! | `DCC_MCP_WEBHOOKS_CONFIG` | YAML event webhook config for forwarding `skill.*`, `tool.*`, and other EventBus envelopes |
//! | `DCC_MCP_STANDALONE_REGISTRY_DCC_TYPE` | FileRegistry `dcc_type` when `--app` is empty (default `python`) |
//! | `DCC_MCP_REGISTRY_DIR`    | Shared FileRegistry directory                      |
//! | `DCC_MCP_STALE_TIMEOUT`   | Seconds without heartbeat = stale (default 30)     |

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
#[cfg(feature = "gateway-auto")]
use dcc_mcp_gateway::{AdminPersistConfig, GatewayConfig, GatewayRunner, SkillPathEntry};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_logging::file_logging::prune_old_logs;
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_skills::constants::resolve_registry_dcc_type;
#[cfg(feature = "gateway-auto")]
use dcc_mcp_skills::constants::{ENV_SKILL_PATHS, app_skill_paths_env_key};
#[cfg(feature = "gateway-auto")]
use dcc_mcp_transport::discovery::types::ServiceEntry;
use sysinfo::{Pid, ProcessesToUpdate, System};
mod capture;
mod event_webhooks;
#[cfg(feature = "gateway-daemon")]
mod gateway_daemon;
#[cfg(feature = "gateway-auto")]
mod sidecar;
#[cfg(feature = "gateway-auto")]
mod sidecar_gateway;
#[cfg(feature = "gateway-auto")]
mod sidecar_mcp;
mod translate;

// ── CLI ───────────────────────────────────────────────────────────────────────

/// DCC-MCP subcommands.
#[derive(Debug, Subcommand)]
enum SubCmd {
    /// Bridge any stdio MCP server to HTTP/SSE/Streamable-HTTP.
    Translate(translate::TranslateArgs),
    /// Catalog commands (search or describe DCC-MCP adapters).
    Catalog {
        #[command(subcommand)]
        action: CatalogAction,
    },
    /// Out-of-process worker for crash-isolated DCC actions (RFC #998).
    ///
    /// Spawned by a DCC plugin/addon (`dcc-mcp-maya`, `dcc-mcp-blender`, …)
    /// and supervised via `--watch-pid`.  Exits cleanly when its parent
    /// DCC dies so we never leak stale workers.
    #[cfg(feature = "gateway-auto")]
    Sidecar(sidecar::SidecarArgs),
    /// Machine-wide gateway daemon. Per-DCC sidecars auto-launch this when needed.
    #[cfg(feature = "gateway-daemon")]
    Gateway(gateway_daemon::GatewayArgs),
    /// Replay or diff gateway traffic capture files.
    Capture {
        #[command(subcommand)]
        action: capture::CaptureAction,
    },
}

/// DCC-MCP server with integrated auto-gateway.
#[derive(Debug, Parser)]
#[command(name = "dcc-mcp-server", about, version)]
struct Args {
    /// Optional subcommand. If omitted, runs as a DCC MCP server.
    #[command(subcommand)]
    command: Option<SubCmd>,
    /// MCP Streamable HTTP server port. Default 0 = OS-assigned.
    #[arg(long, env = "DCC_MCP_MCP_PORT", default_value = "0")]
    mcp_port: u16,

    /// WebSocket bridge server port (for non-Python DCC plugins).
    #[arg(long, env = "DCC_MCP_WS_PORT", default_value = "9001")]
    ws_port: u16,

    /// Application type (e.g. "maya", "photoshop", "blender").
    #[arg(long, env = "DCC_MCP_APP", default_value = "")]
    app: String,

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

    /// Write the server process ID to this file while running.
    #[arg(long, value_name = "PATH")]
    pid_file: Option<PathBuf>,

    /// Overwrite an existing PID file even if it points at a live process.
    #[arg(long, default_value = "false")]
    force: bool,

    /// Seconds to wait for graceful shutdown before exiting.
    #[arg(long, env = "DCC_MCP_SHUTDOWN_TIMEOUT_SECS", default_value = "10")]
    shutdown_timeout_secs: u64,

    // ── Gateway ──
    /// Gateway port to compete for. First instance to bind wins the gateway.
    /// 0 = gateway disabled entirely (and therefore disables admin too).
    #[arg(long, env = "DCC_MCP_GATEWAY_PORT", default_value = "9765")]
    gateway_port: u16,

    /// Gateway host/interface to bind. Defaults to the MCP `--host`.
    #[arg(long, env = "DCC_MCP_GATEWAY_HOST")]
    gateway_host: Option<String>,

    /// Human-readable gateway candidate name written to the `__gateway__`
    /// sentinel when this process wins or challenges the gateway role.
    #[arg(long, env = "DCC_MCP_GATEWAY_NAME")]
    gateway_name: Option<String>,

    /// Remote/LAN gateway host/interface to bind.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_HOST", default_value = "0.0.0.0")]
    gateway_remote_host: String,

    /// Remote/LAN gateway port. 0 disables the remote listener.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_PORT", default_value = "59765")]
    gateway_remote_port: u16,

    /// Disable the read-only Admin UI on the elected gateway.
    #[arg(long, env = "DCC_MCP_NO_ADMIN", default_value = "false")]
    no_admin: bool,

    /// URL prefix for the read-only Admin UI.
    #[arg(long, env = "DCC_MCP_ADMIN_PATH", default_value = "/admin")]
    admin_path: String,

    /// Directory for the shared FileRegistry (auto-created if missing).
    #[arg(long, env = "DCC_MCP_REGISTRY_DIR")]
    registry_dir: Option<String>,

    /// Seconds without a heartbeat before an instance is considered stale.
    #[arg(long, env = "DCC_MCP_STALE_TIMEOUT", default_value = "30")]
    stale_timeout_secs: u64,

    /// Application version (reported in registry, e.g. "2024.2").
    #[arg(long, env = "DCC_MCP_APP_VERSION")]
    app_version: Option<String>,

    /// Currently open scene file (reported in registry, improves routing).
    #[arg(long, env = "DCC_MCP_SCENE")]
    scene: Option<String>,

    /// Heartbeat interval in seconds for the registry. 0 = disabled.
    #[arg(long, env = "DCC_MCP_HEARTBEAT_INTERVAL", default_value = "5")]
    heartbeat_secs: u64,

    /// Reserved compatibility flag for older sidecar supervisors.
    #[arg(long, default_value = "30")]
    reconnect_timeout_secs: u64,

    /// Internal helper: watch a PID and remove its PID file after exit.
    #[arg(long, hide = true)]
    pid_cleanup_watch: Option<PathBuf>,

    /// Internal helper: PID to watch for `--pid-cleanup-watch`.
    #[arg(long, hide = true)]
    watch_pid: Option<u32>,

    // ── File logging ──
    /// Disable logging to rotating files. By default file logging is enabled
    /// unless this flag is passed.
    #[arg(long, env = "DCC_MCP_NO_LOG_FILE", default_value = "false")]
    no_log_file: bool,

    /// Directory for rotated log files. Defaults to the platform log dir
    /// (`dcc_mcp_paths::get_log_dir()`).
    #[arg(long, env = "DCC_MCP_LOG_DIR", value_name = "PATH")]
    log_dir: Option<PathBuf>,

    /// Maximum bytes per log file before a size-triggered rotation.
    #[arg(long, env = "DCC_MCP_LOG_MAX_SIZE", value_name = "BYTES")]
    log_max_size: Option<u64>,

    /// Number of **rolled** files to retain (current file excluded).
    #[arg(long, env = "DCC_MCP_LOG_MAX_FILES", value_name = "N")]
    log_max_files: Option<usize>,

    /// Rotation policy: `size`, `daily`, or `both`.
    #[arg(long, env = "DCC_MCP_LOG_ROTATION", value_name = "POLICY")]
    log_rotation: Option<String>,

    /// File-name prefix (full file is `<prefix>.<pid>.<YYYYMMDD>.log`).
    #[arg(long, env = "DCC_MCP_LOG_FILE_PREFIX", value_name = "PREFIX")]
    log_file_prefix: Option<String>,

    /// Log retention in days (0 = disable age pruning). Default: 7.
    #[arg(long, env = "DCC_MCP_LOG_RETENTION_DAYS", value_name = "DAYS")]
    log_retention_days: Option<u32>,

    /// Maximum total log directory size in MiB (0 = disable size pruning). Default: 100.
    #[arg(long, env = "DCC_MCP_LOG_MAX_TOTAL_SIZE_MB", value_name = "MB")]
    log_max_total_size_mb: Option<u32>,
}

#[derive(Debug, Default)]
struct FileLoggingCliOptions {
    no_log_file: bool,
    log_dir: Option<PathBuf>,
    log_max_size: Option<u64>,
    log_max_files: Option<usize>,
    log_rotation: Option<String>,
    log_file_prefix: Option<String>,
    log_retention_days: Option<u32>,
    log_max_total_size_mb: Option<u32>,
}

impl From<&Args> for FileLoggingCliOptions {
    fn from(args: &Args) -> Self {
        Self {
            no_log_file: args.no_log_file,
            log_dir: args.log_dir.clone(),
            log_max_size: args.log_max_size,
            log_max_files: args.log_max_files,
            log_rotation: args.log_rotation.clone(),
            log_file_prefix: args.log_file_prefix.clone(),
            log_retention_days: args.log_retention_days,
            log_max_total_size_mb: args.log_max_total_size_mb,
        }
    }
}

fn should_enable_file_logging(opts: &FileLoggingCliOptions, enabled_by_env: bool) -> bool {
    !opts.no_log_file
        || opts.log_dir.is_some()
        || opts.log_max_size.is_some()
        || opts.log_max_files.is_some()
        || opts.log_rotation.is_some()
        || opts.log_file_prefix.is_some()
        || opts.log_retention_days.is_some()
        || opts.log_max_total_size_mb.is_some()
        || enabled_by_env
}

// ── Catalog subcommands ───────────────────────────────────────────────────────

/// Catalog subcommand: query the public DCC-MCP catalog.
#[derive(Debug, Subcommand)]
enum CatalogAction {
    /// Search the catalog by keyword (name, description, DCC type, tags).
    Search {
        /// Keyword to search for. Omit to list all entries.
        #[arg(long, default_value = "")]
        query: String,
    },
    /// Show full details for a single catalog entry by exact name.
    Describe {
        /// Exact catalog entry name (e.g. dcc-mcp-maya-skills).
        #[arg(long)]
        name: String,
    },
}

fn run_catalog_cmd(action: &CatalogAction) -> anyhow::Result<()> {
    let catalog_path = if let Ok(p) = std::env::var("DCC_MCP_CATALOG_PATH") {
        PathBuf::from(p)
    } else {
        PathBuf::from("dcc-mcp-catalog.yml")
    };

    let entries = dcc_mcp_catalog::load_from_file(&catalog_path)?;

    match action {
        CatalogAction::Search { query } => {
            let hits = dcc_mcp_catalog::search(&entries, query);
            println!("{}", serde_json::to_string_pretty(&hits)?);
        }
        CatalogAction::Describe { name } => match dcc_mcp_catalog::describe(&entries, name) {
            Some(entry) => println!("{}", serde_json::to_string_pretty(&entry)?),
            None => {
                eprintln!("catalog entry '{}' not found", name);
                std::process::exit(1);
            }
        },
    }
    Ok(())
}

struct PidFileGuard {
    path: PathBuf,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %self.path.display(), %error, "failed to remove PID file");
        }
    }
}

impl PidFileGuard {
    fn remove_now(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %self.path.display(), %error, "failed to remove PID file");
        }
    }
}

pub(crate) fn is_process_alive(pid: u32) -> bool {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    sys.process(Pid::from_u32(pid)).is_some()
}

pub(crate) fn acquire_pid_file(
    path: &std::path::Path,
    force: bool,
) -> anyhow::Result<PidFileGuard> {
    let current_pid = std::process::id();

    if path.exists() {
        let existing = std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if let Some(raw_pid) = existing {
            if let Ok(existing_pid) = raw_pid.parse::<u32>() {
                let alive = existing_pid == current_pid || is_process_alive(existing_pid);
                if alive && !force {
                    return Err(anyhow::anyhow!(
                        "PID file '{}' already points to a running process ({existing_pid}); use --force to overwrite",
                        path.display()
                    ));
                }
                if alive {
                    tracing::warn!(
                        path = %path.display(),
                        existing_pid,
                        "overwriting live PID file because --force was set"
                    );
                } else {
                    tracing::warn!(
                        path = %path.display(),
                        existing_pid,
                        "overwriting stale PID file"
                    );
                }
            } else {
                tracing::warn!(path = %path.display(), "overwriting invalid PID file contents");
            }
        }
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(path, format!("{current_pid}\n"))?;
    Ok(PidFileGuard {
        path: path.to_path_buf(),
    })
}

pub(crate) fn spawn_pid_cleanup_watcher(path: &std::path::Path, pid: u32) {
    let Ok(exe) = std::env::current_exe() else {
        tracing::warn!("failed to resolve current executable for PID cleanup watcher");
        return;
    };

    let mut cmd = Command::new(exe);
    cmd.arg("--pid-cleanup-watch")
        .arg(path)
        .arg("--watch-pid")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }

    if let Err(error) = cmd.spawn() {
        tracing::warn!(path = %path.display(), %error, "failed to start PID cleanup watcher");
    }
}

fn run_pid_cleanup_watcher(path: PathBuf, pid: u32) {
    loop {
        if !is_process_alive(pid) {
            if let Err(error) = std::fs::remove_file(&path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(path = %path.display(), %error, "PID cleanup watcher failed to remove file");
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

// ── WebSocket bridge (unchanged from original) ────────────────────────────────

async fn run_ws_bridge(port: u16, server_name: String, server_version: String) {
    use dcc_mcp_protocols::bridge::{BridgeHelloAck, BridgeMessage};
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

    let listener = match TcpListener::bind(format!("127.0.0.1:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind WebSocket bridge on port {port}: {e}");
            return;
        }
    };
    tracing::info!("WebSocket bridge listening on ws://127.0.0.1:{port}");

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let sn = server_name.clone();
                let sv = server_version.clone();
                tokio::spawn(async move {
                    let ws = match tokio_tungstenite::accept_async(stream).await {
                        Ok(w) => w,
                        Err(e) => {
                            tracing::warn!("WS handshake failed for {addr}: {e}");
                            return;
                        }
                    };
                    let (mut sink, mut stream) = ws.split();
                    while let Some(Ok(msg)) = stream.next().await {
                        match msg {
                            Message::Text(t) => {
                                if let Ok(BridgeMessage::Hello(h)) =
                                    serde_json::from_str::<BridgeMessage>(&t)
                                {
                                    let ack = serde_json::to_string(&BridgeMessage::HelloAck(
                                        BridgeHelloAck {
                                            server: sn.clone(),
                                            version: sv.clone(),
                                            session_id: uuid::Uuid::new_v4().to_string(),
                                        },
                                    ))
                                    .unwrap_or_default();
                                    let _ = sink.send(Message::Text(ack.into())).await;
                                    tracing::info!(
                                        "DCC connected from {addr}: {} {}",
                                        h.client,
                                        h.version
                                    );
                                }
                            }
                            Message::Close(_) => break,
                            _ => {}
                        }
                    }
                    tracing::debug!("DCC plugin {addr} disconnected");
                });
            }
            Err(e) => tracing::warn!("WS bridge accept error: {e}"),
        }
    }
}

// ── shutdown signals ─────────────────────────────────────────────────────────

pub(crate) async fn select_shutdown_signal() -> anyhow::Result<&'static str> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sighup = signal(SignalKind::hangup())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result?;
                Ok("ctrl_c")
            }
            _ = sigterm.recv() => Ok("sigterm"),
            _ = sighup.recv() => Ok("sighup"),
        }
    }
    #[cfg(windows)]
    {
        let mut ctrl_break = tokio::signal::windows::ctrl_break()?;
        let mut ctrl_shutdown = tokio::signal::windows::ctrl_shutdown()?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result?;
                Ok("ctrl_c")
            }
            _ = ctrl_break.recv() => Ok("ctrl_break"),
            _ = ctrl_shutdown.recv() => Ok("ctrl_shutdown"),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        tokio::signal::ctrl_c().await?;
        Ok("ctrl_c")
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
// Without the `server` feature, the early `return Err(...)` in the
// no-subcommand arm makes the rest of `main` provably unreachable. That
// is intentional, not a bug — silence the lint only for that build.
#[cfg_attr(not(feature = "server"), allow(unreachable_code, unused_variables))]
async fn main() -> anyhow::Result<()> {
    // Install the shared subscriber (stderr fmt-layer + reload slot for the
    // optional file-logging layer). Safe to call multiple times.
    dcc_mcp_logging::init_logging();

    // ── Auto-init telemetry from OTEL_EXPORTER_OTLP_ENDPOINT ─────────────
    // If the standard OTel env var is present, wire up the OTLP gRPC exporter.
    // Otherwise, install a minimal no-op provider to suppress OTel warnings.
    #[cfg(feature = "telemetry")]
    {
        let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
        let telemetry_cfg = if let Some(ref endpoint) = otlp_endpoint {
            tracing::info!(endpoint, "OTLP endpoint detected — enabling OTLP telemetry");
            dcc_mcp_telemetry::types::TelemetryConfig::builder("dcc-mcp-server")
                .with_otlp_exporter(endpoint.clone())
                .build()
        } else {
            dcc_mcp_telemetry::types::TelemetryConfig {
                enable_metrics: true,
                enable_tracing: false,
                exporter: dcc_mcp_telemetry::types::ExporterBackend::Noop,
                ..dcc_mcp_telemetry::types::TelemetryConfig::default()
            }
        };
        if let Err(e) = dcc_mcp_telemetry::provider::init(&telemetry_cfg) {
            tracing::warn!(%e, "telemetry init skipped");
        }
    }
    #[cfg(not(feature = "telemetry"))]
    {
        // No telemetry crate compiled in — nothing to do.
    }

    let args = Args::parse();

    // ── Dispatch to subcommands ───────────────────────────────────────────
    match args.command {
        Some(SubCmd::Translate(translate_args)) => return translate::run(translate_args).await,
        Some(SubCmd::Catalog { action }) => return run_catalog_cmd(&action),
        #[cfg(feature = "gateway-auto")]
        Some(SubCmd::Sidecar(sidecar_args)) => return sidecar::run(sidecar_args).await,
        #[cfg(feature = "gateway-daemon")]
        Some(SubCmd::Gateway(gateway_args)) => return gateway_daemon::run(gateway_args).await,
        Some(SubCmd::Capture { action }) => return capture::run(action).await,
        None => {}
    }

    // When this binary is built without the `server` feature, the default
    // (no-subcommand) path has nothing useful to do — print help and exit
    // cleanly so callers get a clear signal instead of opening a port.
    #[cfg(not(feature = "server"))]
    {
        use clap::CommandFactory as _;
        let mut cmd = Args::command();
        cmd.print_long_help().ok();
        return Err(anyhow::anyhow!(
            "this build was compiled without the `server` feature; \
             use a subcommand such as `gateway` to invoke the binary"
        ));
    }

    if let (Some(path), Some(pid)) = (args.pid_cleanup_watch.clone(), args.watch_pid) {
        run_pid_cleanup_watcher(path, pid);
        return Ok(());
    }

    // Wire up rolling-file logging by default unless --no-log-file is passed.
    // Any explicit DCC_MCP_LOG_* env var or CLI flag also enables it.
    let file_logging_cli = FileLoggingCliOptions::from(&args);
    if should_enable_file_logging(
        &file_logging_cli,
        dcc_mcp_logging::FileLoggingConfig::enabled_by_env(),
    ) {
        let mut cfg = dcc_mcp_logging::FileLoggingConfig::from_env_with_defaults()
            .map_err(|e| anyhow::anyhow!("invalid file-logging env vars: {e}"))?;
        if let Some(dir) = args.log_dir.clone() {
            cfg.directory = Some(dir);
        }
        if let Some(size) = args.log_max_size {
            cfg.max_size_bytes = size;
        }
        if let Some(n) = args.log_max_files {
            cfg.max_files = n;
        }
        if let Some(ref rot) = args.log_rotation {
            cfg.rotation = dcc_mcp_logging::RotationPolicy::parse(rot)
                .map_err(|e| anyhow::anyhow!("invalid --log-rotation: {e}"))?;
        }
        if let Some(ref prefix) = args.log_file_prefix {
            if !prefix.trim().is_empty() {
                cfg.file_name_prefix = prefix.clone();
            }
        } else {
            // PID-based naming for multi-instance debugging.
            cfg.file_name_prefix = format!("dcc-mcp-server.{}", std::process::id());
        }
        if let Some(days) = args.log_retention_days {
            cfg.retention_days = days;
        }
        if let Some(mb) = args.log_max_total_size_mb {
            cfg.max_total_size_mb = mb;
        }
        // Save retention settings before cfg is moved into init_file_logging.
        let retention = cfg.retention_days;
        let max_size = cfg.max_total_size_mb;
        let prefix = cfg.file_name_prefix.clone();
        match dcc_mcp_logging::init_file_logging(cfg) {
            Ok(dir) => {
                tracing::info!(
                    path = %dir.display(),
                    "rolling file logging enabled",
                );
                // Prune old log files on startup (issue #558).
                prune_old_logs(&dir, &prefix, retention, max_size);
            }
            Err(e) => {
                tracing::warn!(%e, "failed to enable file logging; continuing with stderr only")
            }
        }
    }

    let mut pid_file_guard = args
        .pid_file
        .as_deref()
        .map(|path| acquire_pid_file(path, args.force))
        .transpose()?;
    if let Some(path) = args.pid_file.as_deref() {
        spawn_pid_cleanup_watcher(path, std::process::id());
    }

    // ── Collect skill paths ───────────────────────────────────────────────

    let registry_dir_path: Option<PathBuf> = args.registry_dir.as_deref().map(PathBuf::from);

    // `skill_paths_snapshot` is fed straight into the gateway admin UI's
    // `AdminPersistConfig`. Slim builds without `gateway-auto` drop the
    // entire admin pipeline, so we skip building the snapshot too.
    #[cfg(feature = "gateway-auto")]
    let mut skill_paths_snapshot: Vec<SkillPathEntry> = Vec::new();
    #[cfg(feature = "gateway-auto")]
    for p in &args.skill_paths {
        skill_paths_snapshot.push(SkillPathEntry {
            path: p.display().to_string(),
            source: "cli".into(),
        });
    }

    let mut skill_paths: Vec<PathBuf> = args.skill_paths.clone();
    skill_paths.extend(
        dcc_mcp_skills::paths::get_skill_paths_from_env()
            .into_iter()
            .inspect(|_s| {
                #[cfg(feature = "gateway-auto")]
                skill_paths_snapshot.push(SkillPathEntry {
                    path: _s.clone(),
                    source: format!("env:{ENV_SKILL_PATHS}"),
                });
            })
            .map(PathBuf::from),
    );
    if !args.app.is_empty() {
        #[cfg(feature = "gateway-auto")]
        let env_key = app_skill_paths_env_key(&args.app);
        skill_paths.extend(
            dcc_mcp_skills::paths::get_app_skill_paths_from_env(&args.app)
                .into_iter()
                .inspect(|_s| {
                    #[cfg(feature = "gateway-auto")]
                    skill_paths_snapshot.push(SkillPathEntry {
                        path: _s.clone(),
                        source: format!("env:{env_key}"),
                    });
                })
                .map(PathBuf::from),
        );
        if let Ok(local_default) = dcc_mcp_skills::paths::get_local_skills_dir(Some(&args.app)) {
            match std::fs::create_dir_all(&local_default) {
                Ok(()) => {
                    let p = PathBuf::from(&local_default);
                    #[cfg(feature = "gateway-auto")]
                    skill_paths_snapshot.push(SkillPathEntry {
                        path: local_default.clone(),
                        source: "local_default".into(),
                    });
                    if !skill_paths.iter().any(|x| x == &p) {
                        skill_paths.push(p);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        path = %local_default,
                        error = %err,
                        "could not initialise local default skill directory"
                    );
                }
            }
        }
    }
    if let Ok(bundled) = dcc_mcp_skills::paths::get_skills_dir(None) {
        let p = PathBuf::from(&bundled);
        if p.exists() {
            #[cfg(feature = "gateway-auto")]
            skill_paths_snapshot.push(SkillPathEntry {
                path: bundled.clone(),
                source: "bundled".into(),
            });
            skill_paths.push(p);
        }
    }

    #[cfg(feature = "gateway-auto")]
    let skill_paths_for_catalog_reload = skill_paths.clone();

    #[cfg(feature = "gateway-auto")]
    let admin_db =
        dcc_mcp_gateway::gateway::admin::resolve_admin_db_path(None, registry_dir_path.as_ref());
    #[cfg(feature = "gateway-auto")]
    for p in
        dcc_mcp_gateway::gateway::admin::sqlite_lane::read_custom_skill_paths_for_startup(&admin_db)
    {
        if p.exists() {
            skill_paths_snapshot.push(SkillPathEntry {
                path: p.display().to_string(),
                source: "admin_custom".into(),
            });
            if !skill_paths.iter().any(|x| x == &p) {
                skill_paths.push(p);
            }
        }
    }

    // ── Build registry + catalog ──────────────────────────────────────────

    let action_registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(ToolDispatcher::new((*action_registry).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        action_registry.clone(),
        dispatcher.clone(),
    ));
    let _event_webhook_runtime =
        event_webhooks::EventWebhookRuntime::from_env(dispatcher.event_bus())?;

    let app_hint = if args.app.is_empty() {
        None
    } else {
        Some(args.app.as_str())
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

    let n = catalog.discover(extra_dirs.as_deref(), app_hint);
    tracing::info!("Discovered {} skill(s) in catalog", n);

    #[cfg(feature = "gateway-auto")]
    let catalog_discover_hook: Arc<dyn Fn() + Send + Sync> = {
        let catalog = catalog.clone();
        let base_dirs = skill_paths_for_catalog_reload.clone();
        let admin_db_path = admin_db.clone();
        let app_owned = args.app.clone();
        Arc::new(move || {
            let mut merged = base_dirs.clone();
            for p in
                dcc_mcp_gateway::gateway::admin::read_custom_skill_paths_for_startup(&admin_db_path)
            {
                if p.exists() && !merged.iter().any(|x| x == &p) {
                    merged.push(p);
                }
            }
            let extra: Vec<String> = merged
                .into_iter()
                .filter(|p| p.exists())
                .map(|p| p.display().to_string())
                .collect();
            let hint = if app_owned.is_empty() {
                None
            } else {
                Some(app_owned.as_str())
            };
            let discovered = catalog.rediscover(Some(&extra), hint);
            tracing::info!(
                discovered,
                "catalog.rediscover after admin skill-path change (hook)"
            );
        })
    };

    // ── Start MCP HTTP server (DCC-specific tools) ────────────────────────

    let mut config = McpHttpConfig::default();
    config.server.port = args.mcp_port;
    config = config.with_name(args.server_name.clone()).with_cors();
    config.server.host = args
        .host
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid --host '{}': {e}", args.host))?;

    let mcp_server = McpHttpServer::with_catalog(action_registry.clone(), catalog.clone(), config)
        .with_dispatcher(dispatcher.clone());

    let handle = mcp_server.start().await?;

    let registry_dcc =
        resolve_registry_dcc_type((!args.app.is_empty()).then_some(args.app.as_str()));

    tracing::info!(
        "MCP server listening on http://{}:{}/mcp  (app={})",
        args.host,
        handle.port,
        registry_dcc,
    );

    // ── Register + gateway competition (via library) ──────────────────────

    #[cfg(feature = "gateway-auto")]
    let gw_handle = {
        let admin_retention = std::env::var("DCC_MCP_GATEWAY_ADMIN_RETENTION_DAYS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(30)
            .clamp(1, 3650);

        let gateway_host = args
            .gateway_host
            .clone()
            .unwrap_or_else(|| args.host.clone());

        let gateway_cfg = GatewayConfig {
            host: gateway_host,
            gateway_port: args.gateway_port,
            remote_host: Some(args.gateway_remote_host.clone()),
            remote_gateway_port: args.gateway_remote_port,
            stale_timeout_secs: args.stale_timeout_secs,
            heartbeat_secs: args.heartbeat_secs,
            server_name: args.server_name.clone(),
            gateway_name: args.gateway_name.clone(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            registry_dir: registry_dir_path,
            // Issue maya#137: standalone server has no adapter package, so
            // the election treats it as the lowest tier and yields to any
            // real DCC adapter at equal crate version.
            adapter_dcc: if args.app.is_empty() {
                None
            } else {
                Some(args.app.clone())
            },
            admin_enabled: !args.no_admin,
            admin_path: args.admin_path.clone(),
            admin_persist: AdminPersistConfig {
                sqlite_path: std::env::var_os("DCC_MCP_GATEWAY_ADMIN_DB").map(PathBuf::from),
                sqlite_retention_days: admin_retention,
                skill_paths_snapshot,
                skill_paths_reload: Some(catalog_discover_hook),
            },
            ..GatewayConfig::default()
        };

        let runner = GatewayRunner::new(gateway_cfg)
            .map_err(|e| anyhow::anyhow!("Failed to create GatewayRunner: {e}"))?;

        let mut entry = ServiceEntry::new(registry_dcc.as_str(), &args.host, handle.port);
        entry.version = args.app_version.clone();
        entry.scene = args.scene.clone();
        entry
            .metadata
            .insert("server_name".to_string(), args.server_name.clone());
        entry.metadata.insert(
            "mcp_url".to_string(),
            format!("http://{}:{}/mcp", args.host, handle.port),
        );

        // Standalone binary: scene is fixed at launch; no live provider needed.
        runner
            .start(entry, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start gateway: {e}"))?
    };
    #[cfg(feature = "gateway-auto")]
    let is_gateway = gw_handle.is_gateway;
    #[cfg(not(feature = "gateway-auto"))]
    let _ = (&registry_dir_path, &registry_dcc);

    // ── Start WebSocket bridge (optional) ─────────────────────────────────

    if !args.no_bridge {
        let ws_port = args.ws_port;
        let sn = args.server_name.clone();
        let sv = env!("CARGO_PKG_VERSION").to_string();
        tokio::spawn(async move { run_ws_bridge(ws_port, sn, sv).await });
    }

    // ── Wait for shutdown signal ──────────────────────────────────────────

    let shutdown_reason = select_shutdown_signal().await?;
    tracing::info!(shutdown_reason, "Shutdown signal received");

    #[cfg(feature = "gateway-auto")]
    {
        if is_gateway {
            tracing::info!("Gateway port released");
        }
        // gw_handle dropped here — aborts heartbeat, cleanup, and gateway tasks automatically
        drop(gw_handle);
    }

    let deadline = Duration::from_secs(args.shutdown_timeout_secs);
    match tokio::time::timeout(deadline, handle.shutdown()).await {
        Ok(()) => tracing::info!("Graceful shutdown complete"),
        Err(_) => tracing::error!(?deadline, "Graceful shutdown exceeded deadline, exiting"),
    }
    if let Some(guard) = pid_file_guard.as_mut() {
        guard.remove_now();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_log_file_disables_default_file_logging() {
        let opts = FileLoggingCliOptions {
            no_log_file: true,
            ..FileLoggingCliOptions::default()
        };

        assert!(!should_enable_file_logging(&opts, false));
    }

    #[test]
    fn parsed_no_log_file_has_no_implicit_retention_override() {
        let args = Args::try_parse_from([
            "dcc-mcp-server",
            "--no-log-file",
            "--gateway-port",
            "0",
            "--no-bridge",
        ])
        .expect("valid CLI args");
        let opts = FileLoggingCliOptions::from(&args);

        assert!(opts.log_retention_days.is_none());
        assert!(opts.log_max_total_size_mb.is_none());
        assert!(!should_enable_file_logging(&opts, false));
    }

    #[test]
    fn explicit_log_option_overrides_no_log_file() {
        let opts = FileLoggingCliOptions {
            no_log_file: true,
            log_retention_days: Some(3),
            ..FileLoggingCliOptions::default()
        };

        assert!(should_enable_file_logging(&opts, false));
    }

    #[test]
    fn env_logging_option_overrides_no_log_file() {
        let opts = FileLoggingCliOptions {
            no_log_file: true,
            ..FileLoggingCliOptions::default()
        };

        assert!(should_enable_file_logging(&opts, true));
    }
}
