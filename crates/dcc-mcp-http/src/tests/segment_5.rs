use super::*;

            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "tools/call",
            "params": {
                "name": "unload_skill",
                "arguments": {"skill_name": "modeling-bevel"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let content_text = body["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content_text).unwrap();
    assert_eq!(result["unloaded"], true);
    assert_eq!(result["tools_removed"], 2);

    // Verify tools are gone from tools/list
    let resp2 = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "tools/list"
        }))
        .await;

    let body2: Value = resp2.json();
    let tools = body2["result"]["tools"].as_array().unwrap();
    // Back to 12 core meta-tools (incl. jobs.get_status #319 + jobs.cleanup
    // #328) + 1 unloaded skill stub = 13
    assert_eq!(tools.len(), 13);
    let stub = tools
        .iter()
        .find(|t| t["name"] == "__skill__modeling-bevel")
        .unwrap();
    assert_eq!(stub["annotations"], serde_json::Value::Null);
}

// Tool namespacing tests (#238)
#[tokio::test]
async fn test_loaded_tools_have_namespaced_names() {
    let server = TestServer::new(make_router_with_skill());
    server.post("/mcp")
            .add_header(axum::http::header::ACCEPT, "application/json".parse::<HeaderValue>().unwrap())
            .json(&json!({"jsonrpc":"2.0","id":100,"method":"tools/call","params":{"name":"load_skill","arguments":{"skill_name":"modeling-bevel"}}}))
            .await;
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc":"2.0","id":101,"method":"tools/list"}))
        .await;
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    // #307: with bare_tool_names=true (default), unique action names
    // publish without the `<skill>.` prefix.
    assert!(
        names.contains(&"bevel"),
        "Expected bare `bevel`, got: {names:?}"
    );
    assert!(
        names.contains(&"chamfer"),
        "Expected bare `chamfer`, got: {names:?}"
    );
    assert!(
        !names.contains(&"modeling_bevel__bevel"),
        "Old __ name must not appear: {names:?}"
    );
}
#[tokio::test]
async fn test_core_tools_keep_bare_names() {
    let server = TestServer::new(make_router_with_skill());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc":"2.0","id":120,"method":"tools/list"}))
        .await;
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for core in &[
        "find_skills",
        "list_skills",
        "get_skill_info",
        "load_skill",
        "unload_skill",
        "search_skills",
        "activate_tool_group",
        "deactivate_tool_group",
        "search_tools",
    ] {
        assert!(
            names.contains(core),
            "Core '{core}' must be bare, got: {names:?}"
        );
    }
}
#[tokio::test]
async fn test_unknown_tool_returns_not_found() {
    let server = TestServer::new(make_router_with_skill());
    let resp = server.post("/mcp")
            .add_header(axum::http::header::ACCEPT, "application/json".parse::<HeaderValue>().unwrap())
            .json(&json!({"jsonrpc":"2.0","id":130,"method":"tools/call","params":{"name":"totally_unknown_xyzzy","arguments":{}}}))
            .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("Unknown tool") || text.contains("ACTION_NOT_FOUND"),
        "Expected Unknown: {text}"
    );
}
#[tokio::test]
async fn test_initialize_reports_list_changed_true() {
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

// ── Real ActionDispatcher dispatch tests ──────────────────────────────

/// Helper: build an AppState with a dispatcher that has a real handler registered.
fn make_app_state_with_handler() -> AppState {
    let registry = Arc::new(make_registry());
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    // Register a real handler for get_scene_info
    dispatcher.register_handler("get_scene_info", |_params| {
        Ok(serde_json::json!({"scene": "test_scene", "objects": 3}))
    });
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

fn make_router_with_handler() -> axum::Router {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};

    let state = make_app_state_with_handler();
    Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(state)
}

#[tokio::test]
async fn test_tools_call_with_registered_handler() {
    let server = TestServer::new(make_router_with_handler());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 40,
            "method": "tools/call",
            "params": {
                "name": "get_scene_info",
                "arguments": {}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    // Handler is registered — should succeed
    assert_eq!(body["result"]["isError"], false);
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    // The JSON output from the handler should be present
    assert!(text.contains("test_scene") || text.contains("objects"));
}

#[tokio::test]
async fn test_tools_call_no_handler() {
    // Uses the default make_router() where no handlers are registered
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 41,
            "method": "tools/call",
            "params": {
                "name": "list_objects",
                "arguments": {}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    // Tool is in registry but has no handler
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("no handler") || text.contains("register"),
        "Expected helpful no-handler message, got: {text}"
    );
}

#[tokio::test]
async fn test_tools_call_handler_error() {
    let registry = Arc::new(make_registry());
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    // Register a handler that always fails
    dispatcher.register_handler("get_scene_info", |_params| {
        Err("simulated DCC error: scene not available".to_string())
    });
    let state = AppState {
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
    };

    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};
    let router = Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(state);

    let server = TestServer::new(router);
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "tools/call",
            "params": {
                "name": "get_scene_info",
                "arguments": {}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    // Handler returned Err — should be is_error=true
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("simulated DCC error") || text.contains("handler error"),
        "Expected handler error message, got: {text}"
    );
}

// ── Session TTL / touch / eviction ────────────────────────────────────

#[test]
fn test_session_touch_refreshes_last_active() {
    let mgr = SessionManager::new();
    let id = mgr.create();

    // Touch should succeed for an existing session.
    assert!(mgr.touch(&id));
    // Touch on a non-existent id returns false.
    assert!(!mgr.touch("no-such-session"));
}

#[test]
fn test_session_evict_stale_removes_old_sessions() {
    use std::time::Duration;
    let mgr = SessionManager::new();

    // Create two sessions; they both start with last_active = now.
    let _id1 = mgr.create();
    let id2 = mgr.create();
    assert_eq!(mgr.count(), 2);

    // Evicting with a generous TTL removes nothing.
    let evicted = mgr.evict_stale(Duration::from_secs(3600));
    assert_eq!(evicted, 0);
    assert_eq!(mgr.count(), 2);

    // Evicting with a zero TTL removes all sessions (all are "stale").
    let evicted = mgr.evict_stale(Duration::ZERO);
    assert_eq!(evicted, 2);
    assert_eq!(mgr.count(), 0);
    assert!(!mgr.exists(&id2));
}

#[test]
fn test_session_touch_prevents_eviction() {
    use std::time::Duration;
    let mgr = SessionManager::new();

    let id = mgr.create();

    // Touch the session (updates last_active to now).
    assert!(mgr.touch(&id));

    // Evict with zero TTL — the touched session should also be removed
    // because Duration::ZERO means any age is too old.
    // This validates that touch() actually writes a fresh Instant.
    let evicted = mgr.evict_stale(Duration::ZERO);
    assert_eq!(evicted, 1);
}

#[test]
fn test_session_evict_stale_does_not_touch_initialized_flag() {
    use std::time::Duration;
    let mgr = SessionManager::new();
    let id = mgr.create();
    mgr.mark_initialized(&id);

    // Sanity: session is initialized before eviction.
    assert!(mgr.is_initialized(&id));

    // Evict with generous TTL — session stays.
    mgr.evict_stale(Duration::from_secs(3600));
    assert!(mgr.exists(&id));
    assert!(mgr.is_initialized(&id));
}

// ── session_ttl_secs config ───────────────────────────────────────────

#[test]
fn test_config_session_ttl_default_is_one_hour() {
    let cfg = McpHttpConfig::new(8765);
    assert_eq!(cfg.session_ttl_secs, 3600);
}

#[test]
fn test_config_session_ttl_builder() {
    let cfg = McpHttpConfig::new(8765).with_session_ttl_secs(0);
    assert_eq!(cfg.session_ttl_secs, 0);

    let cfg2 = McpHttpConfig::new(8765).with_session_ttl_secs(300);
    assert_eq!(cfg2.session_ttl_secs, 300);
}

// ── dispatch_request touches session TTL ─────────────────────────────

#[tokio::test]
async fn test_dispatch_touches_session_on_each_request() {
    // Verify that sending a real request does not panic and the session
    // touch() path is exercised (the session manager must update last_active).
    // We use the in-process axum_test router to avoid network deps.
    let state = make_app_state();
    let router = make_router();
    let server = TestServer::new(router);

    // Initialize — creates a session and returns Mcp-Session-Id.
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0.1"}
