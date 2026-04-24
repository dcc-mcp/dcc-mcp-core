use super::*;

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
