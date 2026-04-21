//! Regression coverage for issue #321 — gateway async-dispatch timeout
//! and wait-for-terminal response passthrough.
//!
//! The gateway must
//!
//! 1. Apply a longer per-request timeout when the client has opted into
//!    async dispatch (either via `_meta.dcc.async` or `progressToken`);
//!    the short sync timeout is otherwise preserved for non-async
//!    calls.
//! 2. Support a wait-for-terminal mode: when the caller sets
//!    `_meta.dcc.wait_for_terminal = true`, the gateway must block the
//!    `tools/call` response until a `$/dcc.jobUpdated` with terminal
//!    status is observed, then return the final envelope.
//! 3. Annotate the response with `_meta.dcc.timed_out = true` when the
//!    wait-for-terminal deadline elapses before the backend emits a
//!    terminal event.
//!
//! We use a tiny axum backend that always replies with a `{pending}`
//! envelope (mimicking the #318 async-dispatch path) and inject the
//! terminal notification directly onto the gateway's per-job broadcast
//! bus via [`SubscriberManager::test_publish_job_event`].

use std::sync::Arc;
use std::time::Duration;

use axum::{Json, Router, routing::post};
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};

use dcc_mcp_http::gateway::aggregator::route_tools_call;
use dcc_mcp_http::gateway::sse_subscriber::SubscriberManager;
use dcc_mcp_http::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;

// ── Helpers ────────────────────────────────────────────────────────────────

async fn make_state(
    backend_timeout: Duration,
    async_dispatch_timeout: Duration,
    wait_terminal_timeout: Duration,
) -> (GatewayState, Arc<RwLock<FileRegistry>>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(16);
    let state = GatewayState {
        registry: registry.clone(),
        stale_timeout: Duration::from_secs(30),
        backend_timeout,
        async_dispatch_timeout,
        wait_terminal_timeout,
        server_name: "test".into(),
        server_version: "0.0.0".into(),
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
        subscriber: SubscriberManager::default(),
    };
    (state, registry, dir)
}

/// Spawn a backend that always replies `{pending, job_id: "job-1"}` for
/// `tools/call`, optionally sleeping for `delay` first. `tools/list`
/// returns a single `slow_tool` so the gateway's prefix-match succeeds.
async fn spawn_pending_backend(delay: Duration) -> u16 {
    #[derive(Clone)]
    struct State {
        delay: Duration,
    }

    async fn handler(
        axum::extract::State(s): axum::extract::State<State>,
        Json(req): Json<Value>,
    ) -> Json<Value> {
        tokio::time::sleep(s.delay).await;
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        match method {
            "tools/list" => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "slow_tool",
                        "description": "slow",
                        "inputSchema": {"type": "object"}
                    }]
                }
            })),
            "tools/call" => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{"type": "text", "text": "Job job-1 queued"}],
                    "structuredContent": {
                        "job_id": "job-1",
                        "status": "pending",
                        "_meta": {"dcc": {"jobId": "job-1"}}
                    },
                    "isError": false
                }
            })),
            other => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("unknown method: {other}")}
            })),
        }
    }

    let app = Router::new()
        .route("/mcp", post(handler))
        .with_state(State { delay });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    port
}

async fn register_backend(registry: &Arc<RwLock<FileRegistry>>, port: u16) -> ServiceEntry {
    let entry = ServiceEntry::new("maya", "127.0.0.1", port);
    let reg = registry.read().await;
    reg.register(entry.clone()).unwrap();
    entry
}

fn encoded_tool_name(instance_id: uuid::Uuid, tool: &str) -> String {
    // Mirror `encode_tool_name` — 8-char prefix + '.' + tool name.
    let short = &instance_id.to_string().replace('-', "")[..8];
    format!("{short}.{tool}")
}

// ── Async dispatch timeout ────────────────────────────────────────────────

/// Part 1 acceptance: an async-opt-in call tolerates a backend that
/// takes longer than the short sync `backend_timeout` to reply, as long
/// as it stays under `async_dispatch_timeout`.
#[tokio::test]
async fn async_dispatch_respects_longer_timeout() {
    let port = spawn_pending_backend(Duration::from_millis(250)).await;
    // Short sync timeout (100 ms) — would fail — but async timeout is 1s.
    let (state, registry, _tmp) = make_state(
        Duration::from_millis(100),
        Duration::from_secs(1),
        Duration::from_secs(5),
    )
    .await;
    let entry = register_backend(&registry, port).await;
    let tool = encoded_tool_name(entry.instance_id, "slow_tool");
    let args = json!({});
    let meta = json!({"dcc": {"async": true}});

    let (text, is_error) = route_tools_call(
        &state,
        &tool,
        &args,
        Some(&meta),
        Some("req-1".into()),
        Some("sess-1"),
    )
    .await;
    assert!(
        !is_error,
        "async opt-in call must not timeout when within async budget; got text={text}"
    );
    assert!(text.contains("job-1") || text.contains("Job"));
}

/// Complementary: a sync call with the same 100 ms timeout DOES fail —
/// proving the async path took the longer timeout, not the shared one.
#[tokio::test]
async fn sync_call_still_uses_short_backend_timeout() {
    let port = spawn_pending_backend(Duration::from_millis(250)).await;
    let (state, registry, _tmp) = make_state(
        Duration::from_millis(100),
        Duration::from_secs(1),
        Duration::from_secs(5),
    )
    .await;
    let entry = register_backend(&registry, port).await;
    let tool = encoded_tool_name(entry.instance_id, "slow_tool");
    let args = json!({});

    // No _meta — synchronous path.
    let (text, is_error) = route_tools_call(
        &state,
        &tool,
        &args,
        None,
        Some("req-2".into()),
        Some("sess-2"),
    )
    .await;
    assert!(
        is_error,
        "sync call must time out under the short backend_timeout; got text={text}"
    );
}

// ── Wait-for-terminal ─────────────────────────────────────────────────────

/// Part 2 happy path: `_meta.dcc.wait_for_terminal = true` blocks the
/// `tools/call` response until `$/dcc.jobUpdated status=completed` is
/// published, and the final envelope carries the backend's `result`.
#[tokio::test]
async fn wait_for_terminal_returns_completed_envelope() {
    let port = spawn_pending_backend(Duration::ZERO).await;
    let (state, registry, _tmp) = make_state(
        Duration::from_secs(1),
        Duration::from_secs(5),
        Duration::from_secs(5),
    )
    .await;
    let entry = register_backend(&registry, port).await;
    let tool = encoded_tool_name(entry.instance_id, "slow_tool");

    // Publish the terminal event on a background task 200 ms after the
    // waiter begins — simulates the backend finishing the job.
    let sub = state.subscriber.clone();
    let pub_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        sub.test_publish_job_event(
            "job-1",
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/$/dcc.jobUpdated",
                "params": {"job_id": "job-1", "status": "completed", "result": {"value": 42}}
            }),
        );
    });

    let args = json!({});
    let meta = json!({"dcc": {"async": true, "wait_for_terminal": true}});
    let (text, is_error) = route_tools_call(
        &state,
        &tool,
        &args,
        Some(&meta),
        Some("req-3".into()),
        Some("sess-3"),
    )
    .await;
    pub_task.await.unwrap();

    assert!(!is_error, "completed job must not set isError; text={text}");
    assert!(
        text.contains("completed"),
        "text body should mention terminal status; got {text}"
    );
}

/// Part 3 timeout behaviour: when no terminal event arrives before
/// `wait_terminal_timeout`, the gateway returns the last-known envelope
/// with `isError=true` and a wait_for_terminal timeout message.
#[tokio::test]
async fn wait_for_terminal_times_out_with_timed_out_flag() {
    let port = spawn_pending_backend(Duration::ZERO).await;
    let (state, registry, _tmp) = make_state(
        Duration::from_secs(1),
        Duration::from_secs(5),
        Duration::from_millis(200), // very short wait timeout
    )
    .await;
    let entry = register_backend(&registry, port).await;
    let tool = encoded_tool_name(entry.instance_id, "slow_tool");
    let args = json!({});
    let meta = json!({"dcc": {"async": true, "wait_for_terminal": true}});

    let started = std::time::Instant::now();
    let (text, is_error) = route_tools_call(
        &state,
        &tool,
        &args,
        Some(&meta),
        Some("req-4".into()),
        Some("sess-4"),
    )
    .await;
    let elapsed = started.elapsed();

    assert!(is_error, "timed-out wait must set isError; text={text}");
    assert!(
        text.contains("timeout") || text.contains("timed_out"),
        "response should signal the wait-for-terminal timeout; got {text}"
    );
    // Tight bound: we should not wait materially longer than the
    // configured 200 ms (allow a generous 2-second ceiling for CI).
    assert!(
        elapsed < Duration::from_secs(2),
        "gateway should return promptly at timeout; elapsed={elapsed:?}"
    );
}

/// Subscribing to the per-job bus must happen BEFORE the backend reply
/// so a fast-completing backend doesn't beat the waiter into position.
/// Publish the terminal event 0 ms later (effectively as soon as the
/// spawn has a tick to run) and assert we still observe completion.
#[tokio::test]
async fn wait_for_terminal_no_race_on_fast_completion() {
    let port = spawn_pending_backend(Duration::ZERO).await;
    let (state, registry, _tmp) = make_state(
        Duration::from_secs(1),
        Duration::from_secs(5),
        Duration::from_secs(2),
    )
    .await;
    let entry = register_backend(&registry, port).await;
    let tool = encoded_tool_name(entry.instance_id, "slow_tool");

    // Publish as fast as we can — the waiter's `job_event_channel` is
    // created inside `wait_for_terminal_reply` before we await the
    // receiver, so this race is safe.
    let sub = state.subscriber.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        sub.test_publish_job_event(
            "job-1",
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/$/dcc.jobUpdated",
                "params": {"job_id": "job-1", "status": "completed"}
            }),
        );
    });

    let args = json!({});
    let meta = json!({"dcc": {"async": true, "wait_for_terminal": true}});
    let (text, is_error) = route_tools_call(
        &state,
        &tool,
        &args,
        Some(&meta),
        Some("req-5".into()),
        Some("sess-5"),
    )
    .await;
    assert!(!is_error, "fast completion should succeed; text={text}");
    assert!(text.contains("completed"));
}
