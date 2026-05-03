use super::*;

// ── search_skills ─────────────────────────────────────────────────────

// Helper: build an app state that has skills in the catalog
pub fn make_app_state_with_skills() -> AppState {
    use dcc_mcp_models::ToolDeclaration;
    let registry = Arc::new(ActionRegistry::new());
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));

    // Add a skill with search_hint
    let mut skill = SkillMetadata {
        name: "maya-bevel".to_string(),
        description: "Polygon bevel and chamfer tools".to_string(),
        search_hint: "polygon modeling, bevel, chamfer, extrude".to_string(),
        dcc: "maya".to_string(),
        tools: vec![
            ToolDeclaration {
                name: "bevel".to_string(),
                description: "Apply bevel to edges".to_string(),
                ..Default::default()
            },
            ToolDeclaration {
                name: "chamfer".to_string(),
                description: "Chamfer vertices".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    skill.tags = vec!["modeling".to_string()];
    catalog.add_skill(skill);

    // Add a second unrelated skill
    let mut skill2 = SkillMetadata {
        name: "git-tools".to_string(),
        description: "Git version control helpers".to_string(),
        search_hint: "git, commit, branch, vcs".to_string(),
        dcc: "python".to_string(),
        tools: vec![ToolDeclaration {
            name: "log".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    skill2.tags = vec!["devops".to_string()];
    catalog.add_skill(skill2);

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
        registry_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        enable_tool_cache: true,
        method_router: crate::handler::AppState::default_method_router(),
        readiness: crate::handler::AppState::default_readiness(),
    }
}

pub fn make_router_with_skills() -> axum::Router {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};
    Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(make_app_state_with_skills())
}

#[tokio::test]
pub async fn test_search_skills_returns_match() {
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "search_skills",
                "arguments": {"query": "bevel"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], false);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("maya-bevel"),
        "Expected maya-bevel in results: {text}"
    );
    assert!(
        !text.contains("git-tools"),
        "git-tools should not match 'bevel': {text}"
    );
}

#[tokio::test]
pub async fn test_search_skills_matches_search_hint() {
    let server = TestServer::new(make_router_with_skills());

    // "chamfer" is only in search_hint, not in description or name
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "search_skills",
                "arguments": {"query": "chamfer"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("maya-bevel"),
        "search_hint match expected for 'chamfer': {text}"
    );
}

#[tokio::test]
pub async fn test_search_skills_matches_tool_name() {
    let server = TestServer::new(make_router_with_skills());

    // "log" is a tool name in git-tools
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "search_skills",
                "arguments": {"query": "log"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("git-tools"),
        "tool name match expected for 'log': {text}"
    );
}

#[tokio::test]
pub async fn test_search_skills_no_match() {
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {
                "name": "search_skills",
                "arguments": {"query": "xyzzy_no_match"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("No skills found"),
        "Expected 'No skills found' for unmatched query: {text}"
    );
}

#[tokio::test]
pub async fn test_search_skills_empty_args_returns_discovery() {
    // Issue #340: empty-args search_skills is a discovery call, not an error.
    // Returns the top skills sorted by scope precedence.
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "tools/call",
            "params": {
                "name": "search_skills",
                "arguments": {}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], false);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    // Discovery mode surfaces every discovered skill in the test fixture.
    assert!(
        text.contains("maya-bevel") && text.contains("git-tools"),
        "Expected discovery to list all skills: {text}"
    );
}

#[tokio::test]
pub async fn test_search_skills_limit_clamps_results() {
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 140,
            "method": "tools/call",
            "params": {
                "name": "search_skills",
                "arguments": {"limit": 1}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    assert_eq!(payload["skills"].as_array().unwrap().len(), 1);
    assert_eq!(payload["total"], 1);
}
