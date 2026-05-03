//! Tests for gateway pass-through functionality (async dispatch, skill aggregation).
//!
//! NOTE: Tests for direct backend tool routing (old behavior before #674)
//! have been removed. The gateway now uses `search_tools` → `describe_tool`
//! → `call_tool` workflow.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};

use dcc_mcp_actions::{ActionDispatcher, ActionMeta, ActionRegistry};
use dcc_mcp_http::gateway::aggregator::route_tools_call;
use dcc_mcp_http::gateway::sse_subscriber::SubscriberManager;
use dcc_mcp_http::gateway::state::GatewayState;
use dcc_mcp_http::{McpHttpConfig, McpHttpServer, McpServerHandle};
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;

// ── Helpers ────────────────────────────────────────────────────────

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
        tool_exposure: dcc_mcp_http::gateway::GatewayToolExposure::Rest,
        cursor_safe_tool_names: true,
        capability_index: Arc::new(dcc_mcp_http::gateway::capability::CapabilityIndex::new()),
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
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    port
}

fn encoded_tool_name(instance_id: uuid::Uuid, tool: &str) -> String {
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

// ── Flat skill-management aggregation (#582) ─────────────────────────

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

// ── Async dispatch timeout ───────────────────────────────────────────────

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
