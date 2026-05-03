use axum::http::HeaderValue;
use axum_test::TestServer;
use dcc_mcp_actions::ActionDispatcher;
use dcc_mcp_actions::registry::{ActionMeta, ActionRegistry};
use dcc_mcp_skills::SkillCatalog;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::handler::AppState;
use crate::session::SessionManager;

/// Build an AppState with the fast-path enabled and two sample actions:
/// one bare, one skill-prefixed. Both have dispatch handlers so we can
/// exercise `call_action` end-to-end.
fn make_state(lazy_actions: bool) -> AppState {
    let registry = Arc::new({
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "create_sphere".into(),
            description: "Create a sphere".into(),
            category: "geometry".into(),
            tags: vec!["geo".into(), "prim".into()],
            dcc: "maya".into(),
            version: "1.0.0".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"radius": {"type": "number"}}
            }),
            ..Default::default()
        });
        reg.register_action(ActionMeta {
            name: "hello_world.greet".into(),
            description: "Say hi".into(),
            category: "demo".into(),
            tags: vec!["demo".into()],
            dcc: "maya".into(),
            version: "1.0.0".into(),
            skill_name: Some("hello_world".into()),
            ..Default::default()
        });
        reg
    });
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    dispatcher.register_handler("create_sphere", |p| {
        let r = p.get("radius").and_then(Value::as_f64).unwrap_or(1.0);
        Ok(json!({"name": "|pSphere1", "radius": r}))
    });
    dispatcher.register_handler("hello_world.greet", |_p| Ok(json!("hi")));
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
        lazy_actions,
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

fn make_router(lazy_actions: bool) -> (axum::Router, SessionManager) {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};
    let state = make_state(lazy_actions);
    let sessions = state.sessions.clone();
    let router = Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(state);
    (router, sessions)
}

async fn call(server: &TestServer, session_id: &str, body: Value) -> Value {
    server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .add_header(
            axum::http::HeaderName::from_static("mcp-session-id"),
            session_id.parse::<HeaderValue>().unwrap(),
        )
        .json(&body)
        .await
        .json()
}

#[tokio::test]
async fn meta_tools_absent_when_disabled() {
    let (router, sessions) = make_router(false);
    let sid = sessions.create();
    sessions.set_protocol_version(&sid, "2025-06-18");
    let server = TestServer::new(router);
    let body = call(
        &server,
        &sid,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    let tools = body["result"]["tools"].as_array().unwrap();
    for name in ["list_actions", "describe_action", "call_action"] {
        assert!(
            tools.iter().all(|t| t["name"] != name),
            "meta-tool {name} must be hidden when lazy_actions is disabled"
        );
    }
}

#[tokio::test]
async fn meta_tools_present_when_enabled() {
    let (router, sessions) = make_router(true);
    let sid = sessions.create();
    sessions.set_protocol_version(&sid, "2025-06-18");
    let server = TestServer::new(router);
    let body = call(
        &server,
        &sid,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    let tools = body["result"]["tools"].as_array().unwrap();
    for name in ["list_actions", "describe_action", "call_action"] {
        assert!(
            tools.iter().any(|t| t["name"] == name),
            "meta-tool {name} must appear when lazy_actions is enabled, got: {tools:?}"
        );
    }
}

#[tokio::test]
async fn list_actions_omits_schema_body() {
    let (router, sessions) = make_router(true);
    let sid = sessions.create();
    sessions.set_protocol_version(&sid, "2025-06-18");
    let server = TestServer::new(router);
    let body = call(
        &server,
        &sid,
        json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {"name": "list_actions", "arguments": {}}
        }),
    )
    .await;
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    let actions = payload["actions"].as_array().unwrap();
    assert_eq!(
        actions.len(),
        2,
        "expected both sample actions, got: {actions:?}"
    );
    for a in actions {
        // Contract: compact triple only. Flagging inputSchema / outputSchema
        // leakage here is the whole point of the fast-path benchmark.
        assert!(a.get("inputSchema").is_none());
        assert!(a.get("input_schema").is_none());
        assert!(a.get("outputSchema").is_none());
        assert!(a["id"].is_string());
        assert!(a["summary"].is_string());
        assert!(a["tags"].is_array());
    }
    // `hello_world.greet` must round-trip as its canonical skill-prefixed id.
    assert!(
        actions.iter().any(|a| a["id"] == "hello_world.greet"),
        "skill-prefixed id must be surfaced verbatim, got: {actions:?}"
    );
}

#[tokio::test]
async fn describe_action_matches_tools_list_schema() {
    let (router, sessions) = make_router(true);
    let sid = sessions.create();
    sessions.set_protocol_version(&sid, "2025-06-18");
    let server = TestServer::new(router);

    // Fetch the same action through `tools/list` for a reference.
    let list_body = call(
        &server,
        &sid,
        json!({"jsonrpc": "2.0", "id": 20, "method": "tools/list"}),
    )
    .await;
    let ref_tool = list_body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "create_sphere")
        .cloned()
        .expect("create_sphere must be in tools/list");

    // Same action through describe_action.
    let desc_body = call(
        &server,
        &sid,
        json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "tools/call",
            "params": {
                "name": "describe_action",
                "arguments": {"id": "create_sphere"}
            }
        }),
    )
    .await;
    let desc_text = desc_body["result"]["content"][0]["text"].as_str().unwrap();
    let desc_tool: Value = serde_json::from_str(desc_text).unwrap();

    assert_eq!(
        desc_tool, ref_tool,
        "describe_action must produce the exact same shape as tools/list"
    );
}

#[tokio::test]
async fn describe_action_rejects_unknown_id() {
    let (router, sessions) = make_router(true);
    let sid = sessions.create();
    let server = TestServer::new(router);
    let body = call(
        &server,
        &sid,
        json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "tools/call",
            "params": {
                "name": "describe_action",
                "arguments": {"id": "no_such_action"}
            }
        }),
    )
    .await;
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("ACTION_NOT_FOUND"),
        "expected ACTION_NOT_FOUND envelope, got: {text}"
    );
}

#[tokio::test]
async fn call_action_dispatches_to_underlying_handler() {
    let (router, sessions) = make_router(true);
    let sid = sessions.create();
    sessions.set_protocol_version(&sid, "2025-06-18");
    let server = TestServer::new(router);
    let body = call(
        &server,
        &sid,
        json!({
            "jsonrpc": "2.0",
            "id": 40,
            "method": "tools/call",
            "params": {
                "name": "call_action",
                "arguments": {
                    "id": "create_sphere",
                    "args": {"radius": 3.0}
                }
            }
        }),
    )
    .await;
    assert_eq!(body["result"]["isError"], false, "body: {body}");
    // Single dispatch path: the underlying handler ran and returned
    // the exact same payload a direct tools/call would have produced.
    let sc = &body["result"]["structuredContent"];
    assert_eq!(sc["radius"], 3.0);
    assert_eq!(sc["name"], "|pSphere1");
}

#[tokio::test]
async fn call_action_refuses_meta_recursion() {
    let (router, sessions) = make_router(true);
    let sid = sessions.create();
    let server = TestServer::new(router);
    let body = call(
        &server,
        &sid,
        json!({
            "jsonrpc": "2.0",
            "id": 50,
            "method": "tools/call",
            "params": {
                "name": "call_action",
                "arguments": {"id": "call_action", "args": {}}
            }
        }),
    )
    .await;
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("RECURSIVE_META_CALL"),
        "expected RECURSIVE_META_CALL envelope, got: {text}"
    );
}

#[tokio::test]
async fn disabled_fast_path_rejects_meta_tool_calls() {
    // With lazy_actions=false, the three meta-tool names must fall
    // through to the generic action resolver → ACTION_NOT_FOUND.
    let (router, sessions) = make_router(false);
    let sid = sessions.create();
    let server = TestServer::new(router);
    let body = call(
        &server,
        &sid,
        json!({
            "jsonrpc": "2.0",
            "id": 60,
            "method": "tools/call",
            "params": {"name": "list_actions", "arguments": {}}
        }),
    )
    .await;
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("ACTION_NOT_FOUND") || text.contains("Unknown tool"));
}
