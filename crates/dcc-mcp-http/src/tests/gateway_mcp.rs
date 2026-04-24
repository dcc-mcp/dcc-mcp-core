use super::*;

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
