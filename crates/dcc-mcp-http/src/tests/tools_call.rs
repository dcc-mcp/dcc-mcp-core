use super::*;

// ── tools/call known (no handler registered) ──────────────────────────

#[tokio::test]
pub async fn test_tools_call_known_tool() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "get_scene_info",
                "arguments": {}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("no handler") || text.contains("register"));
}

// ── tools/call unknown ─────────────────────────────────────────────────

#[tokio::test]
pub async fn test_tools_call_unknown_tool() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "nonexistent_tool",
                "arguments": {}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
}

#[tokio::test]
pub async fn test_unknown_tool_returns_not_found() {
    let server = TestServer::new(make_router_with_skill());
    let resp = server.post("/mcp")
            .add_header(axum::http::header::ACCEPT, "application/json".parse::<HeaderValue>().unwrap())
            .json(&json!({"jsonrpc":"2.0","id":130,"method":"tools/call","params":{"name":"totally_unknown_xyzzy","arguments":{}}}))
            .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("Unknown tool") || text.contains("ACTION_NOT_FOUND"),
        "Expected Unknown: {text}"
    );
}

#[tokio::test]
pub async fn test_search_skills_scope_filter_rejects_invalid_scope() {
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 141,
            "method": "tools/call",
            "params": {
                "name": "search_skills",
                "arguments": {"scope": "bogus"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
}

#[tokio::test]
pub async fn test_find_skills_forwards_and_marks_deprecated() {
    // Issue #340: find_skills is now a compatibility alias. It must still
    // return valid results AND attach `_meta["dcc.deprecation"]`.
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 142,
            "method": "tools/call",
            "params": {
                "name": "find_skills",
                "arguments": {"query": "bevel"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], false);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("maya-bevel"), "forwarded result: {text}");
    assert_eq!(
        body["result"]["_meta"]["dcc.deprecation"]
            .as_str()
            .unwrap_or(""),
        "find_skills is deprecated — use search_skills. Will be removed in v0.17."
    );
}
