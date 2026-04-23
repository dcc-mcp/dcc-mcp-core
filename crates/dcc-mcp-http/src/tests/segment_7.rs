use super::*;

}

#[tokio::test]
async fn test_tools_call_error_includes_log_tail_for_request() {
    let state = make_app_state();
    let sessions = state.sessions.clone();
    let sid = sessions.create();
    sessions.set_protocol_version(&sid, "2025-06-18");

    let server = TestServer::new(
        axum::Router::new()
            .route(
                "/mcp",
                axum::routing::post(crate::handler::handle_post)
                    .get(crate::handler::handle_get)
                    .delete(crate::handler::handle_delete),
            )
            .with_state(state),
    );

    let body: Value = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header(
            axum::http::HeaderName::from_static("mcp-session-id"),
            sid.parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 621,
            "method": "tools/call",
            "params": {"name": "list_objects", "arguments": {}}
        }))
        .await
        .json();

    assert_eq!(
        body["result"]["isError"], true,
        "expected error result: {body}"
    );
    let envelope_text = body["result"]["content"][0]["text"].as_str().unwrap();
    let envelope: Value =
        serde_json::from_str(envelope_text).expect("error envelope must be valid JSON");
    let tail = envelope["details"]["log_tail"]
        .as_array()
        .expect("details.log_tail should be an array");
    assert!(
        !tail.is_empty(),
        "expected non-empty details.log_tail, envelope={envelope}"
    );
    assert!(
        tail.iter()
            .all(|line| line["request_id"] == "621" && line["logger"].is_string()),
        "log_tail entries must correlate with request id: {tail:?}"
    );
}

#[test]
fn test_session_supports_delta_tools() {
    let mgr = SessionManager::new();
    let id = mgr.create();
    assert!(!mgr.supports_delta_tools(&id));
    assert!(mgr.set_supports_delta_tools(&id, true));
    assert!(mgr.supports_delta_tools(&id));
    assert!(mgr.set_supports_delta_tools(&id, false));
    assert!(!mgr.supports_delta_tools(&id));
    assert!(!mgr.supports_delta_tools("nonexistent"));
}

// Submodules extracted from monolithic tests.rs
mod gateway;
mod lazy_actions;
mod next_tools_meta;
mod resource_link;
