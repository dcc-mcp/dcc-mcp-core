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
use std::time::Duration;

use anyhow::Context as _;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing};
use clap::Parser;
use dcc_mcp_actions::{ActionDispatcher, ActionRegistry};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_skills::SkillCatalog;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
use dcc_mcp_utils::filesystem;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

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

// ── Shared gateway state ──────────────────────────────────────────────────────

#[derive(Clone)]
struct GatewayState {
    registry: Arc<RwLock<FileRegistry>>,
    stale_timeout: Duration,
    server_name: String,
    server_version: String,
    http_client: reqwest::Client,
}

impl GatewayState {
    fn live_instances(&self, registry: &FileRegistry) -> Vec<ServiceEntry> {
        registry
            .list_all()
            .into_iter()
            .filter(|e| {
                !e.is_stale(self.stale_timeout)
                    && !matches!(
                        e.status,
                        ServiceStatus::ShuttingDown | ServiceStatus::Unreachable
                    )
            })
            .collect()
    }
}

// ── JSON-RPC types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

// ── Gateway REST handlers ─────────────────────────────────────────────────────

async fn handle_health() -> impl IntoResponse {
    Json(json!({"ok": true, "service": "dcc-mcp-gateway"}))
}

async fn handle_instances(State(gs): State<GatewayState>) -> impl IntoResponse {
    let reg = gs.registry.read().await;
    let instances: Vec<Value> = gs
        .live_instances(&reg)
        .into_iter()
        .map(|e| entry_to_json(&e, gs.stale_timeout))
        .collect();
    Json(json!({ "total": instances.len(), "instances": instances }))
}

fn entry_to_json(e: &ServiceEntry, stale_timeout: Duration) -> Value {
    json!({
        "instance_id": e.instance_id.to_string(),
        "dcc_type": e.dcc_type,
        "host": e.host,
        "port": e.port,
        "mcp_url": format!("http://{}:{}/mcp", e.host, e.port),
        "status": e.status.to_string(),
        "scene": e.scene,
        "version": e.version,
        "metadata": e.metadata,
        "stale": e.is_stale(stale_timeout),
    })
}

// ── Gateway MCP endpoint ──────────────────────────────────────────────────────

/// `POST /mcp` — gateway's own MCP endpoint with discovery meta-tools.
/// Does NOT proxy; returns direct URLs for agents to use.
async fn handle_gateway_mcp(State(gs): State<GatewayState>, body: axum::body::Bytes) -> Response {
    let req: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("Parse error: {e}")}})),
            )
                .into_response();
        }
    };

    let id = req.id.clone();
    let resp = match req.method.as_str() {
        "initialize" => json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": gs.server_name, "version": gs.server_version},
                "instructions":
                    "DCC-MCP Gateway — multi-instance discovery.\n\
                     1. Call list_dcc_instances to see all running DCC servers.\n\
                     2. Call connect_to_dcc to get the MCP URL for a specific DCC type.\n\
                     3. Connect your MCP client directly to that URL for zero-overhead access.\n\
                     4. Or use POST /mcp/{instance_id} on this gateway for transparent proxying."
            }
        }),
        "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
        "notifications/initialized" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
        "tools/list" => {
            json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"tools": gateway_tool_defs(), "nextCursor": null}
            })
        }
        "tools/call" => {
            let tool = req
                .params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let args = req
                .params
                .as_ref()
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or(json!({}));

            let result = match tool {
                "list_dcc_instances" => tool_list_instances(&gs, &args).await,
                "get_dcc_instance" => tool_get_instance(&gs, &args).await,
                "connect_to_dcc" => tool_connect_to_dcc(&gs, &args).await,
                other => Err(format!("Unknown tool: {other}")),
            };

            match result {
                Ok(text) => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"content": [{"type": "text", "text": text}], "isError": false}
                }),
                Err(msg) => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"content": [{"type": "text", "text": msg}], "isError": true}
                }),
            }
        }
        other => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {other}")}
        }),
    };

    let mut response = Json(resp).into_response();
    response
        .headers_mut()
        .insert("Mcp-Session-Id", "dcc-mcp-gateway".parse().unwrap());
    response
}

// ── Proxy handlers ────────────────────────────────────────────────────────────

/// `POST /mcp/{instance_id}` — transparent proxy to a specific DCC instance.
async fn handle_proxy_instance(
    State(gs): State<GatewayState>,
    Path(instance_id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let reg = gs.registry.read().await;
    let entry = reg.list_all().into_iter().find(|e| {
        let eid = e.instance_id.to_string();
        eid == instance_id || eid.starts_with(&instance_id)
    });
    drop(reg);

    match entry {
        Some(e) => {
            let url = format!("http://{}:{}/mcp", e.host, e.port);
            proxy_request(&gs.http_client, &url, headers, body).await
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("Instance '{}' not found", instance_id)})),
        )
            .into_response(),
    }
}

/// `POST /mcp/dcc/{dcc_type}` — proxy to best instance of a DCC type.
async fn handle_proxy_dcc(
    State(gs): State<GatewayState>,
    Path(dcc_type): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let reg = gs.registry.read().await;
    let mut candidates: Vec<ServiceEntry> = gs
        .live_instances(&reg)
        .into_iter()
        .filter(|e| e.dcc_type == dcc_type)
        .collect();
    drop(reg);

    if candidates.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": format!("No live '{}' instances", dcc_type)})),
        )
            .into_response();
    }

    // Prefer Available over Busy
    candidates.sort_by_key(|e| matches!(e.status, ServiceStatus::Busy) as u8);
    let url = format!("http://{}:{}/mcp", candidates[0].host, candidates[0].port);
    proxy_request(&gs.http_client, &url, headers, body).await
}

async fn proxy_request(
    client: &reqwest::Client,
    target_url: &str,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let mut req = client.post(target_url).body(body.to_vec());

    for (key, val) in &headers {
        let name = key.as_str().to_lowercase();
        if matches!(
            name.as_str(),
            "content-type" | "accept" | "mcp-session-id" | "authorization"
        ) {
            if let Ok(v) = val.to_str() {
                req = req.header(key.as_str(), v);
            }
        }
    }

    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let resp_headers = resp.headers().clone();
            let bytes = resp.bytes().await.unwrap_or_default();
            let mut response = Response::new(axum::body::Body::from(bytes));
            *response.status_mut() = status;
            for (k, v) in &resp_headers {
                let n = k.as_str().to_lowercase();
                if n == "content-type" || n.starts_with("mcp-") {
                    response.headers_mut().insert(k, v.clone());
                }
            }
            response
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("Upstream unreachable: {e}")})),
        )
            .into_response(),
    }
}

// ── Discovery meta-tools ──────────────────────────────────────────────────────

async fn tool_list_instances(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let dcc_filter = args.get("dcc_type").and_then(|v| v.as_str());
    let reg = gs.registry.read().await;
    let mut instances: Vec<Value> = gs
        .live_instances(&reg)
        .iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type == f))
        .map(|e| entry_to_json(e, gs.stale_timeout))
        .collect();

    instances.sort_by(|a, b| {
        a["dcc_type"]
            .as_str()
            .cmp(&b["dcc_type"].as_str())
            .then(a["port"].as_u64().cmp(&b["port"].as_u64()))
    });

    let tip = if instances.is_empty() {
        "No live DCC instances. Start dcc-mcp-server for each DCC application."
    } else {
        "Use connect_to_dcc(dcc_type=...) to get the direct MCP URL. \
         Connect your agent directly — no proxy needed."
    };

    serde_json::to_string_pretty(&json!({
        "total": instances.len(),
        "instances": instances,
        "tip": tip
    }))
    .map_err(|e| e.to_string())
}

async fn tool_get_instance(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let all = gs.live_instances(&reg);

    if let Some(id) = args.get("instance_id").and_then(|v| v.as_str()) {
        return all
            .iter()
            .find(|e| {
                let s = e.instance_id.to_string();
                s == id || s.starts_with(id)
            })
            .map(|e| {
                serde_json::to_string_pretty(&entry_to_json(e, gs.stale_timeout))
                    .unwrap_or_default()
            })
            .ok_or_else(|| format!("Instance '{id}' not found"));
    }

    if let Some(dcc) = args.get("dcc_type").and_then(|v| v.as_str()) {
        let candidates: Vec<&ServiceEntry> = all.iter().filter(|e| e.dcc_type == dcc).collect();
        if candidates.is_empty() {
            return Err(format!("No live '{dcc}' instances"));
        }
        let scene = args.get("scene").and_then(|v| v.as_str());
        let entry = scene
            .and_then(|hint| {
                candidates
                    .iter()
                    .find(|e| e.scene.as_deref().unwrap_or("").contains(hint))
            })
            .copied()
            .unwrap_or(candidates[0]);
        return serde_json::to_string_pretty(&entry_to_json(entry, gs.stale_timeout))
            .map_err(|e| e.to_string());
    }

    Err("Provide instance_id or dcc_type".to_string())
}

async fn tool_connect_to_dcc(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let all = gs.live_instances(&reg);

    let entry = if let Some(id) = args.get("instance_id").and_then(|v| v.as_str()) {
        all.iter()
            .find(|e| {
                let s = e.instance_id.to_string();
                s == id || s.starts_with(id)
            })
            .cloned()
            .ok_or_else(|| format!("Instance '{id}' not found"))?
    } else if let Some(dcc) = args.get("dcc_type").and_then(|v| v.as_str()) {
        let candidates: Vec<&ServiceEntry> = all.iter().filter(|e| e.dcc_type == dcc).collect();
        if candidates.is_empty() {
            return Err(format!(
                "No live '{dcc}' instances. Start: dcc-mcp-server --dcc {dcc}"
            ));
        }
        let scene = args.get("scene").and_then(|v| v.as_str());
        let e = scene
            .and_then(|h| {
                candidates
                    .iter()
                    .find(|e| e.scene.as_deref().unwrap_or("").contains(h))
            })
            .copied()
            .unwrap_or(candidates[0]);
        e.clone()
    } else {
        return Err("Provide instance_id or dcc_type".to_string());
    };

    let mcp_url = format!("http://{}:{}/mcp", entry.host, entry.port);
    serde_json::to_string_pretty(&json!({
        "instance_id": entry.instance_id.to_string(),
        "dcc_type": entry.dcc_type,
        "mcp_url": mcp_url,
        "scene": entry.scene,
        "status": entry.status.to_string(),
        "instructions": format!(
            "Point your MCP client to: {mcp_url}\n\
             Direct connection = zero proxy overhead.\n\
             Or use POST /mcp/{id} on this gateway for transparent proxying.",
            id = entry.instance_id
        )
    }))
    .map_err(|e| e.to_string())
}

fn gateway_tool_defs() -> Value {
    json!([
        {
            "name": "list_dcc_instances",
            "description": "List all running DCC server instances. Returns type, port, scene, status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dcc_type": {"type": "string", "description": "Filter by type (e.g. 'maya'). Omit for all."}
                }
            }
        },
        {
            "name": "get_dcc_instance",
            "description": "Get info on a specific DCC instance by id or dcc_type+scene.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "instance_id": {"type": "string", "description": "UUID (or prefix) from list_dcc_instances"},
                    "dcc_type": {"type": "string"},
                    "scene": {"type": "string", "description": "Scene name hint for selection"}
                }
            }
        },
        {
            "name": "connect_to_dcc",
            "description": "Get the direct MCP URL for a DCC instance. Connect to it directly for zero-overhead access.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "instance_id": {"type": "string"},
                    "dcc_type": {"type": "string"},
                    "scene": {"type": "string"}
                }
            }
        }
    ])
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

    // ── Resolve registry dir ──────────────────────────────────────────────

    let registry_dir = args.registry_dir.clone().unwrap_or_else(|| {
        std::env::temp_dir()
            .join("dcc-mcp")
            .to_string_lossy()
            .to_string()
    });

    let registry = FileRegistry::new(&registry_dir)
        .with_context(|| format!("Failed to open FileRegistry at {registry_dir}"))?;
    let registry = Arc::new(RwLock::new(registry));

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

    // ── Register in FileRegistry ──────────────────────────────────────────

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

    let service_key = entry.key();

    {
        let reg = registry.read().await;
        if let Err(e) = reg.register(entry) {
            tracing::warn!("Failed to register in FileRegistry: {e}");
        } else {
            tracing::info!(
                instance = %service_key.instance_id,
                registry = %registry_dir,
                "Registered in FileRegistry"
            );
        }
    }

    // Heartbeat background task
    if args.heartbeat_secs > 0 {
        let reg = registry.clone();
        let key = service_key.clone();
        let interval = args.heartbeat_secs;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(interval));
            loop {
                tick.tick().await;
                let r = reg.read().await;
                let _ = r.heartbeat(&key);
            }
        });
    }

    // ── Try to become the gateway (first-wins competition) ────────────────

    let is_gateway = if args.gateway_port > 0 {
        let gateway_bind = format!("{}:{}", args.host, args.gateway_port);
        match tokio::net::TcpListener::bind(&gateway_bind).await {
            Ok(listener) => {
                tracing::info!("Won gateway port {} — starting gateway", args.gateway_port);

                let stale_timeout = Duration::from_secs(args.stale_timeout_secs);
                let gs = GatewayState {
                    registry: registry.clone(),
                    stale_timeout,
                    server_name: format!("{} (gateway)", args.server_name),
                    server_version: env!("CARGO_PKG_VERSION").to_string(),
                    http_client: reqwest::Client::builder()
                        .timeout(Duration::from_secs(30))
                        .build()
                        .context("Failed to build HTTP client")?,
                };

                // Stale cleanup background task
                {
                    let reg = registry.clone();
                    tokio::spawn(async move {
                        let mut interval = tokio::time::interval(Duration::from_secs(15));
                        loop {
                            interval.tick().await;
                            let r = reg.read().await;
                            match r.cleanup_stale(stale_timeout) {
                                Ok(n) if n > 0 => {
                                    tracing::info!("Gateway: evicted {} stale instance(s)", n)
                                }
                                Err(e) => tracing::warn!("Gateway cleanup error: {e}"),
                                _ => {}
                            }
                        }
                    });
                }

                let router = Router::new()
                    .route("/health", routing::get(handle_health))
                    .route("/instances", routing::get(handle_instances))
                    .route("/mcp", routing::post(handle_gateway_mcp))
                    .route("/mcp/{instance_id}", routing::post(handle_proxy_instance))
                    .route("/mcp/dcc/{dcc_type}", routing::post(handle_proxy_dcc))
                    .with_state(gs)
                    .layer(TraceLayer::new_for_http())
                    .layer(
                        CorsLayer::new()
                            .allow_origin(Any)
                            .allow_methods(Any)
                            .allow_headers(Any),
                    );

                let actual = listener.local_addr()?;
                tracing::info!(
                    "Gateway listening on http://{}  (instances: /instances, mcp: /mcp)",
                    actual
                );

                tokio::spawn(async move {
                    axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            // Keep running until process exits
                            std::future::pending::<()>().await
                        })
                        .await
                        .ok();
                });

                true
            }
            Err(_) => {
                tracing::info!(
                    "Gateway port {} already taken — running as plain DCC instance",
                    args.gateway_port
                );
                false
            }
        }
    } else {
        tracing::debug!("Gateway disabled (gateway_port=0)");
        false
    };

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

    // Deregister from FileRegistry
    {
        let reg = registry.read().await;
        match reg.deregister(&service_key) {
            Ok(_) => tracing::info!("Deregistered from FileRegistry"),
            Err(e) => tracing::warn!("Deregister error: {e}"),
        }
    }

    if is_gateway {
        tracing::info!("Gateway port released");
    }

    handle.shutdown().await;
    Ok(())
}
