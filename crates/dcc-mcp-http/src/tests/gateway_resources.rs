use super::*;

#[tokio::test]
async fn test_gateway_mcp_resources_subscribe_tracks_subscription() {
    let state = make_gateway_state();
    let server = TestServer::new(build_gateway_router(state.clone()));
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header("Mcp-Session-Id", "sess-abc".parse::<HeaderValue>().unwrap())
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "resources/subscribe",
            "params": {"uri": "dcc://maya/1234"}
        }))
        .await;
    resp.assert_status_ok();

    let subs = state.resource_subscriptions.read().await;
    let uris = subs.get("sess-abc").expect("subscription must be recorded");
    assert!(uris.contains("dcc://maya/1234"));
}

#[tokio::test]
async fn test_gateway_mcp_resources_unsubscribe_removes_subscription() {
    let state = make_gateway_state();
    {
        let mut subs = state.resource_subscriptions.write().await;
        let mut set = std::collections::HashSet::new();
        set.insert("dcc://maya/1234".to_string());
        subs.insert("sess-def".to_string(), set);
    }

    let server = TestServer::new(build_gateway_router(state.clone()));
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::CONTENT_TYPE,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header("Mcp-Session-Id", "sess-def".parse::<HeaderValue>().unwrap())
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "resources/unsubscribe",
            "params": {"uri": "dcc://maya/1234"}
        }))
        .await;
    resp.assert_status_ok();

    let subs = state.resource_subscriptions.read().await;
    let uris = subs.get("sess-def").unwrap();
    assert!(!uris.contains("dcc://maya/1234"));
}
