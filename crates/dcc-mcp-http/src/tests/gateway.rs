use axum::http::HeaderValue;
use axum_test::TestServer;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;

use crate::gateway::router::build_gateway_router;
use crate::gateway::state::GatewayState;

fn make_gateway_state() -> GatewayState {
    let dir = tempfile::tempdir().unwrap();
    // keep() returns PathBuf and prevents deletion until the process exits
    let path = dir.keep();
    let registry = FileRegistry::new(&path).unwrap();
    let (yield_tx, _yield_rx) = tokio::sync::watch::channel(false);
    let (events_tx, _) = tokio::sync::broadcast::channel(16);
    GatewayState {
        registry: Arc::new(RwLock::new(registry)),
        stale_timeout: Duration::from_secs(30),
        backend_timeout: Duration::from_secs(10),
        async_dispatch_timeout: Duration::from_secs(60),
        wait_terminal_timeout: Duration::from_secs(600),
        server_name: "test-gateway".to_string(),
        server_version: "0.1.0".to_string(),
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
    }
}

fn make_gateway_router() -> axum::Router {
    build_gateway_router(make_gateway_state())
}

// ── REST endpoints ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_gateway_health_endpoint() {
    let server = TestServer::new(make_gateway_router());
    let resp = server.get("/health").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn test_gateway_instances_endpoint_empty() {
    let server = TestServer::new(make_gateway_router());
    let resp = server.get("/instances").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["total"], 0);
    assert!(body["instances"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_gateway_instances_endpoint_with_entry() {
    let state = make_gateway_state();
    {
        let reg = state.registry.read().await;
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        reg.register(entry).unwrap();
    }
    let server = TestServer::new(build_gateway_router(state));
    let resp = server.get("/instances").await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["total"], 1);
}

// ── MCP endpoint ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_gateway_mcp_initialize() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0"}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["result"]["protocolVersion"], "2025-03-26");
}

#[tokio::test]
async fn test_gateway_mcp_ping() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["result"], serde_json::json!({}));
}

#[tokio::test]
async fn test_gateway_mcp_tools_list() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {}}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(
        names.contains(&"list_dcc_instances"),
        "list_dcc_instances missing: {names:?}"
    );
    assert!(
        names.contains(&"connect_to_dcc"),
        "connect_to_dcc missing: {names:?}"
    );
}

#[tokio::test]
async fn test_gateway_mcp_list_dcc_instances_empty() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "list_dcc_instances", "arguments": {}}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .expect("no text content");
    let result: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["total"], 0);
}

#[tokio::test]
async fn test_gateway_mcp_list_dcc_instances_with_entry() {
    let state = make_gateway_state();
    {
        let reg = state.registry.read().await;
        let entry = ServiceEntry::new("houdini", "127.0.0.1", 19765);
        reg.register(entry).unwrap();
    }
    let server = TestServer::new(build_gateway_router(state));
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "list_dcc_instances", "arguments": {}}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .expect("no text content");
    let result: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["total"], 1);
    assert_eq!(result["instances"][0]["dcc_type"], "houdini");
}

#[tokio::test]
async fn test_gateway_mcp_unknown_method() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 99, "method": "nonexistent"}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert!(
        body.get("error").is_some(),
        "expected error for unknown method"
    );
}

// ── GatewayRunner port-competition ────────────────────────────────────

#[tokio::test]
async fn test_gateway_runner_single_start() {
    use crate::gateway::{GatewayConfig, GatewayRunner};

    let dir = tempfile::tempdir().unwrap();
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: 0,   // 0 disables gateway, so start() registers only
        heartbeat_secs: 0, // no heartbeat in test
        registry_dir: Some(dir.path().to_path_buf()),
        ..GatewayConfig::default()
    };
    let runner = GatewayRunner::new(cfg).unwrap();
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let handle = runner.start(entry).await.unwrap();
    // gateway_port=0 means we never attempt to bind
    assert!(!handle.is_gateway);
}

#[tokio::test]
async fn test_gateway_port_competition() {
    use crate::gateway::{GatewayConfig, GatewayRunner};

    // Find a free port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    // Small sleep so the OS fully releases the port
    tokio::time::sleep(Duration::from_millis(50)).await;

    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();

    let cfg1 = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: port,
        heartbeat_secs: 0,
        registry_dir: Some(dir1.path().to_path_buf()),
        ..GatewayConfig::default()
    };
    let cfg2 = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: port,
        heartbeat_secs: 0,
        registry_dir: Some(dir2.path().to_path_buf()),
        ..GatewayConfig::default()
    };

    let runner1 = GatewayRunner::new(cfg1).unwrap();
    let runner2 = GatewayRunner::new(cfg2).unwrap();

    let entry1 = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let entry2 = ServiceEntry::new("maya", "127.0.0.1", 18813);

    let h1 = runner1.start(entry1).await.unwrap();
    let h2 = runner2.start(entry2).await.unwrap();

    // Exactly one should win the gateway port
    assert_ne!(
        h1.is_gateway, h2.is_gateway,
        "exactly one process should win gateway port (h1={}, h2={})",
        h1.is_gateway, h2.is_gateway
    );
}

#[tokio::test]
async fn test_gateway_runner_is_gateway_true_when_port_free() {
    use crate::gateway::{GatewayConfig, GatewayRunner};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let dir = tempfile::tempdir().unwrap();
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: port,
        heartbeat_secs: 0,
        registry_dir: Some(dir.path().to_path_buf()),
        ..GatewayConfig::default()
    };
    let runner = GatewayRunner::new(cfg).unwrap();
    let entry = ServiceEntry::new("blender", "127.0.0.1", 19000);
    let handle = runner.start(entry).await.unwrap();
    assert!(handle.is_gateway, "first runner should win free port");
}

// ── JSON-RPC batch ───────────────────────────────────────────────────

#[tokio::test]
async fn test_gateway_mcp_batch_mixed_request_and_notification() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!([
            {"jsonrpc": "2.0", "id": 1, "method": "ping"},
            {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "ping"}
        ]))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let arr = body.as_array().expect("batch must return array");
    assert_eq!(arr.len(), 2, "notification must not produce a response");
    assert_eq!(arr[0]["id"], 1);
    assert_eq!(arr[1]["id"], 2);
}

#[tokio::test]
async fn test_gateway_mcp_batch_all_notifications_returns_202() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!([
            {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}},
            {"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 42}}
        ]))
        .await;
    assert_eq!(resp.status_code().as_u16(), 202);
}

#[tokio::test]
async fn test_gateway_mcp_batch_invalid_entry_returns_parse_error() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!([
            {"jsonrpc": "2.0", "id": 1, "method": "ping"},
            "not-an-object",
            {"jsonrpc": "2.0", "id": 3, "method": "ping"}
        ]))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["id"], 1);
    assert_eq!(arr[1]["error"]["code"], -32700);
    assert_eq!(arr[2]["id"], 3);
}

// ── Session id ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_gateway_mcp_post_returns_session_id_header() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .await;
    resp.assert_status_ok();
    let sid = resp
        .headers()
        .get("Mcp-Session-Id")
        .expect("POST /mcp must return Mcp-Session-Id");
    assert!(!sid.is_empty());
}

#[tokio::test]
async fn test_gateway_mcp_post_preserves_client_session_id() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header(
            "Mcp-Session-Id",
            "client-sid-123".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .await;
    resp.assert_status_ok();
    let sid = resp.headers().get("Mcp-Session-Id").unwrap();
    assert_eq!(sid, "client-sid-123");
}

#[tokio::test]
async fn test_gateway_get_sse_returns_session_id_header() {
    let server = TestServer::builder()
        .http_transport()
        .build(make_gateway_router());
    let client = reqwest::Client::new();
    let url = server.server_url("/mcp").unwrap();
    let resp = client
        .get(url.as_str())
        .header(axum::http::header::ACCEPT, "text/event-stream")
        .send()
        .await
        .expect("GET /mcp SSE request must succeed");
    assert_eq!(resp.status(), 200);
    let sid = resp
        .headers()
        .get("Mcp-Session-Id")
        .expect("GET /mcp SSE must return Mcp-Session-Id");
    assert!(!sid.is_empty());
}

// ── Resources subscribe / unsubscribe ────────────────────────────────

#[tokio::test]
async fn test_gateway_mcp_resources_subscribe_tracks_subscription() {
    let state = make_gateway_state();
    let server = TestServer::new(build_gateway_router(state.clone()));
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header("Mcp-Session-Id", "sess-abc".parse::<HeaderValue>().unwrap())
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "resources/subscribe",
            "params": {"uri": "dcc://maya/1234"}
        }))
        .await;
    resp.assert_status_ok();

    let subs = state.resource_subscriptions.read().await;
    let uris = subs.get("sess-abc").expect("subscription must be recorded");
    assert!(uris.contains("dcc://maya/1234"));
}

#[tokio::test]
async fn test_gateway_mcp_resources_unsubscribe_removes_subscription() {
    let state = make_gateway_state();
    {
        let mut subs = state.resource_subscriptions.write().await;
        let mut set = std::collections::HashSet::new();
        set.insert("dcc://maya/1234".to_string());
        subs.insert("sess-def".to_string(), set);
    }

    let server = TestServer::new(build_gateway_router(state.clone()));
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header("Mcp-Session-Id", "sess-def".parse::<HeaderValue>().unwrap())
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "resources/unsubscribe",
            "params": {"uri": "dcc://maya/1234"}
        }))
        .await;
    resp.assert_status_ok();

    let subs = state.resource_subscriptions.read().await;
    let uris = subs.get("sess-def").unwrap();
    assert!(!uris.contains("dcc://maya/1234"));
}

// ── Protocol version storage ─────────────────────────────────────────

#[tokio::test]
async fn test_gateway_mcp_initialize_stores_negotiated_version() {
    let state = make_gateway_state();
    let server = TestServer::new(build_gateway_router(state.clone()));
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"protocolVersion": "2025-03-26"}
        }))
        .await;
    resp.assert_status_ok();

    let pv = state.protocol_version.read().await;
    assert_eq!(pv.as_deref(), Some("2025-03-26"));
}

// ── Pagination (local tools only, no backends) ───────────────────────

#[tokio::test]
async fn test_gateway_mcp_tools_list_no_cursor_no_next_cursor_for_small_list() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert!(
        body["result"]["nextCursor"].is_null(),
        "small aggregated list must not have nextCursor"
    );
}
