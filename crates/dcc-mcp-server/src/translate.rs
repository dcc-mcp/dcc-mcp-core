//! `translate` subcommand — bridge any stdio MCP server to HTTP/SSE/Streamable-HTTP.
//!
//! ## Usage
//!
//! ```bash
//! # Bridge a single stdio server and register it with the local gateway:
//! dcc-mcp-server translate \
//!     --stdio "uvx mcp-server-git" \
//!     --app-type git \
//!     --expose-streamable-http \
//!     --port 0
//!
//! # Multi-protocol expose:
//! dcc-mcp-server translate \
//!     --stdio "npx @modelcontextprotocol/server-filesystem /tmp" \
//!     --expose-streamable-http \
//!     --expose-sse \
//!     --port 8003
//!
//! # Run without the gateway:
//! dcc-mcp-server translate \
//!     --stdio "uvx mcp-server-filesystem /workspace" \
//!     --no-register \
//!     --port 4444
//! ```
//!
//! ## Architecture
//!
//! ```text
//! HTTP Client
//!    │  POST/GET /mcp  (Streamable HTTP)
//!    │  GET  /sse      (legacy SSE)
//!    ▼
//! axum HTTP server  (this module)
//!    │  tokio channel
//!    ▼
//! StdioBridgeActor  ──stdin──►  child process (stdio MCP server)
//!    ▲                ◄stdout──
//!    │  response routing (request id map)
//!    └─ pending_requests HashMap
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use axum::Router;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use clap::Args;
use dcc_mcp_jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
#[cfg(feature = "gateway-auto")]
use dcc_mcp_transport::discovery::types::ServiceEntry;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc, oneshot};
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

// ── CLI Args ──────────────────────────────────────────────────────────────────

/// Bridge any stdio MCP server to HTTP/SSE/Streamable-HTTP.
#[derive(Debug, Args)]
pub struct TranslateArgs {
    /// Shell command to launch the stdio MCP server (e.g. "uvx mcp-server-git").
    #[arg(long, value_name = "CMD")]
    pub stdio: String,

    /// Application type label for gateway registration (e.g. "git", "filesystem").
    #[arg(long, default_value = "external")]
    pub app_type: String,

    /// Expose MCP Streamable HTTP endpoint at /mcp (POST + SSE upgrade).
    #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
    pub expose_streamable_http: bool,

    /// Also expose legacy SSE endpoint at /sse.
    #[arg(long, default_value = "false", action = clap::ArgAction::Set)]
    pub expose_sse: bool,

    /// HTTP port to bind. 0 = OS-assigned.
    #[arg(long, default_value = "0")]
    pub port: u16,

    /// Host address to bind the HTTP server on.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Skip registration with the local FileRegistry / gateway.
    #[arg(long, default_value = "false")]
    pub no_register: bool,

    /// Restart the child process on exit. Set to false to disable.
    #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
    pub restart_on_exit: bool,

    /// Maximum number of restart attempts (0 = unlimited).
    #[arg(long, default_value = "10")]
    pub max_restarts: u32,

    /// Gateway port for registration competition. 0 disables gateway/admin.
    #[arg(long, env = "DCC_MCP_GATEWAY_PORT", default_value = "9765")]
    pub gateway_port: u16,

    /// Gateway host/interface to bind. Defaults to the HTTP `--host`.
    #[arg(long, env = "DCC_MCP_GATEWAY_HOST")]
    pub gateway_host: Option<String>,

    /// Remote/LAN gateway host/interface to bind.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_HOST", default_value = "0.0.0.0")]
    pub gateway_remote_host: String,

    /// Remote/LAN gateway port. 0 disables the remote listener.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_PORT", default_value = "59765")]
    pub gateway_remote_port: u16,

    /// Disable the read-only Admin UI on the elected gateway.
    #[arg(long, env = "DCC_MCP_NO_ADMIN", default_value = "false")]
    pub no_admin: bool,

    /// URL prefix for the read-only Admin UI.
    #[arg(long, env = "DCC_MCP_ADMIN_PATH", default_value = "/admin")]
    pub admin_path: String,

    /// Directory for the shared FileRegistry.
    #[arg(long, env = "DCC_MCP_REGISTRY_DIR")]
    pub registry_dir: Option<String>,

    /// Stale timeout in seconds for the registry.
    #[arg(long, env = "DCC_MCP_STALE_TIMEOUT", default_value = "30")]
    pub stale_timeout_secs: u64,

    /// Heartbeat interval in seconds.
    #[arg(long, env = "DCC_MCP_HEARTBEAT_INTERVAL", default_value = "5")]
    pub heartbeat_secs: u64,

    /// Seconds to wait for graceful shutdown.
    #[arg(long, default_value = "10")]
    pub shutdown_timeout_secs: u64,

    /// Server name advertised in gateway registration.
    #[arg(long, env = "DCC_MCP_SERVER_NAME")]
    pub server_name: Option<String>,

    /// Path to a PID file (written while the bridge is running).
    #[arg(long, value_name = "PATH")]
    pub pid_file: Option<PathBuf>,

    /// Overwrite an existing PID file even if it points at a live process.
    #[arg(long, default_value = "false")]
    pub force: bool,
}

// ── Bridge message types ──────────────────────────────────────────────────────

/// A request sent from an HTTP handler to the stdio bridge actor.
struct BridgeRequest {
    /// The JSON-RPC request to forward.
    message: JsonRpcMessage,
    /// If this is a request (not a notification), the response goes here.
    response_tx: Option<oneshot::Sender<JsonRpcResponse>>,
}

// ── Stdio bridge actor ────────────────────────────────────────────────────────

/// Manages the child stdio MCP server process and routes requests/responses.
struct StdioBridgeInner {
    /// Channel to send requests to the actor loop.
    tx: mpsc::Sender<BridgeRequest>,
}

/// Shared, clone-able handle to the stdio bridge.
#[derive(Clone)]
struct StdioBridge {
    inner: Arc<StdioBridgeInner>,
}

impl StdioBridge {
    /// Send a JSON-RPC request and wait for the response.
    async fn call(&self, req: JsonRpcRequest) -> anyhow::Result<JsonRpcResponse> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.inner
            .tx
            .send(BridgeRequest {
                message: JsonRpcMessage::Request(req),
                response_tx: Some(resp_tx),
            })
            .await
            .map_err(|_| anyhow::anyhow!("bridge actor has shut down"))?;
        resp_rx
            .await
            .map_err(|_| anyhow::anyhow!("bridge actor dropped response sender"))
    }

    /// Send a JSON-RPC notification (fire and forget).
    async fn notify(&self, notif: JsonRpcNotification) -> anyhow::Result<()> {
        self.inner
            .tx
            .send(BridgeRequest {
                message: JsonRpcMessage::Notification(notif),
                response_tx: None,
            })
            .await
            .map_err(|_| anyhow::anyhow!("bridge actor has shut down"))?;
        Ok(())
    }
}

/// Parse a shell command string into program + args (very basic: splits on whitespace).
fn parse_command(cmd: &str) -> (String, Vec<String>) {
    let mut parts = cmd.split_whitespace();
    let program = parts.next().unwrap_or_default().to_string();
    let args = parts.map(String::from).collect();
    (program, args)
}

/// Spawn the child stdio MCP server and return it.
fn spawn_child(cmd: &str) -> anyhow::Result<Child> {
    let (program, args) = parse_command(cmd);
    let child = Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn stdio MCP server: {cmd}"))?;
    info!("Spawned stdio MCP server: {} {:?}", program, args);
    Ok(child)
}

/// Actor loop: read lines from child stdout and route responses back to callers.
async fn run_bridge_actor(
    cmd: String,
    mut rx: mpsc::Receiver<BridgeRequest>,
    restart_on_exit: bool,
    max_restarts: u32,
) {
    let mut restart_count = 0u32;
    let mut backoff = Duration::from_millis(200);

    'outer: loop {
        let child = match spawn_child(&cmd) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to spawn stdio MCP server: {e}");
                if !restart_on_exit {
                    break;
                }
                if max_restarts > 0 && restart_count >= max_restarts {
                    error!("Max restarts ({max_restarts}) exceeded; bridge actor shutting down");
                    break;
                }
                restart_count += 1;
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
                continue;
            }
        };

        // Reset backoff on successful spawn
        backoff = Duration::from_millis(200);

        let mut stdin: ChildStdin = child.stdin.expect("stdin must be piped");
        let stdout = child.stdout.expect("stdout must be piped");
        let mut reader = BufReader::new(stdout).lines();

        // In-flight request map: request id → response sender
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = pending.clone();

        // Spawn a task to read lines from child stdout and route responses
        let mut read_task = tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                debug!(line = %line, "stdio<");
                match serde_json::from_str::<JsonRpcMessage>(&line) {
                    Ok(JsonRpcMessage::Response(resp)) => {
                        let id_key = match &resp.id {
                            Some(Value::Number(n)) => n.to_string(),
                            Some(Value::String(s)) => s.clone(),
                            _ => continue,
                        };
                        let mut map = pending_clone.lock().await;
                        if let Some(tx) = map.remove(&id_key) {
                            let _ = tx.send(resp);
                        }
                    }
                    Ok(JsonRpcMessage::Notification(notif)) => {
                        debug!(method = %notif.method, "stdio notification from child");
                    }
                    Ok(JsonRpcMessage::Request(_)) => {
                        // Server-initiated requests (elicitation, logging) — ignore for now
                    }
                    Err(e) => {
                        warn!("Failed to parse stdio line as JSON-RPC: {e} | line={line}");
                    }
                }
            }
        });

        // Process requests from HTTP handlers
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    let Some(bridge_req) = msg else {
                        // Channel closed — shut down
                        read_task.abort();
                        break 'outer;
                    };
                    match bridge_req.message {
                        JsonRpcMessage::Request(req) => {
                            let id_key = match &req.id {
                                Some(Value::Number(n)) => n.to_string(),
                                Some(Value::String(s)) => s.clone(),
                                _ => {
                                    warn!("Request missing id; cannot route response");
                                    continue;
                                }
                            };
                            if let Some(resp_tx) = bridge_req.response_tx {
                                pending.lock().await.insert(id_key, resp_tx);
                            }
                            let line = match serde_json::to_string(&req) {
                                Ok(s) => format!("{s}\n"),
                                Err(e) => {
                                    error!("Failed to serialize request: {e}");
                                    continue;
                                }
                            };
                            debug!(line = %line.trim(), "stdio>");
                            if let Err(e) = stdin.write_all(line.as_bytes()).await {
                                error!("Failed to write to child stdin: {e}");
                                read_task.abort();
                                break;
                            }
                        }
                        JsonRpcMessage::Notification(notif) => {
                            let line = match serde_json::to_string(&notif) {
                                Ok(s) => format!("{s}\n"),
                                Err(e) => {
                                    error!("Failed to serialize notification: {e}");
                                    continue;
                                }
                            };
                            debug!(line = %line.trim(), "stdio>");
                            let _ = stdin.write_all(line.as_bytes()).await;
                        }
                        JsonRpcMessage::Response(_) => {
                            warn!("HTTP side sent a Response frame to bridge — ignoring");
                        }
                    }
                }
                result = &mut read_task => {
                    // Child process exited (stdout closed)
                    let _ = result; // ignore JoinHandle result
                    info!("stdio MCP server exited");
                    break;
                }
            }
        }

        if !restart_on_exit {
            info!("restart_on_exit=false; bridge actor shutting down");
            break;
        }
        if max_restarts > 0 && restart_count >= max_restarts {
            error!("Max restarts ({max_restarts}) exceeded; bridge actor shutting down");
            break;
        }
        restart_count += 1;
        warn!("stdio MCP server exited; restarting (attempt {restart_count}) after {backoff:?}");
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

// ── Axum app state ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct BridgeState {
    bridge: StdioBridge,
    expose_sse: bool,
}

// ── HTTP handlers ─────────────────────────────────────────────────────────────

/// POST /mcp — receive a JSON-RPC request, forward to stdio, return response.
/// For streaming clients, this returns a JSON body (simplified Streamable HTTP).
async fn handle_mcp_post(
    State(state): State<BridgeState>,
    _headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // MCP JSON-RPC: messages without an "id" field are notifications (fire-and-forget).
    // We must check this before attempting untagged deserialization because serde
    // may parse a notification as a Request with id=None.
    let is_notification = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("id").cloned())
        .is_none();

    if is_notification {
        if let Ok(notif) = serde_json::from_slice::<JsonRpcNotification>(&body) {
            let _ = state.bridge.notify(notif).await;
        }
        return Response::builder()
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .unwrap();
    }

    let msg: JsonRpcMessage = match serde_json::from_slice(&body) {
        Ok(m) => m,
        Err(e) => {
            let err_body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32700, "message": format!("Parse error: {e}")}
            });
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&err_body).unwrap()))
                .unwrap();
        }
    };

    match msg {
        JsonRpcMessage::Request(req) => match state.bridge.call(req).await {
            Ok(resp) => {
                let body = serde_json::to_vec(&resp).unwrap_or_default();
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap()
            }
            Err(e) => {
                let err_body = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32603, "message": format!("Bridge error: {e}")}
                });
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&err_body).unwrap()))
                    .unwrap()
            }
        },
        JsonRpcMessage::Notification(notif) => {
            let _ = state.bridge.notify(notif).await;
            Response::builder()
                .status(StatusCode::ACCEPTED)
                .body(Body::empty())
                .unwrap()
        }
        JsonRpcMessage::Response(_) => Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("Unexpected response frame"))
            .unwrap(),
    }
}

/// GET /sse — legacy SSE endpoint. Opens an SSE stream; messages from the
/// stdio server are forwarded as SSE events.
///
/// For the legacy SSE transport the client sends a separate POST to `/sse`
/// with the JSON-RPC request and we forward that through the bridge.
async fn handle_sse_post(
    State(state): State<BridgeState>,
    _headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // Reuse streamable HTTP POST logic for legacy SSE POST.
    handle_mcp_post(State(state), _headers, body).await
}

/// GET /health — liveness probe.
async fn handle_health() -> impl axum::response::IntoResponse {
    axum::Json(serde_json::json!({"ok": true, "transport": "stdio-bridge"}))
}

/// GET /mcp — Streamable HTTP GET (SSE upgrade for server-initiated messages).
/// Currently returns a minimal SSE stream that sends a keepalive ping.
async fn handle_mcp_get(
    State(_state): State<BridgeState>,
    _query: Query<HashMap<String, String>>,
) -> Response {
    use tokio_stream::wrappers::ReceiverStream;

    let (tx, rx) = mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(16);

    // Send a keepalive every 15s so the client doesn't time out.
    tokio::spawn(async move {
        loop {
            let ping = b"event: ping\ndata: {}\n\n";
            if tx
                .send(Ok(axum::body::Bytes::from_static(ping)))
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });

    let stream = ReceiverStream::new(rx);
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap()
}

// ── Build the axum router ─────────────────────────────────────────────────────

fn build_router(state: BridgeState) -> Router {
    let mut router = Router::new()
        .route("/mcp", post(handle_mcp_post))
        .route("/mcp", get(handle_mcp_get))
        .route("/health", get(handle_health));

    if state.expose_sse {
        router = router.route("/sse", post(handle_sse_post));
    }

    router.with_state(state).layer(CorsLayer::permissive())
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run the translate bridge. Does not return until a shutdown signal is received.
pub async fn run(args: TranslateArgs) -> anyhow::Result<()> {
    // `server_name` is forwarded into the gateway registration entry,
    // so slim builds without `gateway-auto` never read it.
    #[cfg(feature = "gateway-auto")]
    let server_name = args
        .server_name
        .clone()
        .unwrap_or_else(|| format!("translate-{}", args.app_type));

    // ── Start stdio bridge actor ──────────────────────────────────────────
    let (tx, rx) = mpsc::channel::<BridgeRequest>(128);
    let cmd_clone = args.stdio.clone();
    let restart = args.restart_on_exit;
    let max_restarts = args.max_restarts;
    tokio::spawn(async move {
        run_bridge_actor(cmd_clone, rx, restart, max_restarts).await;
    });

    let bridge = StdioBridge {
        inner: Arc::new(StdioBridgeInner { tx }),
    };

    // ── Start axum HTTP server ────────────────────────────────────────────
    let state = BridgeState {
        bridge,
        expose_sse: args.expose_sse,
    };
    let router = build_router(state);

    let bind_addr: std::net::SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .context("Invalid --host / --port")?;
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .context("Failed to bind HTTP listener")?;
    let bound_addr = listener.local_addr()?;
    let bound_port = bound_addr.port();

    info!(
        "translate bridge listening on http://{}:{}/mcp  (app_type={})",
        args.host, bound_port, args.app_type
    );
    if args.expose_sse {
        info!(
            "Legacy SSE endpoint: http://{}:{}/sse",
            args.host, bound_port
        );
    }

    // ── Gateway registration ──────────────────────────────────────────────
    #[cfg(feature = "gateway-auto")]
    let gw_handle_opt = if !args.no_register {
        use dcc_mcp_gateway::{GatewayConfig, GatewayRunner};

        let registry_dir_path: Option<PathBuf> = args.registry_dir.as_deref().map(PathBuf::from);

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
            server_name: server_name.clone(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            registry_dir: registry_dir_path,
            adapter_dcc: Some(args.app_type.clone()),
            admin_enabled: !args.no_admin,
            admin_path: args.admin_path.clone(),
            ..GatewayConfig::default()
        };

        let runner = GatewayRunner::new(gateway_cfg)
            .map_err(|e| anyhow::anyhow!("Failed to create GatewayRunner: {e}"))?;

        let mut entry = ServiceEntry::new(&args.app_type, &args.host, bound_port);
        entry
            .metadata
            .insert("server_name".to_string(), server_name.clone());
        entry.metadata.insert(
            "mcp_url".to_string(),
            format!("http://{}:{}/mcp", args.host, bound_port),
        );
        entry
            .metadata
            .insert("bridge_type".to_string(), "stdio".to_string());
        entry
            .metadata
            .insert("stdio_command".to_string(), args.stdio.clone());

        let handle = runner
            .start(entry, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start gateway runner: {e}"))?;

        let is_gw = handle.is_gateway;
        if is_gw {
            info!(
                "This translate bridge instance won the gateway port {}",
                args.gateway_port
            );
        } else {
            info!(
                "Registered translate bridge with gateway at port {}",
                args.gateway_port
            );
        }

        Some(handle)
    } else {
        info!("--no-register: skipping gateway registration");
        None
    };
    // Slim builds without `gateway-auto` always behave as `--no-register`:
    // there is no GatewayRunner compiled in. The local HTTP/SSE/Streamable
    // bridge still works; only registration with a gateway is dropped.
    #[cfg(not(feature = "gateway-auto"))]
    let gw_handle_opt: Option<()> = {
        if !args.no_register {
            info!(
                "translate compiled without the `gateway-auto` feature; \
                 skipping gateway registration (--no-register behaviour)"
            );
        } else {
            info!("--no-register: skipping gateway registration");
        }
        None
    };

    // ── PID file ──────────────────────────────────────────────────────────
    let _pid_guard = args
        .pid_file
        .as_deref()
        .map(|path| crate::acquire_pid_file(path, args.force))
        .transpose()?;
    if let Some(path) = args.pid_file.as_deref() {
        crate::spawn_pid_cleanup_watcher(path, std::process::id());
    }

    // ── Serve ─────────────────────────────────────────────────────────────
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let serve_handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let mut rx = shutdown_rx;
                let _ = rx.changed().await;
            })
            .await
            .expect("HTTP server error");
    });

    // Wait for OS shutdown signal
    let reason = crate::select_shutdown_signal().await?;
    info!(
        reason,
        "Shutdown signal received; stopping translate bridge"
    );

    // Drop gateway handle (stops heartbeat). In slim builds this is
    // `Option<()>` and the drop is a no-op; `let _ =` keeps the variable
    // alive until this point without tripping `dropping_copy_types`.
    let _ = gw_handle_opt;

    // Signal HTTP server to stop
    let _ = shutdown_tx.send(true);

    let deadline = Duration::from_secs(args.shutdown_timeout_secs);
    match tokio::time::timeout(deadline, serve_handle).await {
        Ok(_) => info!("Translate bridge shutdown complete"),
        Err(_) => error!(?deadline, "Translate bridge shutdown exceeded deadline"),
    }

    Ok(())
}
