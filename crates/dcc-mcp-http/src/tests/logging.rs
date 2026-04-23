use super::*;

// ── logging ───────────────────────────────────────────────────────────

#[tokio::test]
pub async fn test_logging_set_level_updates_session_threshold() {
    let state = make_app_state();
    let sessions = state.sessions.clone();
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

    let init: Value = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 601,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "log-client", "version": "1.0"}
            }
        }))
        .await
        .json();
    let sid = init["result"]["__session_id"].as_str().unwrap().to_string();
    assert_eq!(sessions.get_log_level(&sid), SessionLogLevel::Info);

    let resp: Value = server
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
            "id": 602,
            "method": "logging/setLevel",
            "params": {"level": "debug"}
        }))
        .await
        .json();
    assert!(resp["error"].is_null(), "unexpected error: {resp}");
    assert_eq!(sessions.get_log_level(&sid), SessionLogLevel::Debug);
}

#[tokio::test]
pub async fn test_logging_notifications_respect_session_threshold() {
    let state = make_app_state();
    let sessions = state.sessions.clone();
    let sid = sessions.create();
    sessions.set_protocol_version(&sid, "2025-06-18");
    let mut rx = sessions.subscribe(&sid).expect("session receiver");

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

    let _set_debug = server
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
            "id": 611,
            "method": "logging/setLevel",
            "params": {"level": "debug"}
        }))
        .await;

    let _call_debug = server
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
            "id": 612,
            "method": "tools/call",
            "params": {"name": "list_objects", "arguments": {}}
        }))
        .await;

    let debug_events = drain_sse_events(&mut rx, 16);
    assert!(
        debug_events.iter().any(|event| {
            event["method"] == "notifications/message"
                && event["params"]["level"] == "debug"
                && event["params"]["data"]["event"] == "tools_call_received"
        }),
        "expected debug notifications/message after setLevel=debug, got: {debug_events:?}"
    );

    let _set_warning = server
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
            "id": 613,
            "method": "logging/setLevel",
            "params": {"level": "warning"}
        }))
        .await;

    let _call_warning = server
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
            "id": 614,
            "method": "tools/call",
            "params": {"name": "list_objects", "arguments": {}}
        }))
        .await;

    let warning_events = drain_sse_events(&mut rx, 16);
    assert!(
        warning_events.iter().all(|event| {
            !(event["method"] == "notifications/message" && event["params"]["level"] == "debug")
        }),
        "debug messages should be suppressed at warning threshold: {warning_events:?}"
    );
    assert!(
        warning_events.iter().any(|event| {
            event["method"] == "notifications/message" && event["params"]["level"] == "error"
        }),
        "error notifications should still be delivered at warning threshold: {warning_events:?}"
    );
}

#[tokio::test]
pub async fn test_tools_call_error_includes_log_tail_for_request() {
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
