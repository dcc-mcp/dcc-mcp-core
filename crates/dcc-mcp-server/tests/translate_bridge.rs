//! Integration tests for the `translate` subcommand bridge logic.
//!
//! Spins up minimal Python stdio MCP servers, starts an in-process bridge, and
//! verifies the JSON-RPC round-trip through the HTTP surface.
//!
//! The bridge below is a *mirror* of `translate.rs`, not a copy: it keeps only
//! the parts of the production contract that are observable over HTTP, which
//! are exactly the two this file asserts —
//!
//! * **drop-and-continue**: a response whose id matches no request in flight is
//!   dropped and the child keeps being read. Production does *not* fail the
//!   other calls or end the read loop; a mirror that clears the pending map and
//!   breaks out would assert a contract production does not implement.
//! * **a bounded wait**: a child that never answers the current id fails the
//!   call after a fixed bound instead of pinning the HTTP request open. The
//!   mirror uses a short bound so the suite stays fast; production reads its
//!   bound from `--bridge-timeout-secs` (`DCC_MCP_BRIDGE_TIMEOUT_SECS`,
//!   default 600s).
//!
//! Behaviour that is not observable from an HTTP client — id normalization,
//! duplicate-id rejection, timeout entry release — is covered by the unit tests
//! in `src/translate/correlation_tests.rs`, which drive the real bridge.
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

/// A desynchronized stdio server: on every request after the first it emits a
/// response carrying the *previous* request's id. It never answers the current
/// id, so the bridge must fail the call closed instead of delivering a payload
/// that belongs to an earlier call.
const SKEWED_SERVER_PY: &str = r#"
import sys, json

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

previous_id = None
for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue

    req_id = msg.get("id")
    if req_id is None:
        continue

    if previous_id is None:
        # First request: answer correctly so the caller sees a healthy start.
        send({"jsonrpc":"2.0","id":req_id,"result":{
            "content":[{"type":"text","text":"payload-for-{}".format(req_id)}],"isError":False
        }})
    else:
        # Later requests: emit the previous request's payload under its id.
        send({"jsonrpc":"2.0","id":previous_id,"result":{
            "content":[{"type":"text","text":"stale-payload-for-{}".format(previous_id)}],"isError":False
        }})
    previous_id = req_id
"#;

/// A stdio server that emits one stray response — an id no caller issued — and
/// then serves every request correctly. Used to prove the bridge drops the
/// stray response instead of stranding the child.
const STRAY_THEN_HEALTHY_SERVER_PY: &str = r#"
import sys, json

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

send({"jsonrpc":"2.0","id":999,"result":{"stray":True}})

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
        continue

    if method == "tools/call":
        text = msg.get("params",{}).get("arguments",{}).get("text","")
        send({"jsonrpc":"2.0","id":req_id,"result":{
            "content":[{"type":"text","text":text}],"isError":False
        }})
    else:
        send({"jsonrpc":"2.0","id":req_id,"result":{}})
"#;

// ── Minimal in-process bridge (mirrors translate.rs logic) ───────────────────

/// The mirror's wait bound. Production defaults to 600s; the mirror only needs
/// a bound short enough to keep the suite fast.
const MIRROR_BRIDGE_TIMEOUT: Duration = Duration::from_secs(2);

struct BridgeReq {
    message: JsonRpcMessage,
    resp_tx: Option<oneshot::Sender<JsonRpcResponse>>,
}

#[derive(Clone)]
struct BridgeState {
    tx: mpsc::Sender<BridgeReq>,
}

/// Correlation key for a JSON-RPC id, mirroring `translate.rs`.
///
/// Integers and exactly-integral floats share one spelling, so a child that
/// echoes id `2` as `2.0` still correlates.
fn response_id_key(id: &Option<Value>) -> Option<String> {
    match id {
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                return Some(i.to_string());
            }
            match n.as_f64() {
                Some(f) if f.is_finite() && f.fract() == 0.0 => Some(format!("{f:.0}")),
                _ => Some(n.to_string()),
            }
        }
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
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
            let expected_id = match response_id_key(&req.id) {
                Some(id) => id,
                None => {
                    return Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::from("request missing id"))
                        .unwrap();
                }
            };
            let (tx, rx) = oneshot::channel();
            let _ = state
                .tx
                .send(BridgeReq {
                    message: JsonRpcMessage::Request(req),
                    resp_tx: Some(tx),
                })
                .await;
            // Bounded wait: a child that never answers the current id must not
            // pin the HTTP request open (mirrors translate.rs).
            let awaited = tokio::time::timeout(MIRROR_BRIDGE_TIMEOUT, rx).await;
            let resp = match awaited {
                Ok(Ok(resp)) => resp,
                Ok(Err(_)) => {
                    let err = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": {"code": -32603, "message": format!(
                            "transport desync: no response for request id {expected_id:?}; the bridge dropped the channel"
                        )}
                    });
                    return Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&err).unwrap()))
                        .unwrap();
                }
                Err(_) => {
                    let err = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": {"code": -32603, "message": format!(
                            "bridge timed out after {MIRROR_BRIDGE_TIMEOUT:?} waiting for response id {expected_id:?}"
                        )}
                    });
                    return Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&err).unwrap()))
                        .unwrap();
                }
            };
            // Defence in depth: responses are routed by id, so a payload can
            // only reach the caller that filed that id.
            if response_id_key(&resp.id).as_deref() != Some(expected_id.as_str()) {
                let err = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32603, "message": format!(
                        "transport desync: expected JSON-RPC response id {expected_id:?}, got {:?}",
                        response_id_key(&resp.id)
                    )}
                });
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&err).unwrap()))
                    .unwrap();
            }
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&resp).unwrap()))
                .unwrap()
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
                    let id_key = response_id_key(&resp.id);
                    let mut map = pending_clone.lock().await;
                    if let Some(tx) = id_key.as_ref().and_then(|key| map.remove(key)) {
                        let _ = tx.send(resp);
                    } else {
                        // Unattributable response: drop it and keep reading,
                        // exactly as `translate.rs` does. Failing the calls in
                        // flight or ending the read loop here would strand a
                        // child that is otherwise healthy; the assertion that
                        // catches that is
                        // `test_bridge_stray_response_does_not_strand_later_calls`.
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
                            let Some(id_key) = response_id_key(&r.id) else {
                                continue;
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

/// Write a Python stdio server script to a temp file.
fn write_server_script(source: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .suffix(".py")
        .tempfile()
        .expect("tempfile");
    f.write_all(source.as_bytes()).expect("write script");
    // Flush so child process sees the content immediately.
    f.flush().expect("flush");
    f
}

/// Write the echo server Python script to a temp file.
fn write_echo_server_script() -> tempfile::NamedTempFile {
    write_server_script(ECHO_SERVER_PY)
}

/// The `python` (Windows) / `python3` (Unix) interpreter name.
fn python_program() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// `tools/list` through the bridge returns the `echo` tool.
#[tokio::test]
async fn test_bridge_tools_list() {
    let script = write_echo_server_script();
    let stdio_cmd = format!("{} {}", python_program(), script.path().display());

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
    let stdio_cmd = format!("{} {}", python_program(), script.path().display());

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
    let stdio_cmd = format!("{} {}", python_program(), script.path().display());

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

/// A stdio server that answers with the *previous* request's id must never have
/// that payload delivered to the current caller.
///
/// The production contract is fail-closed and bounded: the stale response
/// matches no request in flight, so it is dropped and the call fails when the
/// wait bound expires. It is *not* an instant `transport desync` — a child this
/// broken is not answered at all, and production reports the bounded wait
/// (`--bridge-timeout-secs`, default 600s) rather than inventing a desync. This
/// test asserts the fail-closed half of that contract; the bound itself is
/// mirrored here as [`MIRROR_BRIDGE_TIMEOUT`].
#[tokio::test]
async fn test_bridge_rejects_stale_response_id() {
    let script = write_server_script(SKEWED_SERVER_PY);
    let stdio_cmd = format!("{} {}", python_program(), script.path().display());

    let port = start_bridge(&stdio_cmd).await;

    // First call: the skewed child echoes the current id (there is no previous
    // id yet), so this call is correctly correlated and must succeed.
    let first = post_jsonrpc(
        port,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "echo", "arguments": {"text": "first"}}
        }),
    )
    .await;

    // Second call: the child answers with id `1` (the previous request), so the
    // bridge must refuse to deliver that payload under request id `2`.
    let client = reqwest::Client::new();
    let started = std::time::Instant::now();
    let second = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "echo", "arguments": {"text": "second"}}
        }))
        .send()
        .await
        .expect("HTTP POST");
    let elapsed = started.elapsed();
    let status = second.status();
    let body: Value = second.json().await.expect("error body JSON");

    // The first call is correlated: its own payload comes back.
    assert!(
        first.get("error").is_none(),
        "first call should be correlated, got: {first}"
    );
    assert_eq!(
        first["result"]["content"][0]["text"], "payload-for-1",
        "first call must receive its own payload, got: {first}"
    );

    // The second call must fail closed rather than return the first payload.
    assert!(
        !status.is_success(),
        "desynchronized second call must not succeed, got: {status}"
    );
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("timed out") || message.contains("transport desync"),
        "expected a bounded-wait failure, got: {body}"
    );
    // The wait is bounded, so the failure cannot hang past the mirror's bound.
    assert!(
        elapsed < MIRROR_BRIDGE_TIMEOUT + Duration::from_secs(5),
        "the wait must be bounded, took {elapsed:?}"
    );
    assert!(
        !serde_json::to_string(&body)
            .expect("serialize body")
            .contains("payload-for-1"),
        "the stale payload must never reach the caller, got: {body}"
    );
}

/// A stray response must not strand the child.
///
/// This is the regression guard for the drop-and-continue half of the contract:
/// a mirror (or an implementation) that clears the pending map and stops
/// reading when it sees an unattributable response desynchronizes a child that
/// is serving every request correctly, and every later call then stalls for the
/// full wait bound.
#[tokio::test]
async fn test_bridge_stray_response_does_not_strand_later_calls() {
    let script = write_server_script(STRAY_THEN_HEALTHY_SERVER_PY);
    let stdio_cmd = format!("{} {}", python_program(), script.path().display());

    let port = start_bridge(&stdio_cmd).await;

    // The child emits id 999 once, at startup. No call issued it, so dropping
    // it is enough: the child must keep serving every request correctly.
    for (id, text) in [(1, "alpha"), (2, "bravo"), (3, "charlie")] {
        let response = post_jsonrpc(
            port,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": "echo", "arguments": {"text": text}}
            }),
        )
        .await;

        assert!(
            response.get("error").is_none(),
            "call {id} must still be served after a stray response, got: {response}"
        );
        assert_eq!(
            response["result"]["content"][0]["text"], text,
            "call {id} must receive its own payload, got: {response}"
        );
    }
}
