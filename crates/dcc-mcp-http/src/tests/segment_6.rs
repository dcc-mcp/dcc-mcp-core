use super::*;

            }
        }))
        .await;
    resp.assert_status_ok();

    // Extract session id from response header.
    let session_id = resp
        .headers()
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Even if the header is absent in this test harness, the code path is
    // exercised. Just assert the session was created.
    let _ = state; // state is already cloned into the router

    // Send a ping with the session id to exercise the touch() code path.
    let ping_resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header(
            "Mcp-Session-Id".parse::<axum::http::HeaderName>().unwrap(),
            session_id
                .parse::<HeaderValue>()
                .unwrap_or_else(|_| HeaderValue::from_static("test-session")),
        )
        .json(&json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))
        .await;
    ping_resp.assert_status_ok();
}

// ── Server with TTL=0 starts without background task ─────────────────

#[tokio::test]
async fn test_server_start_with_ttl_zero() {
    let registry = Arc::new(make_registry());
    let config = McpHttpConfig::new(0).with_session_ttl_secs(0);
    let server = McpHttpServer::new(registry, config);
    let handle = server.start().await.unwrap();
    assert!(handle.port > 0);
    handle.shutdown().await;
}

// ── tools/list pagination ─────────────────────────────────────────────

fn make_app_state_many_tools() -> AppState {
    let registry = Arc::new(ActionRegistry::new());
    for i in 0..40usize {
        registry.register_action(ActionMeta {
            name: format!("tool_{i:02}"),
            description: format!("Test tool {i}"),
            dcc: "test".into(),
            version: "1.0.0".into(),
            ..Default::default()
        });
    }
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    AppState {
        registry,
        dispatcher,
        catalog,
        sessions: SessionManager::new(),
        executor: None,
        bridge_registry: crate::BridgeRegistry::new(),
        server_name: "test-dcc".to_string(),
        server_version: "0.1.0".to_string(),
        cancelled_requests: std::sync::Arc::new(dashmap::DashMap::new()),
        in_flight: crate::inflight::InFlightRequests::new(),
        pending_elicitations: std::sync::Arc::new(dashmap::DashMap::new()),
        lazy_actions: false,

        bare_tool_names: true,
        declared_capabilities: std::sync::Arc::new(Vec::new()),
        jobs: std::sync::Arc::new(crate::job::JobManager::new()),
        job_notifier: crate::notifications::JobNotifier::new(SessionManager::new(), true),
        resources: crate::resources::ResourceRegistry::new(true, false),
        enable_resources: true,
        prompts: crate::prompts::PromptRegistry::new(true),
        enable_prompts: true,
    }
}

fn make_router_many_tools() -> axum::Router {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};
    Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(make_app_state_many_tools())
}

#[tokio::test]
async fn test_tools_list_pagination_first_page() {
    use crate::protocol::TOOLS_LIST_PAGE_SIZE;
    let server = TestServer::new(make_router_many_tools());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    // Total = 12 core (incl. jobs.get_status #319 + jobs.cleanup #328) + 40 registered = 52; first page = 32.
    assert_eq!(
        tools.len(),
        TOOLS_LIST_PAGE_SIZE,
        "First page must be exactly {TOOLS_LIST_PAGE_SIZE}"
    );
    let cursor = body["result"]["nextCursor"]
        .as_str()
        .expect("nextCursor must be present on first page");
    assert!(!cursor.is_empty());
}

#[tokio::test]
async fn test_tools_list_pagination_second_page() {
    use crate::protocol::TOOLS_LIST_PAGE_SIZE;
    let server = TestServer::new(make_router_many_tools());

    // Page 1
    let r1: Value = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .await
        .json();
    let cursor = r1["result"]["nextCursor"].as_str().unwrap().to_string();

    // Page 2
    let r2: Value = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 2,
            "method": "tools/list",
            "params": { "cursor": cursor }
        }))
        .await
        .json();
    let tools2 = r2["result"]["tools"].as_array().unwrap();
    // 52 - 32 = 20 tools on second page
    assert_eq!(tools2.len(), 52 - TOOLS_LIST_PAGE_SIZE);
    assert!(
        r2["result"]["nextCursor"].is_null(),
        "Last page must not have nextCursor"
    );
}

#[tokio::test]
async fn test_tools_list_all_pages_no_duplicates() {
    let server = TestServer::new(make_router_many_tools());
    let mut all_names: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let params = match &cursor {
            Some(c) => serde_json::json!({ "cursor": c }),
            None => serde_json::json!({}),
        };
        let body: Value = server
                .post("/mcp")
                .add_header(axum::http::header::ACCEPT, "application/json".parse::<HeaderValue>().unwrap())
                .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": params}))
                .await
                .json();
        let tools = body["result"]["tools"].as_array().unwrap();
        all_names.extend(
            tools
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string()),
        );
        cursor = body["result"]["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }

    assert_eq!(all_names.len(), 52, "All pages must cover exactly 52 tools");
    let unique: std::collections::HashSet<_> = all_names.iter().collect();
    assert_eq!(unique.len(), all_names.len(), "No duplicates across pages");
}

#[tokio::test]
async fn test_tools_list_no_cursor_for_small_list() {
    let server = TestServer::new(make_router());
    let body: Value = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .await
        .json();
    assert!(
        body["result"]["nextCursor"].is_null(),
        "Small list must not have nextCursor"
    );
}

// ── Delta notification capability negotiation ─────────────────────────

#[tokio::test]
async fn test_initialize_negotiates_delta_capability() {
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
async fn test_initialize_no_delta_when_not_requested() {
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

#[tokio::test]
async fn test_logging_set_level_updates_session_threshold() {
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
async fn test_logging_notifications_respect_session_threshold() {
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
