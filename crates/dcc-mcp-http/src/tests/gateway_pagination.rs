use super::*;

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
