use super::*;

// ── Real ActionDispatcher dispatch tests ──────────────────────────────

/// Helper: build an AppState with a dispatcher that has a real handler registered.
pub fn make_app_state_with_handler() -> AppState {
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
        registry_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        enable_tool_cache: true,
        method_router: crate::handler::AppState::default_method_router(),
    }
}

pub fn make_router_with_handler() -> axum::Router {
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
pub async fn test_tools_call_with_registered_handler() {
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
pub async fn test_tools_call_no_handler() {
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
pub async fn test_tools_call_handler_error() {
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
        registry_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        enable_tool_cache: true,
        method_router: crate::handler::AppState::default_method_router(),
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

// ── DeferredExecutor ──────────────────────────────────────────────────

#[tokio::test]
pub async fn test_deferred_executor_roundtrip() {
    use crate::executor::DeferredExecutor;

    let mut exec = DeferredExecutor::new(16);
    let handle = exec.handle();

    // Submit a task from tokio context, poll from "main thread"
    let task_handle = tokio::spawn(async move {
        handle
            .execute(Box::new(|| "hello from main thread".to_string()))
            .await
            .unwrap()
    });

    // Simulate DCC main thread polling
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    exec.poll_pending();

    let result = task_handle.await.unwrap();
    assert_eq!(result, "hello from main thread");
}

// ── Batch requests ─────────────────────────────────────────────────────

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
