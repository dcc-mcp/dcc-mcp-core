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

use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};

use dcc_mcp_actions::{ActionDispatcher, ActionMeta, ActionRegistry};
use dcc_mcp_http::gateway::aggregator::{aggregate_tools_list, route_tools_call};
use dcc_mcp_http::gateway::sse_subscriber::SubscriberManager;
use dcc_mcp_http::gateway::state::GatewayState;
use dcc_mcp_http::{McpHttpConfig, McpHttpServer, McpServerHandle};
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
        own_host: "127.0.0.1".into(),
        own_port: 0,
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
        subscriber: SubscriberManager::default(),
        allow_unknown_tools: false,
        adapter_version: None,
        adapter_dcc: None,
        tool_exposure: dcc_mcp_http::gateway::GatewayToolExposure::Full,
        cursor_safe_tool_names: true,
    };
    (state, registry, dir)
}

/// Spawn a backend that always replies `{pending, job_id: "job-1"}` for
/// `tools/call`, optionally sleeping for `delay` first. `tools/list`
/// returns a single `slow_tool` so the gateway's prefix-match succeeds.
async fn spawn_pending_backend(delay: Duration) -> McpServerHandle {
    let registry = Arc::new(ActionRegistry::new());
    registry.register_action(ActionMeta {
        name: "slow_tool".into(),
        description: "slow".into(),
        category: "test".into(),
        version: "1.0.0".into(),
        ..Default::default()
    });
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    dispatcher.register_handler("slow_tool", move |_params| {
        std::thread::sleep(delay);
        Ok(json!({
            "job_id": "job-1",
            "status": "pending",
            "_meta": {"dcc": {"jobId": "job-1"}}
        }))
    });

    McpHttpServer::new(
        registry,
        McpHttpConfig::new(0).with_name("pending-real-backend"),
    )
    .with_dispatcher(dispatcher)
    .start()
    .await
    .expect("real pending backend must start")
}

async fn register_backend(registry: &Arc<RwLock<FileRegistry>>, port: u16) -> ServiceEntry {
    let entry = ServiceEntry::new("maya", "127.0.0.1", port);
    let reg = registry.read().await;
    reg.register(entry.clone()).unwrap();
    entry
}

async fn register_backend_with_dcc(
    registry: &Arc<RwLock<FileRegistry>>,
    port: u16,
    dcc: &str,
) -> ServiceEntry {
    let entry = ServiceEntry::new(dcc, "127.0.0.1", port);
    let reg = registry.read().await;
    reg.register(entry.clone()).unwrap();
    entry
}

async fn spawn_mock_pending_backend(delay: Duration) -> u16 {
    async fn handler(
        axum::extract::State(delay): axum::extract::State<Duration>,
        Json(req): Json<Value>,
    ) -> Json<Value> {
        tokio::time::sleep(delay).await;
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
        .route("/health", get(|| async { "ok" }))
        .route("/mcp", post(handler))
        .with_state(delay);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    port
}

async fn spawn_mock_skill_backend(dcc: &str, skill_name: &str) -> u16 {
    #[derive(Clone)]
    struct State {
        dcc: String,
        skill_name: String,
    }

    async fn handler(
        axum::extract::State(s): axum::extract::State<State>,
        Json(req): Json<Value>,
    ) -> Json<Value> {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        match method {
            "tools/list" => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"tools": []}
            })),
            "tools/call" => {
                let name = req
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match name {
                    "list_skills" | "search_skills" => {
                        let text = serde_json::to_string(&json!({
                            "total": 1,
                            "skills": [{
                                "name": s.skill_name,
                                "description": format!("{} skill", s.dcc),
                                "tools": 1,
                                "loaded": false,
                                "dcc": s.dcc,
                            }]
                        }))
                        .unwrap();
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [{"type": "text", "text": text}],
                                "isError": false
                            }
                        }))
                    }
                    other => Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32601, "message": format!("unknown tool: {other}")}
                    })),
                }
            }
            other => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("unknown method: {other}")}
            })),
        }
    }

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/mcp", post(handler))
        .with_state(State {
            dcc: dcc.to_string(),
            skill_name: skill_name.to_string(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    port
}

fn encoded_tool_name(instance_id: uuid::Uuid, tool: &str) -> String {
    // Mirror `encode_tool_name_cursor_safe` (#656 default). The helper
    // intentionally matches the emitter the default gateway state uses
    // so these tests exercise the wire form real clients see.
    let short = instance_id.to_string().replace('-', "")[..8].to_string();
    let escaped: String = tool
        .bytes()
        .map(|b| match b {
            b'_' => "_U_".to_string(),
            b'.' => "_D_".to_string(),
            b'-' => "_H_".to_string(),
            other if other.is_ascii_alphanumeric() => (other as char).to_string(),
            other => panic!("unexpected byte {other:#04x} in backend tool name {tool:?}"),
        })
        .collect();
    format!("i_{short}__{escaped}")
}

async fn collect_tool_names(state: &GatewayState) -> Vec<String> {
    let mut cursor: Option<String> = None;
    let mut names = Vec::new();
    loop {
        let result = aggregate_tools_list(state, cursor.as_deref()).await;
        names.extend(
            result["tools"]
                .as_array()
                .expect("tools array")
                .iter()
                .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                .map(str::to_string),
        );
        cursor = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            return names;
        }
    }
}

// ── Single-instance bare-name aliases (#583) ───────────────────────────────

#[tokio::test]
async fn single_backend_tools_list_publishes_bare_alias() {
    let backend = spawn_pending_backend(Duration::ZERO).await;
    let (state, registry, _tmp) = make_state(
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .await;
    let entry = register_backend(&registry, backend.port).await;
    let encoded = encoded_tool_name(entry.instance_id, "slow_tool");

    let names = collect_tool_names(&state).await;

    assert!(
        names.contains(&encoded),
        "prefixed tool name missing from tools/list: {names:?}"
    );
    assert!(
        names.contains(&"slow_tool".to_string()),
        "single-instance bare alias missing from tools/list: {names:?}"
    );
}

#[tokio::test]
async fn single_backend_tools_call_accepts_bare_name() {
    let backend = spawn_pending_backend(Duration::ZERO).await;
    let (state, registry, _tmp) = make_state(
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .await;
    register_backend(&registry, backend.port).await;

    let (text, is_error) = route_tools_call(
        &state,
        "slow_tool",
        &json!({}),
        None,
        Some("req-bare".into()),
        Some("sess-bare"),
    )
    .await;

    assert!(!is_error, "bare single-instance call failed: {text}");
    assert!(text.contains("job-1") || text.contains("Job"));
}

#[tokio::test]
async fn multiple_backends_keep_bare_name_ambiguous() {
    let backend_a = spawn_pending_backend(Duration::ZERO).await;
    let backend_b = spawn_pending_backend(Duration::ZERO).await;
    let (state, registry, _tmp) = make_state(
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .await;
    register_backend(&registry, backend_a.port).await;
    register_backend(&registry, backend_b.port).await;

    let result = aggregate_tools_list(&state, None).await;
    let names: Vec<&str> = result["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();
    assert!(
        !names.contains(&"slow_tool"),
        "multi-instance tools/list must not expose ambiguous bare aliases: {names:?}"
    );

    let (text, is_error) = route_tools_call(
        &state,
        "slow_tool",
        &json!({}),
        None,
        Some("req-ambiguous".into()),
        Some("sess-ambiguous"),
    )
    .await;

    assert!(is_error, "ambiguous bare call must fail; text={text}");
    assert!(text.contains("Unknown tool"));
}

// ── Flat skill-management aggregation (#582) ───────────────────────────────

#[tokio::test]
async fn search_skills_returns_flat_gateway_skill_list() {
    let maya_port = spawn_mock_skill_backend("maya", "maya-python").await;
    let blender_port = spawn_mock_skill_backend("blender", "blender-python").await;
    let (state, registry, _tmp) = make_state(
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .await;
    register_backend_with_dcc(&registry, maya_port, "maya").await;
    register_backend_with_dcc(&registry, blender_port, "blender").await;

    let (text, is_error) = route_tools_call(
        &state,
        "search_skills",
        &json!({"query": "python"}),
        None,
        Some("req-search-skills".into()),
        Some("sess-search-skills"),
    )
    .await;

    assert!(!is_error, "search_skills fan-out failed: {text}");
    let result: Value = serde_json::from_str(&text).expect("flat JSON payload");
    let skills = result["skills"].as_array().expect("skills array");
    assert_eq!(result["total"], 2);
    assert_eq!(skills.len(), 2);
    assert!(skills.iter().any(|skill| skill["name"] == "maya-python"));
    assert!(skills.iter().any(|skill| skill["name"] == "blender-python"));
    for skill in skills {
        assert!(skill["_instance_id"].as_str().is_some());
        assert!(skill["_instance_short"].as_str().is_some());
        assert!(skill["_dcc_type"].as_str().is_some());
    }

    let instances = result["instances"].as_array().expect("instances array");
    assert_eq!(instances.len(), 2);
    assert!(instances.iter().all(|inst| inst["skill_count"] == 1));
}

#[tokio::test]
async fn list_skills_returns_flat_gateway_skill_list() {
    let port = spawn_mock_skill_backend("maya", "maya-modeling").await;
    let (state, registry, _tmp) = make_state(
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .await;
    register_backend_with_dcc(&registry, port, "maya").await;

    let (text, is_error) = route_tools_call(
        &state,
        "list_skills",
        &json!({}),
        None,
        Some("req-list-skills".into()),
        Some("sess-list-skills"),
    )
    .await;

    assert!(!is_error, "list_skills fan-out failed: {text}");
    let result: Value = serde_json::from_str(&text).expect("flat JSON payload");
    let skills = result["skills"].as_array().expect("skills array");
    assert_eq!(result["total"], 1);
    assert_eq!(skills[0]["name"], "maya-modeling");
    assert_eq!(skills[0]["_dcc_type"], "maya");
    assert_eq!(result["instances"][0]["skill_count"], 1);
}

// ── Async dispatch timeout ────────────────────────────────────────────────

/// Part 1 acceptance: an async-opt-in call tolerates a backend that
/// takes longer than the short sync `backend_timeout` to reply, as long
/// as it stays under `async_dispatch_timeout`.
#[tokio::test]
async fn async_dispatch_respects_longer_timeout() {
    let backend = spawn_pending_backend(Duration::from_millis(250)).await;
    // Short sync timeout (100 ms) — would fail — but async timeout is 1s.
    let (state, registry, _tmp) = make_state(
        Duration::from_millis(100),
        Duration::from_secs(1),
        Duration::from_secs(5),
    )
    .await;
    let entry = register_backend(&registry, backend.port).await;
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
    let backend = spawn_pending_backend(Duration::from_millis(250)).await;
    let (state, registry, _tmp) = make_state(
        Duration::from_millis(100),
        Duration::from_secs(1),
        Duration::from_secs(5),
    )
    .await;
    let entry = register_backend(&registry, backend.port).await;
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
    let port = spawn_mock_pending_backend(Duration::ZERO).await;
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
    let port = spawn_mock_pending_backend(Duration::ZERO).await;
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
/// Publish the terminal event once the gateway has subscribed and
/// assert we still observe completion.
#[tokio::test]
async fn wait_for_terminal_no_race_on_fast_completion() {
    let port = spawn_mock_pending_backend(Duration::ZERO).await;
    let (state, registry, _tmp) = make_state(
        Duration::from_secs(1),
        Duration::from_secs(5),
        Duration::from_secs(2),
    )
    .await;
    let entry = register_backend(&registry, port).await;
    let tool = encoded_tool_name(entry.instance_id, "slow_tool");

    // Poll until the gateway's `wait_for_terminal_reply` has called
    // `job_event_channel("job-1")` and registered its receiver, then
    // publish. This deliberately avoids timing assumptions so the
    // test is robust under heavy instrumentation (e.g. tarpaulin) and
    // on loaded CI runners where a fixed 50 ms sleep was occasionally
    // losing the race with the backend RTT.
    let sub = state.subscriber.clone();
    let publisher = tokio::spawn(async move {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            if sub.job_bus_receiver_count("job-1") >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
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
    let _ = publisher.await;
    assert!(!is_error, "fast completion should succeed; text={text}");
    assert!(text.contains("completed"));
}
