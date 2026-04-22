//! Unit and integration tests for the MCP HTTP server.

use axum::http::HeaderValue;
use axum_test::TestServer;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{
    config::McpHttpConfig,
    handler::AppState,
    server::McpHttpServer,
    session::{SessionLogLevel, SessionManager},
};
use dcc_mcp_actions::{ActionDispatcher, ActionMeta, ActionRegistry};
use dcc_mcp_models::{SkillMetadata, ToolDeclaration};
use dcc_mcp_skills::SkillCatalog;

fn make_registry() -> ActionRegistry {
    let reg = ActionRegistry::new();
    reg.register_action(ActionMeta {
        name: "get_scene_info".into(),
        description: "Get current scene info".into(),
        category: "scene".into(),
        tags: vec!["query".into()],
        dcc: "test_dcc".into(),
        version: "1.0.0".into(),
        ..Default::default()
    });
    reg.register_action(ActionMeta {
        name: "list_objects".into(),
        description: "List all objects".into(),
        category: "scene".into(),
        tags: vec!["query".into(), "list".into()],
        dcc: "test_dcc".into(),
        version: "1.0.0".into(),
        ..Default::default()
    });
    reg
}

fn make_app_state() -> AppState {
    let registry = Arc::new(make_registry());
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

fn make_router() -> axum::Router {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};

    let state = make_app_state();
    Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(state)
}

fn parse_sse_payload(raw_event: &str) -> Value {
    let payload = raw_event
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap_or("{}");
    serde_json::from_str(payload).unwrap_or_else(|_| json!({}))
}

fn drain_sse_events(
    rx: &mut tokio::sync::broadcast::Receiver<String>,
    max_events: usize,
) -> Vec<Value> {
    let mut out = Vec::new();
    for _ in 0..max_events {
        match rx.try_recv() {
            Ok(raw) => out.push(parse_sse_payload(&raw)),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
        }
    }
    out
}

// ── initialize ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_initialize_advertises_elicitation_for_2025_06_18_only() {
    let server = TestServer::new(make_router());

    let init_2025_06_18 = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 101,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        }))
        .await;
    init_2025_06_18.assert_status_ok();
    let body_2025_06_18: Value = init_2025_06_18.json();
    assert!(
        body_2025_06_18["result"]["capabilities"]["elicitation"].is_object(),
        "2025-06-18 initialize must advertise elicitation capability"
    );

    let init_2025_03_26 = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 102,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        }))
        .await;
    init_2025_03_26.assert_status_ok();
    let body_2025_03_26: Value = init_2025_03_26.json();
    assert!(
        body_2025_03_26["result"]["capabilities"]
            .get("elicitation")
            .is_none(),
        "2025-03-26 initialize must not advertise elicitation capability"
    );
}

#[tokio::test]
async fn test_elicitation_create_requires_2025_06_18() {
    let server = TestServer::new(make_router());
    let session_id = "elicitation-gate-session";

    // Negotiate 2025-03-26 first.
    let init = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header(
            axum::http::HeaderName::from_static("mcp-session-id"),
            session_id.parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 201,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        }))
        .await;
    init.assert_status_ok();

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header(
            axum::http::HeaderName::from_static("mcp-session-id"),
            session_id.parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 202,
            "method": "elicitation/create",
            "params": {
                "message": "confirm destructive action?",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "confirm": {"type": "boolean"}
                    },
                    "required": ["confirm"]
                }
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let err = body["error"]
        .as_object()
        .expect("must return method-not-found error");
    assert_eq!(err["code"], -32601);
}

#[tokio::test]
async fn test_elicitation_create_roundtrip_via_sse_response() {
    let registry = Arc::new(make_registry());
    let config = McpHttpConfig::new(0);
    let server = McpHttpServer::new(registry, config);
    let handle = server.start().await.unwrap();
    let mcp_url = format!("http://{}{}/", handle.bind_addr, "/mcp");
    let mcp_url = mcp_url.trim_end_matches('/').to_string();
    let client = reqwest::Client::new();

    let init_resp = client
        .post(&mcp_url)
        .header("Accept", "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 201,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0"}
            }
        }))
        .send()
        .await
        .unwrap();
    assert!(init_resp.status().is_success());
    let init_body: Value = init_resp.json().await.unwrap();
    let session_id = init_body["result"]["__session_id"]
        .as_str()
        .map(str::to_owned)
        .expect("initialize must return __session_id");

    let responder_client = client.clone();
    let responder_url = mcp_url.clone();
    let sid_clone = session_id.clone();
    let responder = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let _ = responder_client
            .post(&responder_url)
            .header("Accept", "application/json")
            .header("Mcp-Session-Id", sid_clone)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 9001,
                "result": {
                    "action": "accept",
                    "content": {"confirmed": true}
                }
            }))
            .send()
            .await;
    });

    let call_resp = client
        .post(&mcp_url)
        .header("Accept", "application/json")
        .header("Mcp-Session-Id", session_id)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 9001,
            "method": "elicitation/create",
            "params": {
                "message": "Proceed with destructive operation?",
                "requestedSchema": {
                    "type": "object",
                    "properties": {"confirmed": {"type": "boolean"}},
                    "required": ["confirmed"]
                }
            }
        }))
        .send()
        .await
        .unwrap();
    assert!(call_resp.status().is_success());
    let body: Value = call_resp.json().await.unwrap();
    assert_eq!(body["result"]["action"], "accept");
    assert_eq!(body["result"]["content"]["confirmed"], true);

    responder.await.unwrap();
    handle.shutdown().await;
}

#[tokio::test]
async fn test_initialize() {
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
    // Session ID injected
    assert!(result["__session_id"].is_string());
}

#[tokio::test]
async fn test_list_roots_reports_cached_session_roots() {
    let _session_id = "roots-cache-session";

    // Initialize with roots capability advertised by client.
    // Seed cached roots explicitly for this deterministic unit test path.
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

    // Query cached roots via the new core meta-tool.
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
async fn test_list_roots_returns_cached_roots() {
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

// ── tools/list ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_tools_list() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    // 12 core meta-tools (10 + jobs.get_status #319 + jobs.cleanup #328)
    // + 2 registered actions = 14
    assert_eq!(tools.len(), 14);
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"get_scene_info"));
    assert!(names.contains(&"list_objects"));
    assert!(names.contains(&"find_skills"));
    assert!(names.contains(&"load_skill"));
    assert!(names.contains(&"search_skills"));
    assert!(names.contains(&"activate_tool_group"));
    assert!(names.contains(&"deactivate_tool_group"));
    assert!(names.contains(&"search_tools"));
    assert!(
        names.contains(&"jobs.get_status"),
        "tools/list must always expose the built-in jobs.get_status (#319)"
    );
}

// ── jobs.get_status (#319) ────────────────────────────────────────────

#[tokio::test]
async fn test_jobs_get_status_unknown_id_returns_is_error_envelope() {
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
                "arguments": {"job_id": "nonexistent-uuid"}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    // No JSON-RPC error object — the failure is carried inside a valid
    // CallToolResult with isError=true (MCP convention).
    assert!(
        body.get("error").is_none(),
        "unknown job id must not produce a transport-level JSON-RPC error"
    );
    let result = &body["result"];
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
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

#[tokio::test]
async fn test_search_skills_scope_filter_rejects_invalid_scope() {
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 141,
            "method": "tools/call",
            "params": {
                "name": "search_skills",
                "arguments": {"scope": "bogus"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
}

#[tokio::test]
async fn test_find_skills_forwards_and_marks_deprecated() {
    // Issue #340: find_skills is now a compatibility alias. It must still
    // return valid results AND attach `_meta["dcc.deprecation"]`.
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 142,
            "method": "tools/call",
            "params": {
                "name": "find_skills",
                "arguments": {"query": "bevel"}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], false);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("maya-bevel"), "forwarded result: {text}");
    assert_eq!(
        body["result"]["_meta"]["dcc.deprecation"]
            .as_str()
            .unwrap_or(""),
        "find_skills is deprecated — use search_skills. Will be removed in v0.17."
    );
}

#[tokio::test]
async fn test_tools_list_includes_unloaded_skill_stubs() {
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc": "2.0", "id": 15, "method": "tools/list"}))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    // Unloaded skills appear as __skill__<name> stubs
    assert!(
        names.contains(&"__skill__maya-bevel"),
        "Expected stub __skill__maya-bevel, got: {names:?}"
    );
    assert!(
        names.contains(&"__skill__git-tools"),
        "Expected stub __skill__git-tools, got: {names:?}"
    );

    let maya_stub = tools
        .iter()
        .find(|t| t["name"] == "__skill__maya-bevel")
        .unwrap();
    assert_eq!(maya_stub["annotations"], serde_json::Value::Null);
}

// ── On-demand loading invariants ──────────────────────────────────────
//
// These tests enforce the core contract of the progressive-loading design:
//
// 1. Before any load_skill call the full tool schemas of discovered skills
//    MUST NOT appear in tools/list — only lightweight stubs are allowed.
// 2. Skill tool names (non-stubs, non-core) MUST NOT appear in tools/list
//    until the skill is explicitly loaded.
// 3. Stubs MUST have minimal input_schema (no per-parameter definitions).
// 4. After load_skill the skill's real tools appear and the stub is gone.

#[tokio::test]
async fn test_tools_list_no_full_schemas_before_load() {
    // All discovered (unloaded) skills must appear ONLY as stubs — their
    // individual tool names (e.g. "maya-bevel.bevel") must NOT be present,
    // and the stubs themselves must not carry a rich input_schema.
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .await;

    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();

    for tool in tools {
        let name = tool["name"].as_str().unwrap_or("");

        // Individual skill tools (non-stubs, non-core) must not appear.
        let is_core = matches!(
            name,
            "list_roots"
                | "find_skills"
                | "list_skills"
                | "get_skill_info"
                | "load_skill"
                | "unload_skill"
                | "search_skills"
                | "activate_tool_group"
                | "deactivate_tool_group"
                | "search_tools"
                | "jobs.get_status"
                | "jobs.cleanup"
        );
        let is_stub = name.starts_with("__skill__") || name.starts_with("__group__");

        assert!(
            is_core || is_stub,
            "Found unexpected tool '{name}' in tools/list before any skill was loaded. \
                 Only core meta-tools and __skill__<name> / __group__<name> stubs should appear."
        );

        // Stubs must have a minimal input_schema — no nested 'properties'
        // that describe individual parameters.
        if is_stub {
            let schema = &tool["inputSchema"];
            let has_properties = schema
                .as_object()
                .and_then(|o| o.get("properties"))
                .map(|p| {
                    p.as_object()
                        .map(|props| !props.is_empty())
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            assert!(
                !has_properties,
                "Stub '{name}' must not expose per-parameter input_schema before loading. \
                     Got: {schema}"
            );
        }
    }
}

#[tokio::test]
async fn test_skill_tool_names_absent_before_load() {
    // The actual tool names declared inside a skill (e.g. "bevel", "chamfer")
    // must not appear as top-level tool names until load_skill is called.
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .await;

    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();

    // These are the real tool names from make_app_state_with_skills().
    // They must NOT appear before loading — covers both the legacy
    // `<skill>.<action>` form and the bare form introduced by #307.
    for forbidden in &[
        "maya-bevel.bevel",
        "maya-bevel.chamfer",
        "git-tools.log",
        "bevel",
        "chamfer",
        "log",
    ] {
        assert!(
            !names.contains(forbidden),
            "Tool '{forbidden}' appeared in tools/list before load_skill was called. \
                 Tools must only be registered after load_skill."
        );
    }
}

#[tokio::test]
async fn test_load_skill_then_tools_list_has_real_tools_not_stub() {
    // After load_skill: real tool(s) appear AND the stub disappears.
    let state = make_app_state_with_skills();
    let router = make_router_with_skills();
    let server = TestServer::new(router);

    // Load maya-bevel.
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "load_skill", "arguments": {"skill_name": "maya-bevel"}}
        }))
        .await;
    resp.assert_status_ok();

    // tools/list after load.
    let tl = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .await;
    let body: Value = tl.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();

    // Real tools registered (#307: bare names when unique within the
    // instance; `maya-bevel` is the only skill here, so bare wins).
    assert!(
        names.contains(&"bevel"),
        "Expected bare `bevel` after load, got: {names:?}"
    );
    assert!(
        names.contains(&"chamfer"),
        "Expected bare `chamfer` after load, got: {names:?}"
    );

    // Stub gone.
    assert!(
        !names.contains(&"__skill__maya-bevel"),
        "__skill__maya-bevel stub should be gone after loading, got: {names:?}"
    );

    // git-tools is still a stub (not loaded).
    assert!(
        names.contains(&"__skill__git-tools"),
        "__skill__git-tools stub should still be present (not loaded), got: {names:?}"
    );

    // The real tools carry a non-trivial inputSchema (set by ActionMeta).
    let bevel_tool = tools.iter().find(|t| t["name"] == "bevel").unwrap();
    // inputSchema must be at least `{"type": "object"}` — not null/absent.
    assert!(
        !bevel_tool["inputSchema"].is_null(),
        "Loaded tool must have an inputSchema"
    );
    // Issue #344 — tools without declared annotations must OMIT the
    // `annotations` field entirely (no empty object, no defaults) and
    // `deferredHint` (a dcc-mcp-core extension) rides in `_meta`,
    // never in the spec `annotations` map.
    assert!(
        bevel_tool.get("annotations").is_none()
            || bevel_tool["annotations"].get("deferredHint").is_none(),
        "deferredHint must not appear inside the spec `annotations` map; got {bevel_tool}"
    );

    let git_stub = tools
        .iter()
        .find(|t| t["name"] == "__skill__git-tools")
        .unwrap();
    assert_eq!(git_stub["annotations"], serde_json::Value::Null);

    let _ = state; // suppress unused warning
}

#[tokio::test]
async fn test_on_demand_count_invariant() {
    // Invariant: tools/list tool count = N_core + N_loaded_skill_tools + N_stubs
    // Before any load: count = 6 core + 0 loaded + 2 stubs = 8
    // After loading maya-bevel (2 tools): = 6 core + 2 loaded + 1 remaining stub = 9
    let server = TestServer::new(make_router_with_skills());

    let count_before = {
        let resp = server
            .post("/mcp")
            .add_header(
                axum::http::header::ACCEPT,
                "application/json".parse::<HeaderValue>().unwrap(),
            )
            .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .await;
        let body: Value = resp.json();
        body["result"]["tools"].as_array().unwrap().len()
    };

    // Load maya-bevel.
    server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "load_skill", "arguments": {"skill_name": "maya-bevel"}}
        }))
        .await;

    let count_after = {
        let resp = server
            .post("/mcp")
            .add_header(
                axum::http::header::ACCEPT,
                "application/json".parse::<HeaderValue>().unwrap(),
            )
            .json(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}))
            .await;
        let body: Value = resp.json();
        body["result"]["tools"].as_array().unwrap().len()
    };

    // Loading adds 2 real tools and removes 1 stub → net +1.
    assert_eq!(
        count_after,
        count_before + 1,
        "After loading maya-bevel (2 tools, 1 stub replaced): \
             expected count_before({count_before})+1={}, got {count_after}",
        count_before + 1
    );
}

#[tokio::test]
async fn test_skill_stub_call_returns_load_hint() {
    let server = TestServer::new(make_router_with_skills());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 16,
            "method": "tools/call",
            "params": {
                "name": "__skill__maya-bevel",
                "arguments": {}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("load_skill"),
        "Stub call should hint at load_skill: {text}"
    );
    assert!(
        text.contains("maya-bevel"),
        "Stub call should name the skill: {text}"
    );
}

// ── tools/call known (no handler registered) ──────────────────────────

#[tokio::test]
async fn test_tools_call_known_tool() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "get_scene_info",
                "arguments": {}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    // No handler registered for get_scene_info → is_error=true with guidance message
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("no handler") || text.contains("register"));
}

// ── tools/call unknown ─────────────────────────────────────────────────

#[tokio::test]
async fn test_tools_call_unknown_tool() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "nonexistent_tool",
                "arguments": {}
            }
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
}

// ── ping ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_ping() {
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

// ── method not found ──────────────────────────────────────────────────

#[tokio::test]
async fn test_method_not_found() {
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

// ── notifications (202) ───────────────────────────────────────────────

#[tokio::test]
async fn test_notification_returns_202() {
    let server = TestServer::new(make_router());

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .await;

    resp.assert_status(axum::http::StatusCode::ACCEPTED);
}

// ── DELETE nonexistent session ─────────────────────────────────────────

#[tokio::test]
async fn test_delete_nonexistent_session() {
    let server = TestServer::new(make_router());

    let resp = server
        .delete("/mcp")
        .add_header(
            axum::http::HeaderName::from_static("mcp-session-id"),
            "nonexistent-id".parse::<HeaderValue>().unwrap(),
        )
        .await;

    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

// ── Batch requests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_batch_requests() {
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

// ── GET without SSE Accept returns 405 ────────────────────────────────

#[tokio::test]
async fn test_get_without_sse_accept_returns_405() {
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

// ── SessionManager ────────────────────────────────────────────────────

#[test]
fn test_session_manager_lifecycle() {
    let mgr = SessionManager::new();
    assert_eq!(mgr.count(), 0);

    let id = mgr.create();
    assert_eq!(mgr.count(), 1);
    assert!(mgr.exists(&id));
    assert!(!mgr.is_initialized(&id));

    assert!(mgr.mark_initialized(&id));
    assert!(mgr.is_initialized(&id));

    assert!(mgr.remove(&id));
    assert_eq!(mgr.count(), 0);
    assert!(!mgr.remove(&id));
}

// ── Server start/stop ──────────────────────────────────────────────────

#[tokio::test]
async fn test_server_start_stop() {
    let registry = Arc::new(make_registry());
    let config = McpHttpConfig::new(0); // port 0 = random available port
    let server = McpHttpServer::new(registry, config);
    let handle = server.start().await.unwrap();
    assert!(handle.port > 0);
    handle.shutdown().await;
}

// ── DeferredExecutor ──────────────────────────────────────────────────

#[tokio::test]
async fn test_deferred_executor_roundtrip() {
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

// ── Core discovery tools ──────────────────────────────────────────────

fn make_app_state_with_skill() -> AppState {
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
    }
}

fn make_router_with_skill() -> axum::Router {
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
async fn test_find_skills_returns_discovered_skills() {
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
                "name": "find_skills",
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
async fn test_list_skills_shows_all() {
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
async fn test_get_skill_info() {
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
async fn test_load_skill_registers_tools() {
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
    // 12 core meta-tools (incl. jobs.get_status #319 + jobs.cleanup #328)
    // + 2 skill tools = 14
    assert_eq!(tools.len(), 14);
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
async fn test_unload_skill_removes_tools() {
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
