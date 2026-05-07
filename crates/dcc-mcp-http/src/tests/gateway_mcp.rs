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
async fn test_gateway_mcp_tools_list_omits_removed_verbs() {
    // The instance discovery triple (#813 phase 1) and the diagnostics +
    // catalog tools (#813 phase 2) were removed in favour of MCP
    // resources. Their absence is part of the surface contract; assert it
    // explicitly so a regression that re-adds them is loud.
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
    for removed in [
        // #813 phase 1
        "list_dcc_instances",
        "get_dcc_instance",
        "connect_to_dcc",
        // #813 phase 2 — diagnostics → resources
        "diagnostics__process_status",
        "diagnostics__audit_log",
        "diagnostics__tool_metrics",
        // #813 phase 2 — catalog → resources
        "dcc_catalog__search",
        "dcc_catalog__describe",
    ] {
        assert!(
            !names.contains(&removed),
            "{removed} must not appear in tools/list: {names:?}",
        );
    }
    // Lease verbs and dynamic-capability wrappers stay published.
    for kept in [
        "acquire_dcc_instance",
        "release_dcc_instance",
        "search_tools",
        "describe_tool",
        "call_tool",
    ] {
        assert!(
            names.contains(&kept),
            "{kept} should still be published: {names:?}",
        );
    }
    // Skill-management tools also stay published (they are a separate
    // namespace from the dispatch verbs and are still tools, not resources).
    for skill_mgmt in [
        "list_skills",
        "search_skills",
        "get_skill_info",
        "load_skill",
        "unload_skill",
    ] {
        assert!(
            names.contains(&skill_mgmt),
            "skill-management tool {skill_mgmt} should still be published: {names:?}",
        );
    }
    // 5 dispatch verbs + 5 skill-management = 10 gateway meta-tools.
    // Diagnostics + catalog moved to resources (#813 phase 2).
    assert_eq!(
        names.len(),
        10,
        "expected 10 gateway meta-tools (5 dispatch + 5 skill mgmt) after #813 phases 1+2, got: {names:?}",
    );
}

#[tokio::test]
async fn test_gateway_resources_list_includes_all_native_pointers() {
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
    for required in [
        "gateway://instances",
        "gateway://diagnostics/process",
        "gateway://diagnostics/audit",
        "gateway://diagnostics/metrics",
        "gateway://catalog",
    ] {
        assert!(
            uris.contains(&required),
            "{required} must appear in resources/list: {uris:?}",
        );
    }
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
async fn test_gateway_resources_read_diagnostics_process() {
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
            "jsonrpc": "2.0", "id": 1, "method": "resources/read",
            "params": {"uri": "gateway://diagnostics/process"}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let text = body["result"]["contents"][0]["text"]
        .as_str()
        .expect("no text content");
    let result: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["success"], true);
    assert_eq!(result["counts"]["total"], 1);
    assert_eq!(result["instances"][0]["dcc_type"], "maya");
}

#[tokio::test]
async fn test_gateway_resources_read_diagnostics_process_with_dcc_filter() {
    let state = make_gateway_state();
    {
        let reg = state.registry.read().await;
        reg.register(ServiceEntry::new("maya", "127.0.0.1", 19770))
            .unwrap();
        reg.register(ServiceEntry::new("blender", "127.0.0.1", 19771))
            .unwrap();
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
            "params": {"uri": "gateway://diagnostics/process?dcc_type=maya"}
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    let text = body["result"]["contents"][0]["text"]
        .as_str()
        .expect("no text content");
    let result: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(result["counts"]["total"], 1, "filter must apply");
    assert_eq!(result["instances"][0]["dcc_type"], "maya");
}

#[tokio::test]
async fn test_gateway_resources_read_diagnostics_audit_and_metrics() {
    let server = TestServer::new(make_gateway_router());

    for uri in [
        "gateway://diagnostics/audit",
        "gateway://diagnostics/metrics",
    ] {
        let resp = server
            .post("/mcp")
            .add_header(
                axum::http::header::CONTENT_TYPE,
                "application/json".parse::<HeaderValue>().unwrap(),
            )
            .json(&serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "resources/read",
                "params": {"uri": uri}
            }))
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert!(body.get("error").is_none(), "{uri} returned error: {body}",);
        let text = body["result"]["contents"][0]["text"]
            .as_str()
            .expect("no text content");
        let result: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            result["success"], true,
            "{uri} payload must report success: {result}",
        );
    }
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
