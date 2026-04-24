use super::*;

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
