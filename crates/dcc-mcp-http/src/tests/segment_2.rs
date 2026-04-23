use super::*;

        text.contains("nonexistent-uuid"),
        "error message must name the missing id, got: {text}"
    );
}

#[tokio::test]
async fn test_jobs_get_status_missing_job_id_param_is_error() {
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
            "method": "tools/call",
            "params": {
                "name": "jobs.get_status",
                "arguments": {}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.to_lowercase().contains("job_id"),
        "error text must name the missing parameter, got: {text}"
    );
}

#[tokio::test]
async fn test_jobs_get_status_returns_full_envelope_for_terminal_job() {
    use crate::job::JobProgress;

    let state = make_app_state();
    // Create + drive a job to completion through JobManager directly,
    // then invoke `jobs.get_status` via the full axum stack.
    let parent = state.jobs.create("workflow.run");
    let parent_id = parent.read().id.clone();
    let child = state
        .jobs
        .create_with_parent("workflow.step", Some(parent_id.clone()));
    let child_id = child.read().id.clone();
    state.jobs.start(&child_id).unwrap();
    state
        .jobs
        .update_progress(
            &child_id,
            JobProgress {
                current: 3,
                total: 10,
                message: Some("half-way".into()),
            },
        )
        .unwrap();
    state
        .jobs
        .complete(&child_id, json!({"ok": true, "value": 42}))
        .unwrap();

    let app = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::post(crate::handler::handle_post)
                .get(crate::handler::handle_get)
                .delete(crate::handler::handle_delete),
        )
        .with_state(state);
    let server = TestServer::new(app);

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "jobs.get_status",
                "arguments": {"job_id": child_id, "include_result": true}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let result = &body["result"];
    assert_eq!(result["isError"], false);
    let sc = &result["structuredContent"];
    assert_eq!(sc["job_id"], child_id);
    assert_eq!(sc["parent_job_id"], parent_id);
    assert_eq!(sc["tool"], "workflow.step");
    assert_eq!(sc["status"], "completed");
    assert!(sc["created_at"].is_string());
    assert!(sc["started_at"].is_string());
    assert!(sc["completed_at"].is_string());
    assert_eq!(sc["progress"]["current"], 3);
    assert_eq!(sc["progress"]["total"], 10);
    assert_eq!(sc["result"]["ok"], true);
    assert_eq!(sc["result"]["value"], 42);
}

#[tokio::test]
async fn test_jobs_get_status_include_result_false_omits_result() {
    let state = make_app_state();
    let job = state.jobs.create("t.x");
    let id = job.read().id.clone();
    state.jobs.start(&id).unwrap();
    state.jobs.complete(&id, json!({"v": 1})).unwrap();

    let app = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::post(crate::handler::handle_post)
                .get(crate::handler::handle_get)
                .delete(crate::handler::handle_delete),
        )
        .with_state(state);
    let server = TestServer::new(app);

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "jobs.get_status",
                "arguments": {"job_id": id, "include_result": false}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let sc = &body["result"]["structuredContent"];
    assert_eq!(sc["status"], "completed");
    assert!(
        sc.get("result").is_none(),
        "include_result=false must omit `result` key, got {sc}"
    );
}

#[tokio::test]
async fn test_jobs_get_status_running_job_has_no_result_yet() {
    let state = make_app_state();
    let job = state.jobs.create("t.slow");
    let id = job.read().id.clone();
    state.jobs.start(&id).unwrap();

    let app = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::post(crate::handler::handle_post)
                .get(crate::handler::handle_get)
                .delete(crate::handler::handle_delete),
        )
        .with_state(state);
    let server = TestServer::new(app);

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "jobs.get_status",
                "arguments": {"job_id": id, "include_result": true}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let sc = &body["result"]["structuredContent"];
    assert_eq!(sc["status"], "running");
    assert!(
        sc.get("result").is_none(),
        "running job must not have a `result` key even with include_result=true"
    );
    assert!(sc["started_at"].is_string());
    assert_eq!(sc["completed_at"], Value::Null);
}

// ── search_skills ─────────────────────────────────────────────────────────────

// Helper: build an app state that has skills in the catalog
fn make_app_state_with_skills() -> AppState {
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
    }
}

fn make_router_with_skills() -> axum::Router {
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
async fn test_search_skills_returns_match() {
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
async fn test_search_skills_matches_search_hint() {
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
async fn test_search_skills_matches_tool_name() {
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
async fn test_search_skills_no_match() {
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
async fn test_search_skills_empty_args_returns_discovery() {
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
async fn test_search_skills_limit_clamps_results() {
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
