use super::*;

// ── initialize ────────────────────────────────────────────────────────

#[tokio::test]
pub async fn test_initialize() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], 1);
    let result = &body["result"];
    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert_eq!(result["serverInfo"]["name"], "test-dcc");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["capabilities"]["logging"].is_object());
    assert!(result["__session_id"].is_string());
}

#[tokio::test]
pub async fn test_list_roots_reports_cached_session_roots() {
    let state = make_app_state();
    let sid = state.sessions.create();
    state.sessions.set_supports_roots(&sid, true);
    state.sessions.set_client_roots(
        &sid,
        vec![
            crate::protocol::ClientRoot {
                uri: "file:///projects/demo".to_string(),
                name: Some("Demo Root".to_string()),
            },
            crate::protocol::ClientRoot {
                uri: "file:///projects/demo/assets".to_string(),
                name: None,
            },
        ],
    );
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

    let init = server
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
            "id": 301,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {"roots": {}},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        }))
        .await;
    init.assert_status_ok();

    let roots_call = server
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
            "id": 302,
            "method": "tools/call",
            "params": {"name": "list_roots", "arguments": {}}
        }))
        .await;
    roots_call.assert_status_ok();
    let body: Value = roots_call.json();
    assert_eq!(body["result"]["isError"], false);
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .expect("list_roots should return text payload");
    let payload: Value = serde_json::from_str(text).expect("list_roots payload must be valid JSON");
    assert_eq!(payload["supports_roots"], true);
    assert_eq!(payload["count"], 2);
    assert_eq!(
        payload["roots"][0]["uri"], "file:///projects/demo",
        "cached roots should include client-advertised root URI"
    );
}

#[tokio::test]
pub async fn test_list_roots_returns_cached_roots() {
    let state = make_app_state();
    let sid = state.sessions.create();
    state.sessions.set_supports_roots(&sid, true);
    state.sessions.set_client_roots(
        &sid,
        vec![crate::protocol::ClientRoot {
            uri: "file:///projects/demo".to_string(),
            name: Some("demo".to_string()),
        }],
    );

    let router = {
        use crate::handler::{handle_delete, handle_get, handle_post};
        use axum::{Router, routing};
        Router::new()
            .route(
                "/mcp",
                routing::post(handle_post)
                    .get(handle_get)
                    .delete(handle_delete),
            )
            .with_state(state)
    };
    let server = TestServer::new(router);

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header(
            "Mcp-Session-Id".parse::<axum::http::HeaderName>().unwrap(),
            sid.parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 301,
            "method": "tools/call",
            "params": {"name": "list_roots", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], false);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    assert_eq!(payload["supports_roots"], true);
    assert_eq!(payload["count"], 1);
    assert_eq!(payload["roots"][0]["uri"], "file:///projects/demo");
}

#[tokio::test]
pub async fn test_ping() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc": "2.0", "id": 99, "method": "ping"}))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["id"], 99);
    assert!(body["result"].is_object());
}

#[tokio::test]
pub async fn test_method_not_found() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc": "2.0", "id": 5, "method": "unknown/method"}))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(body["error"].is_object());
    assert_eq!(body["error"]["code"], -32601);
}

#[tokio::test]
pub async fn test_batch_requests() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!([
            {"jsonrpc": "2.0", "id": 1, "method": "ping"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}
        ]))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[tokio::test]
pub async fn test_get_without_sse_accept_returns_405() {
    let server = TestServer::new(make_router());

    let resp = server
        .get("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .await;

    resp.assert_status(axum::http::StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
pub async fn test_server_start_stop() {
    let registry = Arc::new(make_registry());
    let config = McpHttpConfig::new(0);
    let server = McpHttpServer::new(registry, config);
    let handle = server.start().await.unwrap();
    assert!(handle.port > 0);
    handle.shutdown().await;
}

#[tokio::test]
pub async fn test_initialize_reports_list_changed_true() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0"}
            }
        }))
        .await;

    let body: Value = resp.json();
    assert_eq!(body["result"]["capabilities"]["tools"]["listChanged"], true);
}
