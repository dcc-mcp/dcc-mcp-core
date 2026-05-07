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
async fn test_gateway_mcp_tools_list_omits_removed_instance_verbs() {
    // The instance discovery triple (`list_dcc_instances`, `get_dcc_instance`,
    // `connect_to_dcc`) was removed in #813 phase 1 in favour of the
    // `gateway://instances` MCP resource. Their absence is part of the
    // surface contract; assert it explicitly so a regression that
    // re-adds them is loud.
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
    for removed in ["list_dcc_instances", "get_dcc_instance", "connect_to_dcc"] {
        assert!(
            !names.contains(&removed),
            "{removed} must not appear in tools/list (#813 phase 1 removed it): {names:?}",
        );
    }
    // Lease verbs and dynamic-capability wrappers stay published.
    for kept in [
        "acquire_dcc_instance",
        "release_dcc_instance",
        "search_tools",
        "describe_tool",
        "call_tool",
        "diagnostics__process_status",
        "diagnostics__audit_log",
        "diagnostics__tool_metrics",
    ] {
        assert!(
            names.contains(&kept),
            "{kept} should still be published: {names:?}",
        );
    }
}

#[tokio::test]
async fn test_gateway_resources_list_includes_gateway_instances_pointer() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "resources/list", "params": {}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let resources = body["result"]["resources"].as_array().unwrap();
    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
    assert!(
        uris.contains(&"gateway://instances"),
        "resources/list must include gateway://instances pointer: {uris:?}",
    );
}

#[tokio::test]
async fn test_gateway_resources_read_instances_empty() {
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "resources/read",
            "params": {"uri": "gateway://instances"}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let text = body["result"]["contents"][0]["text"]
        .as_str()
        .expect("no text content");
    let result: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["total"], 0);
    assert!(result["instances"].is_array());
}

#[tokio::test]
async fn test_gateway_resources_read_instances_with_entry_carries_mcp_url() {
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
            "jsonrpc": "2.0", "id": 1, "method": "resources/read",
            "params": {"uri": "gateway://instances"}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let text = body["result"]["contents"][0]["text"]
        .as_str()
        .expect("no text content");
    let result: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["total"], 1);
    assert_eq!(result["instances"][0]["dcc_type"], "houdini");
    // Each entry carries `mcp_url` so the client connects without a
    // follow-up tool call.
    assert_eq!(
        result["instances"][0]["mcp_url"],
        "http://127.0.0.1:19765/mcp"
    );
}

#[tokio::test]
async fn test_gateway_resources_read_single_instance_by_prefix() {
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
            "jsonrpc": "2.0", "id": 1, "method": "resources/read",
            "params": {"uri": format!("gateway://instances/{prefix}")}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let text = body["result"]["contents"][0]["text"]
        .as_str()
        .expect("no text content");
    let result: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["dcc_type"], "maya");
    assert_eq!(result["mcp_url"], "http://127.0.0.1:19768/mcp");
}

#[tokio::test]
async fn test_gateway_resources_read_instances_query_filters() {
    // `?include_stale=false` is parsed and forwarded to the underlying
    // registry view (smoke test — verifies URI query plumbing).
    let server = TestServer::new(make_gateway_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "resources/read",
            "params": {"uri": "gateway://instances?include_stale=false"}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert!(body.get("error").is_none(), "got error: {body}");
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
