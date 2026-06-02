use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_host_rpc::{HostRpcClient, StubHostRpcClient, UnavailableHostRpcClient};
use serde_json::{Value, json};

use super::*;

async fn connected_stub_client() -> Box<dyn HostRpcClient> {
    let mut client = StubHostRpcClient::new();
    client
        .connect("stub://localhost", Duration::from_millis(10))
        .await
        .expect("connect stub client");
    Box::new(client)
}

async fn fresh_listener() -> SidecarMcpListenerHandle {
    let client = connected_stub_client().await;
    let state = SidecarMcpState::new(client, "test-0.0.0");
    spawn_listener(state, "127.0.0.1", 0)
        .await
        .expect("spawn_listener")
}

async fn post_mcp(url: &str, body: Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(url)
        .json(&body)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("POST /mcp")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn healthz_responds_ok() {
    let handle = fresh_listener().await;
    let response = reqwest::Client::new()
        .get(format!("http://{}/healthz", handle.bind_addr))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .expect("GET /healthz");
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "ok");
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_aliases_support_gateway_probes() {
    let handle = fresh_listener().await;
    let client = reqwest::Client::new();

    let health: Value = client
        .get(format!("http://{}/health", handle.bind_addr))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .expect("GET /health")
        .json()
        .await
        .expect("parse /health");
    assert_eq!(health["ok"], true);

    let healthz: Value = client
        .get(format!("http://{}/v1/healthz", handle.bind_addr))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .expect("GET /v1/healthz")
        .json()
        .await
        .expect("parse /v1/healthz");
    assert_eq!(healthz["ok"], true);

    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readyz_reports_fully_ready() {
    let handle = fresh_listener().await;
    let response = reqwest::Client::new()
        .get(format!("http://{}/v1/readyz", handle.bind_addr))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .expect("GET /v1/readyz");
    assert_eq!(response.status().as_u16(), 200);
    let body: Value = response.json().await.expect("parse /v1/readyz");

    assert_eq!(body["process"], true);
    assert_eq!(body["dispatcher"], true);
    assert_eq!(body["dcc"], true);
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readyz_reports_dispatcher_unavailable() {
    let client: Box<dyn HostRpcClient> = Box::new(UnavailableHostRpcClient::new(
        "host-rpc connect to `qtserver://127.0.0.1:1` failed",
    ));
    let state = SidecarMcpState::new(client, "test-0.0.0");
    let handle = spawn_listener(state, "127.0.0.1", 0).await.expect("spawn");

    let response = reqwest::Client::new()
        .get(format!("http://{}/v1/readyz", handle.bind_addr))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .expect("GET /v1/readyz");
    assert_eq!(response.status().as_u16(), 503);
    let body: Value = response.json().await.expect("parse /v1/readyz");

    assert_eq!(body["process"], true);
    assert_eq!(body["dispatcher"], false);
    assert_eq!(body["dcc"], false);
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialize_returns_negotiated_protocol_version() {
    let handle = fresh_listener().await;
    let body: Value = post_mcp(
        &handle.mcp_url,
        json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "initialize",
            "params": {"protocolVersion": "2025-03-26"}
        }),
    )
    .await
    .json()
    .await
    .expect("parse JSON");

    assert_eq!(body["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    assert_eq!(body["result"]["serverInfo"]["name"], SIDECAR_SERVER_NAME);
    assert_eq!(body["result"]["serverInfo"]["version"], "test-0.0.0");
    // tools.listChanged: false - we intentionally don't promise
    // discovery here; gateway is the discovery surface.
    assert_eq!(
        body["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ping_echoes_empty_result() {
    let handle = fresh_listener().await;
    let body: Value = post_mcp(
        &handle.mcp_url,
        json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
    )
    .await
    .json()
    .await
    .expect("parse JSON");

    assert_eq!(body["id"], 2);
    assert!(body["result"].is_object());
    assert_eq!(body["result"].as_object().unwrap().len(), 0);
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tools_call_routes_through_host_rpc_client() {
    // StubHostRpcClient::call always returns transport("stub client")
    // and records the action slug - so we can assert the listener
    // forwarded the right slug to the client.
    let stub = Arc::new(StubHostRpcClient::new());
    let stub_for_state: Box<dyn HostRpcClient> = Box::new(StubHostRpcClient::new());
    // We can't share Arc<StubHostRpcClient> directly because the
    // listener owns its own copy via Box<dyn>. So we make a
    // separate stub and verify via the response error instead -
    // the wire path is what we want to pin.
    drop(stub);

    let state = SidecarMcpState::new(stub_for_state, "test");
    let handle = spawn_listener(state, "127.0.0.1", 0).await.expect("spawn");

    let body: Value = post_mcp(
        &handle.mcp_url,
        json!({
            "jsonrpc": "2.0", "id": "req-1",
            "method": "tools/call",
            "params": {
                "name": "maya_primitives__create_sphere",
                "arguments": {"radius": 1.0}
            }
        }),
    )
    .await
    .json()
    .await
    .expect("parse JSON");

    // Stub client returns TransportError("stub client") - that
    // travels through our error envelope into the JSON-RPC error
    // structure. Pin the wire shape.
    assert_eq!(body["id"], "req-1");
    assert!(
        body["error"].is_object(),
        "tools/call against stub must produce JSON-RPC error envelope: {body}"
    );
    assert_eq!(body["error"]["code"], -32000);
    assert_eq!(body["error"]["message"], "transport-error");
    assert_eq!(body["error"]["data"]["kind"], "transport-error");
    assert!(
        body["error"]["data"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("stub client"),
        "data.message should propagate the transport error: {body}"
    );

    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_method_returns_method_not_found() {
    let handle = fresh_listener().await;
    let body: Value = post_mcp(
        &handle.mcp_url,
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
    )
    .await
    .json()
    .await
    .expect("parse JSON");

    assert_eq!(body["error"]["code"], -32601);
    // Hint to the agent that this is not a generic MCP server.
    assert!(body["error"]["data"]["note"].is_string());
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notification_is_accepted_without_response_body() {
    let handle = fresh_listener().await;
    let response = post_mcp(
        &handle.mcp_url,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    )
    .await;
    // Notifications: 202 Accepted, empty body (RFC 8259 §4 / MCP
    // Streamable HTTP). Distinguishes from request responses
    // which always carry id + result|error.
    assert_eq!(response.status(), 202);
    let body = response.text().await.unwrap();
    assert!(
        body.is_empty(),
        "notification body should be empty, got: {body:?}"
    );
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parse_error_carries_jsonrpc_minus_32700() {
    let handle = fresh_listener().await;
    let response = reqwest::Client::new()
        .post(&handle.mcp_url)
        .body("{ not even json")
        .header("content-type", "application/json")
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .expect("POST malformed");
    // Servers SHOULD return 200 with a JSON-RPC error envelope
    // for parse errors (per JSON-RPC 2.0 §5.1). Some hosts treat
    // 4xx as transport failure so the envelope path is more
    // diagnosable.
    let body: Value = response.json().await.expect("parse JSON");
    assert_eq!(body["error"]["code"], -32700);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_id_is_echoed_back_in_header() {
    let handle = fresh_listener().await;
    let response = reqwest::Client::new()
        .post(&handle.mcp_url)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .header("Mcp-Session-Id", "pinned-session-42")
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .expect("POST");
    let echoed = response
        .headers()
        .get("Mcp-Session-Id")
        .expect("server must echo Mcp-Session-Id")
        .to_str()
        .unwrap();
    assert_eq!(echoed, "pinned-session-42");
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_initialize_requests_all_succeed() {
    // Pins the contract from #1009: multiple concurrent MCP
    // clients attaching to the same listener must each get a
    // negotiated `initialize` response without serialization-
    // induced timeouts. The HostRpcClient mutex only matters for
    // `tools/call`; `initialize` should be lock-free.
    let handle = fresh_listener().await;
    let mut handles = Vec::new();
    for client_idx in 0..8 {
        let url = handle.mcp_url.clone();
        handles.push(tokio::spawn(async move {
            post_mcp(
                &url,
                json!({
                    "jsonrpc": "2.0",
                    "id": client_idx,
                    "method": "initialize",
                    "params": {"protocolVersion": "2025-03-26"}
                }),
            )
            .await
            .json::<Value>()
            .await
            .expect("parse")
        }));
    }
    for h in handles {
        let body = tokio::time::timeout(Duration::from_secs(2), h)
            .await
            .expect("each initialize must complete within 2s")
            .expect("task did not panic");
        assert_eq!(body["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    }
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_stops_listener_quickly() {
    let handle = fresh_listener().await;
    let addr = handle.bind_addr;
    let start = std::time::Instant::now();
    handle.shutdown().await;
    // Graceful shutdown should be well under the 5s hard cap.
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "shutdown took {:?} - should be sub-second on the happy path",
        start.elapsed()
    );
    // After shutdown the listener should refuse new connections.
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        reqwest::Client::new()
            .get(format!("http://{}/healthz", addr))
            .send(),
    )
    .await;
    match result {
        Ok(Err(_)) => {} // connection refused / closed - expected
        Err(_) => {}     // request timed out - also acceptable
        Ok(Ok(_)) => panic!("listener should not accept after shutdown"),
    }
}
