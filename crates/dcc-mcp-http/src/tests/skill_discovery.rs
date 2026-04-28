use super::*;
// ── Core discovery tools ──────────────────────────────────────────────
pub fn make_app_state_with_skill() -> AppState {
    let registry = Arc::new(ActionRegistry::new());
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));

    // Add a test skill to the catalog
    catalog.add_skill(SkillMetadata {
        name: "modeling-bevel".to_string(),
        description: "Advanced bevel operations for polygon modeling".to_string(),
        tools: vec![
            ToolDeclaration {
                name: "bevel".to_string(),
                description: "Apply bevel to selected edges".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "offset": {"type": "number"},
                        "segments": {"type": "integer"}
                    }
                }),
                ..Default::default()
            },
            ToolDeclaration {
                name: "chamfer".to_string(),
                description: "Apply chamfer bevel".to_string(),
                ..Default::default()
            },
        ],
        dcc: "maya".to_string(),
        tags: vec!["modeling".to_string(), "polygon".to_string()],
        version: "1.0.0".to_string(),
        ..Default::default()
    });

    AppState {
        registry: registry.clone(),
        dispatcher: Arc::new(ActionDispatcher::new((*registry).clone())),
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

pub fn make_router_with_skill() -> axum::Router {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};

    let state = make_app_state_with_skill();
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
pub async fn test_search_skills_returns_discovered_skills() {
    let server = TestServer::new(make_router_with_skill());

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
    let content_text = body["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content_text).unwrap();
    assert_eq!(result["total"], 1);
    assert_eq!(result["skills"][0]["name"], "modeling-bevel");
    assert_eq!(result["skills"][0]["loaded"], false);
}

#[tokio::test]
pub async fn test_list_skills_shows_all() {
    let server = TestServer::new(make_router_with_skill());

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
                "name": "list_skills"
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let content_text = body["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content_text).unwrap();
    assert_eq!(result["total"], 1);
}

#[tokio::test]
pub async fn test_get_skill_info() {
    let server = TestServer::new(make_router_with_skill());

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
                "name": "get_skill_info",
                "arguments": {"skill_name": "modeling-bevel"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let content_text = body["result"]["content"][0]["text"].as_str().unwrap();
    let info: Value = serde_json::from_str(content_text).unwrap();
    assert_eq!(info["name"], "modeling-bevel");
    assert_eq!(info["tools"].as_array().unwrap().len(), 2);
    assert_eq!(info["state"], "discovered");
}

#[tokio::test]
pub async fn test_load_skill_registers_tools() {
    let server = TestServer::new(make_router_with_skill());

    // Load the skill
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
                "name": "load_skill",
                "arguments": {"skill_name": "modeling-bevel"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let content_text = body["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content_text).unwrap();
    assert_eq!(result["loaded"], true);
    assert_eq!(result["tool_count"], 2);

    // Verify tools are now in tools/list
    let resp2 = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "tools/list"
        }))
        .await;

    let body2: Value = resp2.json();
    let tools = body2["result"]["tools"].as_array().unwrap();
    // 14 core meta-tools (11 + register_tool/deregister_tool/list_dynamic_tools #462)
    // + 2 skill tools = 16
    assert_eq!(tools.len(), 16);
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    // #307: bare names when unique within the instance.
    assert!(names.contains(&"bevel"));
    assert!(names.contains(&"chamfer"));

    let bevel_tool = tools.iter().find(|t| t["name"] == "bevel").unwrap();
    // Issue #344 — deferredHint lives in `_meta["dcc.deferred_hint"]`,
    // never inside the spec `annotations` map. A tool with no declared
    // annotations and no async/timeout hint should omit both fields.
    assert!(
        bevel_tool.get("annotations").is_none()
            || bevel_tool["annotations"].get("deferredHint").is_none(),
        "deferredHint must not appear inside the spec `annotations` map"
    );
}

#[tokio::test]
pub async fn test_unload_skill_removes_tools() {
    let server = TestServer::new(make_router_with_skill());

    // Load first
    server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {
                "name": "load_skill",
                "arguments": {"skill_name": "modeling-bevel"}
            }
        }))
        .await;

    // Unload
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
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
    // Back to 14 core meta-tools (11 + register_tool/deregister_tool/list_dynamic_tools #462)
    // + 1 unloaded skill stub = 15
    assert_eq!(tools.len(), 15);
    let stub = tools
        .iter()
        .find(|t| t["name"] == "__skill__modeling-bevel")
        .unwrap();
    assert_eq!(stub["annotations"], serde_json::Value::Null);
}
