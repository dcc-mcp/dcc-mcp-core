//! End-to-end gateway prompts aggregation over real TCP (issue #731).
//!
//! The in-crate unit tests in `dcc-mcp-gateway` exercise the aggregator
//! handlers against an axum `Router` wrapped around a [`GatewayState`]
//! — sufficient for the JSON-RPC contract, but the SSE push path
//! (`notifications/prompts/list_changed`) lives in a separate tokio
//! task (`prompts_watcher_handle` in `tasks.rs`) and can only be proven
//! by a client that actually subscribes to the gateway's `GET /mcp`
//! SSE stream over the wire.
//!
//! This module fills that gap:
//!
//! 1. Spawn fake MCP backends on real TCP sockets — each with its own
//!    `prompts/list` response — and register them in a shared
//!    [`FileRegistry`] dir.
//! 2. Spin up a real gateway via [`GatewayRunner::start`] (its `Won
//!    gateway election` branch runs the full watcher pipeline).
//! 3. Drive `initialize` / `prompts/list` / `prompts/get` from a
//!    [`reqwest::Client`] — the same HTTP surface any external MCP
//!    client would see.
//! 4. Subscribe to the gateway's SSE stream, mutate a backend's
//!    advertised prompt, and assert
//!    `notifications/prompts/list_changed` arrives within the watcher's
//!    3 s cadence budget.
//!
//! The module guards against regressions that in-process unit tests
//! can't see: a broken broadcast channel wiring, a missing watcher
//! spawn, or an SSE endpoint that drops the prompts notifications on
//! the floor.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    Json, Router,
    routing::{get, post},
};
use futures::StreamExt;
use serde_json::{Value, json};

use dcc_mcp_http::gateway::{GatewayConfig, GatewayRunner};
use dcc_mcp_transport::discovery::types::ServiceEntry;

// ── Fake backend ────────────────────────────────────────────────────────────

/// State shared with the axum handler so a test can mutate the
/// advertised prompt set mid-flight (for the SSE watcher test).
#[derive(Clone)]
struct FakeBackendState {
    prompt_name: Arc<Mutex<&'static str>>,
    /// Flips to `true` the first time `prompts/get` is observed — lets
    /// the routing test assert the request landed on the right backend.
    get_hit: Arc<AtomicBool>,
    /// Deterministic echo marker embedded in the rendered prompt text.
    echo_marker: &'static str,
}

/// Spawn a fake MCP backend that answers `prompts/list` and `prompts/get`.
///
/// Returns the listening port and the shared state so the caller can
/// swap the prompt name during the test.
async fn spawn_fake_prompts_backend(
    initial_prompt: &'static str,
    echo_marker: &'static str,
) -> (u16, FakeBackendState) {
    let state = FakeBackendState {
        prompt_name: Arc::new(Mutex::new(initial_prompt)),
        get_hit: Arc::new(AtomicBool::new(false)),
        echo_marker,
    };

    async fn handler(
        axum::extract::State(state): axum::extract::State<FakeBackendState>,
        Json(req): Json<Value>,
    ) -> Json<Value> {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let result: Value = match method {
            "tools/list" => json!({"tools": []}),
            "prompts/list" => {
                let name = *state.prompt_name.lock().unwrap();
                json!({
                    "prompts": [{
                        "name": name,
                        "description": format!("prompt from {}", state.echo_marker),
                        "arguments": [],
                    }]
                })
            }
            "prompts/get" => {
                state.get_hit.store(true, Ordering::SeqCst);
                let requested = req
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_string();
                json!({
                    "description": format!("{}:{}", state.echo_marker, requested),
                    "messages": [{
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!("{}:{}", state.echo_marker, requested),
                        }
                    }]
                })
            }
            _ => {
                return Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("unknown method: {method}")}
                }));
            }
        };
        Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/mcp", post(handler))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(40)).await;
    (port, state)
}

// ── Gateway bootstrap ───────────────────────────────────────────────────────

/// Pick an unused TCP port on 127.0.0.1.
///
/// Bind + close pattern — there is a small race window, but the gateway
/// immediately tries to re-bind inside `start()` and the test runs
/// serially per module.
fn pick_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Start a full [`GatewayRunner`] winner on `gw_port`.
///
/// The runner creates its own [`FileRegistry`] directory inside
/// `registry_dir`, so callers must pre-populate the backend rows in
/// that directory *before* calling this helper — the gateway's startup
/// port probe will otherwise evict rows whose TCP socket is closed.
///
/// Returns the [`GatewayHandle`] (must be kept alive for the duration
/// of the test to keep the gateway port bound) and the `http://…/mcp`
/// URL callers hit.
async fn start_gateway_winner(
    registry_dir: &std::path::Path,
    gw_port: u16,
) -> (dcc_mcp_http::gateway::GatewayHandle, String) {
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: gw_port,
        heartbeat_secs: 1,
        registry_dir: Some(registry_dir.to_path_buf()),
        ..GatewayConfig::default()
    };
    let runner = GatewayRunner::new(cfg).expect("GatewayRunner::new");

    // The runner still requires an instance row of its own so its
    // sentinel-election + heartbeat bookkeeping has a plain-instance
    // target. Use a filler `maya` row on port 0 — the gateway will
    // not fan-out to it because its port is unreachable.
    let own_entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    let handle = runner.start(own_entry, None).await.expect("runner.start");
    assert!(
        handle.is_gateway,
        "test harness must win gateway election on the dedicated port"
    );

    (handle, format!("http://127.0.0.1:{gw_port}/mcp"))
}

async fn register_backend_async(
    registry_dir: &std::path::Path,
    dcc: &str,
    port: u16,
) -> ServiceEntry {
    let reg = dcc_mcp_transport::discovery::file_registry::FileRegistry::new(registry_dir).unwrap();
    let entry = ServiceEntry::new(dcc, "127.0.0.1", port);
    let out = entry.clone();
    reg.register(entry).unwrap();
    out
}

// ── JSON-RPC client helpers ─────────────────────────────────────────────────

async fn post_json(client: &reqwest::Client, url: &str, body: Value) -> Value {
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(body.to_string())
        .send()
        .await
        .expect("POST failed");
    resp.json().await.expect("JSON decode failed")
}

// ── Tests ───────────────────────────────────────────────────────────────────

/// Empty-cluster contract: no live backends → `prompts/list` is
/// `{"prompts": []}`, never the historic `-32601` Method-not-found.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_empty_gateway_prompts_list_is_empty_array() {
    let dir = tempfile::tempdir().unwrap();
    let gw_port = pick_free_port();

    let (_handle, gw_url) = start_gateway_winner(dir.path(), gw_port).await;

    // Give the gateway a moment to finish its self-probe.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let client = reqwest::Client::new();
    let resp = post_json(
        &client,
        &gw_url,
        json!({"jsonrpc": "2.0", "id": 1, "method": "prompts/list"}),
    )
    .await;
    assert!(resp.get("error").is_none(), "must not be an error: {resp}");
    assert_eq!(resp["result"]["prompts"], json!([]));

    // initialize must advertise prompts.listChanged=true even when the
    // cluster is empty.
    let init = post_json(
        &client,
        &gw_url,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "initialize",
            "params": {"protocolVersion": "2025-03-26"}
        }),
    )
    .await;
    assert_eq!(
        init["result"]["capabilities"]["prompts"]["listChanged"],
        json!(true),
    );
}

/// Two backends, disjoint prompts → merged list, correct cursor-safe
/// prefixes, and `prompts/get` routes to the owning backend's echo.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn e2e_merges_and_routes_across_real_backends() {
    let dir = tempfile::tempdir().unwrap();
    let gw_port = pick_free_port();

    // Register the two fake backends in the shared registry dir BEFORE
    // the gateway starts so its startup port probe keeps both rows.
    let (port_a, _state_a) = spawn_fake_prompts_backend("bake_animation", "maya-A").await;
    let (port_b, state_b) = spawn_fake_prompts_backend("export_gltf", "blender-B").await;
    let entry_a = register_backend_async(dir.path(), "maya", port_a).await;
    let entry_b = register_backend_async(dir.path(), "blender", port_b).await;

    let (_handle, gw_url) = start_gateway_winner(dir.path(), gw_port).await;

    // Let the gateway's instance watcher + tools watcher see both
    // backend rows before we query.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let client = reqwest::Client::new();
    let resp = post_json(
        &client,
        &gw_url,
        json!({"jsonrpc": "2.0", "id": 1, "method": "prompts/list"}),
    )
    .await;
    assert!(resp.get("error").is_none(), "unexpected error: {resp}");
    let prompts = resp["result"]["prompts"].as_array().unwrap();
    let names: Vec<String> = prompts
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect();

    fn short(iid: &uuid::Uuid) -> String {
        let mut s = iid.to_string().replace('-', "");
        s.truncate(8);
        s
    }
    let expect_a = format!("i_{}__bake_U_animation", short(&entry_a.instance_id));
    let expect_b = format!("i_{}__export_U_gltf", short(&entry_b.instance_id));
    assert!(names.contains(&expect_a), "missing {expect_a}: {names:?}");
    assert!(names.contains(&expect_b), "missing {expect_b}: {names:?}");

    // prompts/get against backend B: the response text must carry
    // B's echo marker *and* the decoded bare name, proving the
    // gateway decoded the prefix + reached the right backend.
    let get_resp = post_json(
        &client,
        &gw_url,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "prompts/get",
            "params": {"name": expect_b}
        }),
    )
    .await;
    assert!(
        get_resp.get("error").is_none(),
        "prompts/get error: {get_resp}"
    );
    let text = get_resp["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("text content");
    assert_eq!(text, "blender-B:export_gltf");
    assert!(
        state_b.get_hit.load(Ordering::SeqCst),
        "backend B must have received the prompts/get"
    );
}

/// Watcher SSE contract: subscribe to the gateway's SSE stream, mutate
/// a backend's prompt set, then assert `notifications/prompts/list_changed`
/// arrives within the watcher's 3-second cadence + a small slack.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn e2e_prompts_list_changed_fires_on_backend_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let gw_port = pick_free_port();

    let (port_a, state_a) = spawn_fake_prompts_backend("bake_animation", "maya-A").await;
    register_backend_async(dir.path(), "maya", port_a).await;

    let (_handle, gw_url) = start_gateway_winner(dir.path(), gw_port).await;

    // Open a real SSE subscription — reqwest's bytes_stream keeps the
    // stream open without the per-readline timeout foot-gun that
    // Python's urllib imposes, so this test's assertion can wait as
    // long as the watcher needs without special socket config.
    let client = reqwest::Client::builder().build().expect("reqwest client");
    let sse_resp = client
        .get(&gw_url)
        .header("Accept", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .send()
        .await
        .expect("SSE GET failed");
    assert!(
        sse_resp.status().is_success(),
        "SSE not accepted: {:?}",
        sse_resp.status()
    );

    // Collect SSE frames in a background task so the main thread can
    // continue to drive the mutation.
    let (notif_tx, mut notif_rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        let mut stream = sse_resp.bytes_stream();
        let mut buf = String::new();
        while let Some(Ok(bytes)) = stream.next().await {
            buf.push_str(&String::from_utf8_lossy(&bytes));
            // Split on blank-line SSE frame boundaries.
            while let Some(idx) = buf.find("\n\n") {
                let frame = buf[..idx].to_string();
                buf.drain(..idx + 2);
                // Concatenate every `data:` line in the frame.
                let mut data = String::new();
                for line in frame.lines() {
                    if let Some(rest) = line.strip_prefix("data:") {
                        if !data.is_empty() {
                            data.push('\n');
                        }
                        data.push_str(rest.trim_start());
                    }
                }
                if !data.is_empty()
                    && let Ok(val) = serde_json::from_str::<Value>(&data)
                {
                    let _ = notif_tx.send(val);
                }
            }
        }
    });

    // Let the watcher commit its baseline fingerprint (one tick + slack).
    tokio::time::sleep(Duration::from_millis(3_500)).await;

    // Mutate the backend's advertised prompt set. Next watcher tick
    // (≤ 3 s away) must observe a different fingerprint and broadcast.
    *state_a.prompt_name.lock().unwrap() = "render_preview";

    // Drain events looking for the prompts/list_changed one —
    // resources/list_changed and tools/list_changed share the channel,
    // so we must keep reading until the prompts one surfaces (or the
    // budget expires).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    let mut saw_prompts_changed = false;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let ev = match tokio::time::timeout(remaining, notif_rx.recv()).await {
            Ok(Some(v)) => v,
            Ok(None) | Err(_) => break,
        };
        if ev.get("method").and_then(Value::as_str) == Some("notifications/prompts/list_changed") {
            saw_prompts_changed = true;
            break;
        }
    }

    assert!(
        saw_prompts_changed,
        "gateway did not broadcast notifications/prompts/list_changed within 12s",
    );

    // Final sanity: a fresh prompts/list must show the mutated name.
    let resp = post_json(
        &client,
        &gw_url,
        json!({"jsonrpc": "2.0", "id": 99, "method": "prompts/list"}),
    )
    .await;
    let names: Vec<String> = resp["result"]["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect();
    assert!(
        names.iter().any(|n| n.contains("render_U_preview")),
        "post-mutation prompts/list missing render_preview: {names:?}",
    );
}
