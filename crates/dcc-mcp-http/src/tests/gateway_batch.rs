use super::*;

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
