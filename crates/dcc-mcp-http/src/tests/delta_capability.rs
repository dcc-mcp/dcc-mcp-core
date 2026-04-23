use super::*;

// ── Delta notification capability negotiation ─────────────────────────

#[tokio::test]
pub async fn test_initialize_negotiates_delta_capability() {
    let server = TestServer::new(make_router_with_skill());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {
                    "experimental": {
                        "dcc_mcp_core/deltaToolsUpdate": { "enabled": true }
                    }
                },
                "clientInfo": {"name": "delta-client", "version": "1.0"}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let exp = &body["result"]["capabilities"]["experimental"];
    assert_eq!(
        exp["dcc_mcp_core/deltaToolsUpdate"]["enabled"], true,
        "Server must echo delta capability: {exp}"
    );
}

#[tokio::test]
pub async fn test_initialize_no_delta_when_not_requested() {
    let server = TestServer::new(make_router_with_skill());
    let body: Value = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "plain-client", "version": "1.0"}
            }
        }))
        .await
        .json();
    assert!(
        body["result"]["capabilities"]["experimental"].is_null(),
        "Server must not advertise delta when client did not opt in"
    );
}

#[test]
pub fn test_session_supports_delta_tools() {
    let mgr = SessionManager::new();
    let id = mgr.create();
    assert!(!mgr.supports_delta_tools(&id));
    assert!(mgr.set_supports_delta_tools(&id, true));
    assert!(mgr.supports_delta_tools(&id));
    assert!(mgr.set_supports_delta_tools(&id, false));
    assert!(!mgr.supports_delta_tools(&id));
    assert!(!mgr.supports_delta_tools("nonexistent"));
}
