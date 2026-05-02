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
//! dcc-mcp-server --dcc maya
//!
//! # Terminal 2 — Maya, gets :18813, gateway port already taken → plain instance
//! dcc-mcp-server --dcc maya
//!
//! # Terminal 3 — Photoshop, gets :18814, plain instance
//! dcc-mcp-server --dcc photoshop
//! ```
//!
//! ```bash
//! # Agent always talks to one endpoint regardless of how many DCCs are running
//! curl http://localhost:9765/instances           # → [maya@18812, maya@18813, photoshop@18814]
//! curl -X POST http://localhost:9765/mcp \       # → list_dcc_instances / connect_to_dcc
//!      -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_dcc_instances"}}'
//! ```
//!
//! ## Gateway behaviour
//!
//! The gateway exposes **three discovery meta-tools** via its own MCP endpoint:
//!
//! | Tool | Description |
//! |------|-------------|
//! | `list_dcc_instances` | List all live DCC servers (type, port, scene, status) |
//! | `get_dcc_instance`   | Get info for a specific instance (by id or dcc_type+scene) |
//! | `connect_to_dcc`     | Return the direct MCP URL for a DCC instance |
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
//! | `DCC_MCP_DCC`             | DCC name hint (e.g. "maya", "photoshop")           |
//! | `DCC_MCP_SERVER_NAME`     | Server name advertised to MCP clients              |
//! | `DCC_MCP_GATEWAY_PORT`    | Gateway port to compete for (default 9765, 0=off)  |
//! | `DCC_MCP_REGISTRY_DIR`    | Shared FileRegistry directory                      |
//! | `DCC_MCP_STALE_TIMEOUT`   | Seconds without heartbeat = stale (default 30)     |

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_http::gateway::{GatewayConfig, GatewayRunner};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use sysinfo::{Pid, ProcessesToUpdate, System};

// ── CLI ───────────────────────────────────────────────────────────────────────

/// Clap [`value_parser`](clap::Arg::value_parser) for `--gateway-tool-exposure`.
///
/// Parses the documented `full | slim | both | rest` vocabulary
/// case-insensitively and surfaces the full list of accepted values on
/// error so operators can fix typos without digging into docs.
fn parse_gateway_tool_exposure(
    s: &str,
) -> Result<dcc_mcp_http::gateway::GatewayToolExposure, String> {
    s.parse()
        .map_err(|e: dcc_mcp_http::gateway::ParseGatewayToolExposureError| e.to_string())
}

/// DCC-MCP server with integrated auto-gateway.
#[derive(Debug, Parser)]
#[command(name = "dcc-mcp-server", about, version)]
struct Args {
    /// MCP Streamable HTTP server port. Default 0 = OS-assigned.
    #[arg(long, env = "DCC_MCP_MCP_PORT", default_value = "0")]
    mcp_port: u16,

    /// WebSocket bridge server port (for non-Python DCC plugins).
    #[arg(long, env = "DCC_MCP_WS_PORT", default_value = "9001")]
    ws_port: u16,

    /// DCC application type (e.g. "maya", "photoshop", "blender").
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

    /// Write the server process ID to this file while running.
    #[arg(long, value_name = "PATH")]
    pid_file: Option<PathBuf>,

    /// Overwrite an existing PID file even if it points at a live process.
    #[arg(long, default_value = "false")]
    force: bool,

    // ── Gateway ──
    /// Gateway port to compete for. First instance to bind wins the gateway.
    /// 0 = gateway disabled entirely.
    #[arg(long, env = "DCC_MCP_GATEWAY_PORT", default_value = "9765")]
    gateway_port: u16,

    /// Gateway tool-exposure mode (issue #652).
    ///
    /// * `full` — publish every live backend tool through `tools/list`
    ///   (legacy behavior; default for compatibility).
    /// * `slim` — publish only gateway meta-tools + skill management;
    ///   backend capabilities reached via dynamic wrappers.
    /// * `both` — alias of `full` today; reserved for the transition
    ///   window once dynamic wrapper tools land (#657).
    /// * `rest` — same bounded surface as `slim`; signals that REST is
    ///   the canonical capability API.
    #[arg(
        long,
        env = "DCC_MCP_GATEWAY_TOOL_EXPOSURE",
        default_value = "full",
        value_parser = parse_gateway_tool_exposure,
    )]
    gateway_tool_exposure: dcc_mcp_http::gateway::GatewayToolExposure,

    /// Emit Cursor-safe gateway tool names (`i_<id8>__<escaped>`) instead
    /// of the pre-#656 SEP-986 dotted form (`<id8>.<tool>`). Issue #656.
    ///
    /// When `true` (the default), every gateway-published tool name
    /// matches the stricter `^[A-Za-z0-9_]+$` regex enforced by Cursor
    /// and several other MCP clients, which silently hide names
    /// containing `.` or `-`. The legacy dotted form is still decoded
    /// for the compatibility window so in-flight clients keep routing.
    ///
    /// Set to `false` only when you need diagnostic parity with a
    /// single-instance server that publishes SEP-986 dotted names
    /// directly.
    #[arg(
        long,
        env = "DCC_MCP_GATEWAY_CURSOR_SAFE_TOOL_NAMES",
        default_value = "true",
        action = clap::ArgAction::Set,
    )]
    gateway_cursor_safe_tool_names: bool,

    /// Directory for the shared FileRegistry (auto-created if missing).
    #[arg(long, env = "DCC_MCP_REGISTRY_DIR")]
    registry_dir: Option<String>,

    /// Seconds without a heartbeat before an instance is considered stale.
    #[arg(long, env = "DCC_MCP_STALE_TIMEOUT", default_value = "30")]
    stale_timeout_secs: u64,

    /// DCC application version (reported in registry, e.g. "2024.2").
    #[arg(long, env = "DCC_MCP_DCC_VERSION")]
    dcc_version: Option<String>,

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

fn is_process_alive(pid: u32) -> bool {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    sys.process(Pid::from_u32(pid)).is_some()
}

fn acquire_pid_file(path: &std::path::Path, force: bool) -> anyhow::Result<PidFileGuard> {
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

fn spawn_pid_cleanup_watcher(path: &std::path::Path, pid: u32) {
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

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install the shared subscriber (stderr fmt-layer + reload slot for the
    // optional file-logging layer). Safe to call multiple times.
    dcc_mcp_logging::init_logging();

    let args = Args::parse();

    if let (Some(path), Some(pid)) = (args.pid_cleanup_watch.clone(), args.watch_pid) {
        run_pid_cleanup_watcher(path, pid);
        return Ok(());
    }

    // Wire up rolling-file logging by default unless --no-log-file is passed.
    // Any explicit DCC_MCP_LOG_* env var or CLI flag also enables it.
    if !args.no_log_file
        || args.log_dir.is_some()
        || args.log_max_size.is_some()
        || args.log_max_files.is_some()
        || args.log_rotation.is_some()
        || args.log_file_prefix.is_some()
        || args.log_retention_days.is_some()
        || args.log_max_total_size_mb.is_some()
        || dcc_mcp_logging::FileLoggingConfig::enabled_by_env()
    {
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
        match dcc_mcp_logging::init_file_logging(cfg) {
            Ok(dir) => tracing::info!(
                path = %dir.display(),
                "rolling file logging enabled",
            ),
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

    let mut skill_paths: Vec<PathBuf> = args.skill_paths.clone();
    skill_paths.extend(
        dcc_mcp_skills::paths::get_skill_paths_from_env()
            .into_iter()
            .map(PathBuf::from),
    );
    if !args.dcc.is_empty() {
        skill_paths.extend(
            dcc_mcp_skills::paths::get_app_skill_paths_from_env(&args.dcc)
                .into_iter()
                .map(PathBuf::from),
        );
    }
    if let Ok(bundled) = dcc_mcp_skills::paths::get_skills_dir(None) {
        let p = PathBuf::from(bundled);
        if p.exists() {
            skill_paths.push(p);
        }
    }

    // ── Build registry + catalog ──────────────────────────────────────────

    let action_registry = Arc::new(ActionRegistry::new());
    let dispatcher = Arc::new(ActionDispatcher::new((*action_registry).clone()));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        action_registry.clone(),
        dispatcher.clone(),
    ));

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

    // ── Start MCP HTTP server (DCC-specific tools) ────────────────────────

    let mut config = McpHttpConfig::new(args.mcp_port)
        .with_name(args.server_name.clone())
        .with_cors();
    config.host = args
        .host
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid --host '{}': {e}", args.host))?;

    let mcp_server = McpHttpServer::with_catalog(action_registry.clone(), catalog.clone(), config)
        .with_dispatcher(dispatcher.clone());

    let handle = mcp_server.start().await?;

    tracing::info!(
        "MCP server listening on http://{}:{}/mcp  (dcc={})",
        args.host,
        handle.port,
        if args.dcc.is_empty() {
            "generic"
        } else {
            &args.dcc
        }
    );

    // ── Register + gateway competition (via library) ──────────────────────

    let registry_dir_path: Option<PathBuf> = args.registry_dir.as_deref().map(PathBuf::from);

    let gateway_cfg = GatewayConfig {
        host: args.host.clone(),
        gateway_port: args.gateway_port,
        stale_timeout_secs: args.stale_timeout_secs,
        heartbeat_secs: args.heartbeat_secs,
        server_name: args.server_name.clone(),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        registry_dir: registry_dir_path,
        allow_unknown_tools: false,
        challenger_timeout_secs: 120,
        backend_timeout_ms: 10_000,
        async_dispatch_timeout_ms: 60_000,
        wait_terminal_timeout_ms: 600_000,
        route_ttl_secs: 60 * 60 * 24,
        max_routes_per_session: 1_000,
        // Issue maya#137: standalone server has no adapter package, so the
        // election treats it as the lowest tier and yields to any real
        // DCC adapter at equal crate version.
        adapter_version: None,
        adapter_dcc: if args.dcc.is_empty() {
            None
        } else {
            Some(args.dcc.clone())
        },
        tool_exposure: args.gateway_tool_exposure,
        cursor_safe_tool_names: args.gateway_cursor_safe_tool_names,
    };

    let runner = GatewayRunner::new(gateway_cfg)
        .map_err(|e| anyhow::anyhow!("Failed to create GatewayRunner: {e}"))?;

    let mut entry = ServiceEntry::new(
        if args.dcc.is_empty() {
            "unknown"
        } else {
            &args.dcc
        },
        &args.host,
        handle.port,
    );
    entry.version = args.dcc_version.clone();
    entry.scene = args.scene.clone();
    entry
        .metadata
        .insert("server_name".to_string(), args.server_name.clone());
    entry.metadata.insert(
        "mcp_url".to_string(),
        format!("http://{}:{}/mcp", args.host, handle.port),
    );

    // Standalone binary: scene is fixed at launch; no live provider needed.
    let gw_handle = runner
        .start(entry, None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start gateway: {e}"))?;
    let is_gateway = gw_handle.is_gateway;

    // ── Start WebSocket bridge (optional) ─────────────────────────────────

    if !args.no_bridge {
        let ws_port = args.ws_port;
        let sn = args.server_name.clone();
        let sv = env!("CARGO_PKG_VERSION").to_string();
        tokio::spawn(async move { run_ws_bridge(ws_port, sn, sv).await });
    }

    // ── Wait for Ctrl+C ───────────────────────────────────────────────────

    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down…");

    if is_gateway {
        tracing::info!("Gateway port released");
    }
    // gw_handle dropped here — aborts heartbeat, cleanup, and gateway tasks automatically

    handle.shutdown().await;
    if let Some(guard) = pid_file_guard.as_mut() {
        guard.remove_now();
    }
    Ok(())
}
