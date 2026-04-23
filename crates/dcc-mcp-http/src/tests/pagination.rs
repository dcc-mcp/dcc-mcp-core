use super::*;

// ── tools/list pagination ─────────────────────────────────────────────

pub fn make_app_state_many_tools() -> AppState {
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

pub fn make_router_many_tools() -> axum::Router {
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
pub async fn test_tools_list_pagination_first_page() {
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
pub async fn test_tools_list_pagination_second_page() {
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
pub async fn test_tools_list_all_pages_no_duplicates() {
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
pub async fn test_tools_list_no_cursor_for_small_list() {
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
