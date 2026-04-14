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
use std::sync::Arc;

use clap::Parser;
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_http::gateway::{GatewayConfig, GatewayRunner};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use dcc_mcp_utils::filesystem;

// ── CLI ───────────────────────────────────────────────────────────────────────

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

    // ── Gateway ──
    /// Gateway port to compete for. First instance to bind wins the gateway.
    /// 0 = gateway disabled entirely.
    #[arg(long, env = "DCC_MCP_GATEWAY_PORT", default_value = "9765")]
    gateway_port: u16,

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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    // ── Collect skill paths ───────────────────────────────────────────────

    let mut skill_paths: Vec<PathBuf> = args.skill_paths.clone();
    skill_paths.extend(
        filesystem::get_skill_paths_from_env()
            .into_iter()
            .map(PathBuf::from),
    );
    if !args.dcc.is_empty() {
        skill_paths.extend(
            filesystem::get_app_skill_paths_from_env(&args.dcc)
                .into_iter()
                .map(PathBuf::from),
        );
    }
    if let Ok(bundled) = filesystem::get_skills_dir(None) {
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

    let config = McpHttpConfig::new(args.mcp_port)
        .with_name(args.server_name.clone())
        .with_cors();

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

    let gw_handle = runner
        .start(entry)
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
    Ok(())
}
