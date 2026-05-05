//! Integration tests for the `translate` subcommand bridge logic.
//!
//! Spins up a minimal Python echo stdio MCP server, starts an in-process
//! bridge (same logic as `translate::run`), and verifies the full
//! JSON-RPC round-trip through the HTTP surface.
//!
//! Requires `python` (or `python3`) to be on PATH.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
use dcc_mcp_jsonrpc::{JsonRpcMessage, JsonRpcResponse};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex as TokioMutex, mpsc, oneshot};
use tower_http::cors::CorsLayer;

// ── Echo MCP server (Python script) ──────────────────────────────────────────

const ECHO_SERVER_PY: &str = r#"
import sys, json

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue

    req_id = msg.get("id")
    method = msg.get("method", "")

    if req_id is None:
        continue  # notification, ignore

    if method == "initialize":
        send({"jsonrpc":"2.0","id":req_id,"result":{
            "protocolVersion":"2025-03-26",
            "serverInfo":{"name":"echo-test","version":"0.1.0"},
            "capabilities":{"tools":{}}
        }})
    elif method == "tools/list":
        send({"jsonrpc":"2.0","id":req_id,"result":{"tools":[{
            "name":"echo",
            "description":"Return the input text unchanged",
            "inputSchema":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}
        }]}})
    elif method == "tools/call":
        text = msg.get("params",{}).get("arguments",{}).get("text","")
        send({"jsonrpc":"2.0","id":req_id,"result":{
            "content":[{"type":"text","text":text}],"isError":False
        }})
    else:
        send({"jsonrpc":"2.0","id":req_id,"error":{"code":-32601,"message":f"unknown: {method}"}})
"#;

// ── Minimal in-process bridge (mirrors translate.rs logic) ───────────────────

struct BridgeReq {
    message: JsonRpcMessage,
    resp_tx: Option<oneshot::Sender<JsonRpcResponse>>,
}

#[derive(Clone)]
struct BridgeState {
    tx: mpsc::Sender<BridgeReq>,
}

async fn handle_post(State(state): State<BridgeState>, body: axum::body::Bytes) -> Response {
    // Peek at the raw JSON to distinguish requests (have "id") from notifications (no "id").
    let is_notification = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("id").cloned())
        .is_none();

    if is_notification {
        // Fire-and-forget: parse as notification and forward.
        if let Ok(notif) = serde_json::from_slice::<dcc_mcp_jsonrpc::JsonRpcNotification>(&body) {
            let _ = state
                .tx
                .send(BridgeReq {
                    message: JsonRpcMessage::Notification(notif),
                    resp_tx: None,
                })
                .await;
        }
        return Response::builder()
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .unwrap();
    }

    let msg: JsonRpcMessage = match serde_json::from_slice(&body) {
        Ok(m) => m,
        Err(e) => {
            let err = serde_json::json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":e.to_string()}});
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&err).unwrap()))
                .unwrap();
        }
    };

    match msg {
        JsonRpcMessage::Request(req) => {
            let (tx, rx) = oneshot::channel();
            let _ = state
                .tx
                .send(BridgeReq {
                    message: JsonRpcMessage::Request(req),
                    resp_tx: Some(tx),
                })
                .await;
            match rx.await {
                Ok(resp) => Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&resp).unwrap()))
                    .unwrap(),
                Err(_) => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("bridge closed"))
                    .unwrap(),
            }
        }
        JsonRpcMessage::Notification(notif) => {
            let _ = state
                .tx
                .send(BridgeReq {
                    message: JsonRpcMessage::Notification(notif),
                    resp_tx: None,
                })
                .await;
            Response::builder()
                .status(StatusCode::ACCEPTED)
                .body(Body::empty())
                .unwrap()
        }
        JsonRpcMessage::Response(_) => Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("unexpected response"))
            .unwrap(),
    }
}

/// Start a bridge for the given stdio command; returns the bound HTTP port.
async fn start_bridge(stdio_cmd: &str) -> u16 {
    let (bridge_tx, mut bridge_rx) = mpsc::channel::<BridgeReq>(64);

    let cmd = stdio_cmd.to_string();
    tokio::spawn(async move {
        let mut parts = cmd.split_whitespace();
        let program = parts.next().unwrap_or("python").to_string();
        let args_vec: Vec<String> = parts.map(String::from).collect();

        let mut child = Command::new(&program)
            .args(&args_vec)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("spawn echo server");

        let mut stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout).lines();

        let pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let pending_clone = pending.clone();

        let mut read_task = tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(JsonRpcMessage::Response(resp)) =
                    serde_json::from_str::<JsonRpcMessage>(&line)
                {
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
            }
        });

        loop {
            tokio::select! {
                msg = bridge_rx.recv() => {
                    let Some(req) = msg else { break; };
                    match req.message {
                        JsonRpcMessage::Request(r) => {
                            let id_key = match &r.id {
                                Some(Value::Number(n)) => n.to_string(),
                                Some(Value::String(s)) => s.clone(),
                                _ => continue,
                            };
                            if let Some(resp_tx) = req.resp_tx {
                                pending.lock().await.insert(id_key, resp_tx);
                            }
                            if let Ok(line) = serde_json::to_string(&r) {
                                let _ = stdin.write_all(format!("{line}\n").as_bytes()).await;
                            }
                        }
                        JsonRpcMessage::Notification(n) => {
                            if let Ok(line) = serde_json::to_string(&n) {
                                let _ = stdin.write_all(format!("{line}\n").as_bytes()).await;
                            }
                        }
                        JsonRpcMessage::Response(_) => {}
                    }
                }
                _done = &mut read_task => { break; }
            }
        }
    });

    let state = BridgeState { tx: bridge_tx };
    let router = Router::new()
        .route("/mcp", post(handle_post))
        .with_state(state)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("local_addr").port();

    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });

    // Give the spawned actor a moment to fully initialise.
    tokio::time::sleep(Duration::from_millis(200)).await;
    port
}

/// Send a JSON-RPC request and parse the response.
async fn post_jsonrpc(port: u16, body: serde_json::Value) -> serde_json::Value {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .json(&body)
        .send()
        .await
        .expect("HTTP POST");
    resp.json().await.expect("JSON response")
}

/// Write the echo server Python script to a temp file.
fn write_echo_server_script() -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .suffix(".py")
        .tempfile()
        .expect("tempfile");
    f.write_all(ECHO_SERVER_PY.as_bytes())
        .expect("write script");
    // Flush so child process sees the content immediately.
    f.flush().expect("flush");
    f
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// `tools/list` through the bridge returns the `echo` tool.
#[tokio::test]
async fn test_bridge_tools_list() {
    let script = write_echo_server_script();
    // Use `python` on Windows, `python3` on most Unix.
    let python = if cfg!(windows) { "python" } else { "python3" };
    let stdio_cmd = format!("{python} {}", script.path().display());

    let port = start_bridge(&stdio_cmd).await;

    // MCP requires initialize first.
    let _ = post_jsonrpc(
        port,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "clientInfo": {"name": "test", "version": "0.1"},
                "capabilities": {}
            }
        }),
    )
    .await;

    let resp = post_jsonrpc(
        port,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
    )
    .await;

    assert!(resp.get("error").is_none(), "unexpected error: {resp}");
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 1, "expected one tool, got: {resp}");
    assert_eq!(tools[0]["name"], "echo");
}

/// `tools/call` returns the text argument unchanged.
#[tokio::test]
async fn test_bridge_tool_call_echo() {
    let script = write_echo_server_script();
    let python = if cfg!(windows) { "python" } else { "python3" };
    let stdio_cmd = format!("{python} {}", script.path().display());

    let port = start_bridge(&stdio_cmd).await;

    // Initialize.
    let _ = post_jsonrpc(
        port,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "clientInfo": {"name": "test", "version": "0.1"},
                "capabilities": {}
            }
        }),
    )
    .await;

    let resp = post_jsonrpc(
        port,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "echo",
                "arguments": {"text": "hello stdio bridge"}
            }
        }),
    )
    .await;

    assert!(resp.get("error").is_none(), "unexpected error: {resp}");
    let content = &resp["result"]["content"][0];
    assert_eq!(content["type"], "text");
    assert_eq!(content["text"], "hello stdio bridge");
}

/// Notifications do not panic and return 202 Accepted.
#[tokio::test]
async fn test_bridge_notification_accepted() {
    let script = write_echo_server_script();
    let python = if cfg!(windows) { "python" } else { "python3" };
    let stdio_cmd = format!("{python} {}", script.path().display());

    let port = start_bridge(&stdio_cmd).await;

    let client = reqwest::Client::new();
    let status = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": "42", "reason": "user cancelled"}
        }))
        .send()
        .await
        .expect("POST")
        .status();

    // Keep the script file alive until after we have the response.
    drop(script);

    assert_eq!(status, reqwest::StatusCode::ACCEPTED);
}
