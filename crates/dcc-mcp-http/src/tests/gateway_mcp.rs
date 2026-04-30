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
    assert!(
        names.contains(&"diagnostics__process_status"),
        "diagnostics__process_status missing: {names:?}"
    );
    assert!(
        names.contains(&"diagnostics__audit_log"),
        "diagnostics__audit_log missing: {names:?}"
    );
    assert!(
        names.contains(&"diagnostics__tool_metrics"),
        "diagnostics__tool_metrics missing: {names:?}"
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
async fn test_gateway_mcp_instances_list_method_with_entry() {
    let state = make_gateway_state();
    {
        let reg = state.registry.read().await;
        let entry = ServiceEntry::new("maya", "127.0.0.1", 19766);
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
            "jsonrpc": "2.0", "id": 11, "method": "instances/list", "params": {}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["result"]["total"], 1);
    assert_eq!(body["result"]["instances"][0]["dcc_type"], "maya");
}

#[tokio::test]
async fn test_gateway_diagnostics_tools_are_native() {
    let state = make_gateway_state();
    {
        let reg = state.registry.read().await;
        let entry = ServiceEntry::new("maya", "127.0.0.1", 19767);
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
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {"name": "diagnostics__process_status", "arguments": {}}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["result"]["isError"], false);
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .expect("no text content");
    let result: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["success"], true);
    assert_eq!(result["counts"]["total"], 1);
    assert_eq!(result["instances"][0]["dcc_type"], "maya");
}

#[tokio::test]
async fn test_connect_to_dcc_succeeds_for_available_instance_prefix() {
    let state = make_gateway_state();
    let prefix = {
        let reg = state.registry.read().await;
        let entry = ServiceEntry::new("maya", "127.0.0.1", 19768);
        let prefix = entry.instance_id.to_string()[..8].to_string();
        reg.register(entry).unwrap();
        prefix
    };
    let server = TestServer::new(build_gateway_router(state));
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {"name": "connect_to_dcc", "arguments": {"instance_id": prefix}}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["result"]["isError"], false);
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .expect("no text content");
    let result: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["dcc_type"], "maya");
    assert_eq!(result["mcp_url"], "http://127.0.0.1:19768/mcp");
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
