use axum::http::HeaderValue;
use axum_test::TestServer;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{handler::AppState, session::SessionManager};
use dcc_mcp_actions::{ActionDispatcher, ActionMeta, ActionRegistry};
use dcc_mcp_skills::SkillCatalog;

// ── ResourceLink (#243) — 2025-06-18 artifact surfacing ───────────────
//
// On MCP 2025-06-18 sessions, tools/call results that include
// `artifact_paths` / `artifacts` / `artifact_path` must surface them as
// `resource_link` content items. On 2025-03-26 sessions the text fallback
// is preserved (no resource_link content).

fn make_app_state_with_artifact_handler() -> AppState {
    let registry = Arc::new({
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "playblast".into(),
            description: "Render a playblast".into(),
            category: "render".into(),
            tags: vec!["render".into()],
            dcc: "test_dcc".into(),
            version: "1.0.0".into(),
            ..Default::default()
        });
        reg
    });
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    dispatcher.register_handler("playblast", |_params| {
        Ok(json!({
            "frame_count": 24,
            "artifact_paths": ["/tmp/shot_010.mp4"]
        }))
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
        readiness: crate::handler::AppState::default_readiness(),
    }
}

fn make_router_with_artifact_handler() -> (axum::Router, SessionManager) {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};
    let state = make_app_state_with_artifact_handler();
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

#[tokio::test]
async fn test_resource_link_emitted_on_2025_06_18_session() {
    let (router, sessions) = make_router_with_artifact_handler();
    let session_id = sessions.create();
    sessions.set_protocol_version(&session_id, "2025-06-18");

    let server = TestServer::new(router);
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
            "id": 200,
            "method": "tools/call",
            "params": {"name": "playblast", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], false, "body = {body}");

    let content = body["result"]["content"].as_array().unwrap();
    // First item is the text summary.
    assert_eq!(content[0]["type"], "text");
    // Second item must be the resource_link for the artifact.
    let link = content.iter().find(|c| c["type"] == "resource_link");
    assert!(
        link.is_some(),
        "Expected a resource_link content item on 2025-06-18, got: {content:?}"
    );
    let link = link.unwrap();
    assert_eq!(link["uri"], "file:///tmp/shot_010.mp4");
    assert_eq!(link["mimeType"], "video/mp4");
    assert_eq!(link["name"], "shot_010.mp4");
}

#[tokio::test]
async fn test_resource_link_suppressed_on_2025_03_26_session() {
    let (router, sessions) = make_router_with_artifact_handler();
    let session_id = sessions.create();
    sessions.set_protocol_version(&session_id, "2025-03-26");

    let server = TestServer::new(router);
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
            "id": 201,
            "method": "tools/call",
            "params": {"name": "playblast", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let content = body["result"]["content"].as_array().unwrap();
    assert!(
        content.iter().all(|c| c["type"] != "resource_link"),
        "resource_link must NOT appear on 2025-03-26 sessions, got: {content:?}"
    );
    // Text fallback still carries the full JSON payload including the path.
    let text = content[0]["text"].as_str().unwrap();
    assert!(text.contains("/tmp/shot_010.mp4"));
}

#[tokio::test]
async fn test_resource_link_suppressed_when_session_header_absent() {
    let (router, _sessions) = make_router_with_artifact_handler();
    let server = TestServer::new(router);
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 202,
            "method": "tools/call",
            "params": {"name": "playblast", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let content = body["result"]["content"].as_array().unwrap();
    assert!(content.iter().all(|c| c["type"] != "resource_link"));
}

// ── structuredContent + outputSchema (#242) — 2025-06-18 ─────────────
//
// On 2025-06-18 sessions:
//   * ``tools/list`` must advertise ``outputSchema`` for actions that
//     declared one
//   * ``tools/call`` must populate ``structuredContent`` when the dispatch
//     returns a JSON object / array
// On 2025-03-26 sessions both fields must be completely absent.

fn make_app_state_with_structured_handler() -> AppState {
    let registry = Arc::new({
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "list_selected_nodes".into(),
            description: "Return selected scene nodes".into(),
            category: "scene".into(),
            tags: vec!["scene".into()],
            dcc: "test_dcc".into(),
            version: "1.0.0".into(),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "nodes": {"type": "array", "items": {"type": "string"}},
                    "count": {"type": "integer"}
                },
                "required": ["nodes", "count"]
            }),
            ..Default::default()
        });
        // Second tool that returns a plain string — must NOT get
        // structuredContent even on 2025-06-18.
        reg.register_action(ActionMeta {
            name: "greet".into(),
            description: "Plain-text hello".into(),
            category: "demo".into(),
            dcc: "test_dcc".into(),
            version: "1.0.0".into(),
            ..Default::default()
        });
        reg
    });
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    dispatcher.register_handler("list_selected_nodes", |_p| {
        Ok(json!({"nodes": ["|pSphere1", "|pCube1"], "count": 2}))
    });
    dispatcher.register_handler("greet", |_p| Ok(json!("hi there")));
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

fn make_router_with_structured_handler() -> (axum::Router, SessionManager) {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};
    let state = make_app_state_with_structured_handler();
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

#[tokio::test]
async fn test_output_schema_emitted_on_2025_06_18_tools_list() {
    let (router, sessions) = make_router_with_structured_handler();
    let session_id = sessions.create();
    sessions.set_protocol_version(&session_id, "2025-06-18");

    let server = TestServer::new(router);
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
        .json(&json!({"jsonrpc": "2.0", "id": 300, "method": "tools/list"}))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();

    let list_nodes = tools
        .iter()
        .find(|t| t["name"] == "list_selected_nodes")
        .expect("list_selected_nodes missing from tools/list");
    let schema = list_nodes
        .get("outputSchema")
        .expect("outputSchema must be emitted on 2025-06-18 for tools that declared one");
    assert_eq!(schema["type"], "object");
    assert_eq!(
        schema["required"],
        json!(["nodes", "count"]),
        "schema round-trip lost ``required`` array"
    );

    // Tool with no declared schema must not get a null / empty outputSchema;
    // the field must be absent.
    let greet = tools.iter().find(|t| t["name"] == "greet").unwrap();
    assert!(
        greet.get("outputSchema").is_none(),
        "undeclared outputSchema must be omitted, got: {greet:?}"
    );
}

#[tokio::test]
async fn test_output_schema_omitted_on_2025_03_26_tools_list() {
    let (router, sessions) = make_router_with_structured_handler();
    let session_id = sessions.create();
    sessions.set_protocol_version(&session_id, "2025-03-26");

    let server = TestServer::new(router);
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
        .json(&json!({"jsonrpc": "2.0", "id": 301, "method": "tools/list"}))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    let tools = body["result"]["tools"].as_array().unwrap();
    for t in tools {
        assert!(
            t.get("outputSchema").is_none(),
            "outputSchema must be stripped on 2025-03-26, but {} carried it",
            t["name"]
        );
    }
}

#[tokio::test]
async fn test_structured_content_emitted_on_2025_06_18_call() {
    let (router, sessions) = make_router_with_structured_handler();
    let session_id = sessions.create();
    sessions.set_protocol_version(&session_id, "2025-06-18");

    let server = TestServer::new(router);
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
            "id": 302,
            "method": "tools/call",
            "params": {"name": "list_selected_nodes", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], false, "body = {body}");

    // structuredContent must mirror the dispatch payload verbatim.
    let sc = body["result"]
        .get("structuredContent")
        .expect("structuredContent must be present on 2025-06-18");
    assert_eq!(sc["nodes"], json!(["|pSphere1", "|pCube1"]));
    assert_eq!(sc["count"], 2);

    // Text fallback is still present for legacy display.
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("pSphere1"));
}

#[tokio::test]
async fn test_structured_content_omitted_on_2025_03_26_call() {
    let (router, sessions) = make_router_with_structured_handler();
    let session_id = sessions.create();
    sessions.set_protocol_version(&session_id, "2025-03-26");

    let server = TestServer::new(router);
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
            "id": 303,
            "method": "tools/call",
            "params": {"name": "list_selected_nodes", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(
        body["result"].get("structuredContent").is_none(),
        "structuredContent must not appear on 2025-03-26, got: {}",
        body["result"]
    );
    // The text fallback must still carry the JSON.
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("pSphere1"));
}

#[tokio::test]
async fn test_structured_content_omitted_for_string_output() {
    let (router, sessions) = make_router_with_structured_handler();
    let session_id = sessions.create();
    sessions.set_protocol_version(&session_id, "2025-06-18");

    let server = TestServer::new(router);
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
            "id": 304,
            "method": "tools/call",
            "params": {"name": "greet", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(
        body["result"].get("structuredContent").is_none(),
        "structuredContent must not wrap a plain string payload, got: {}",
        body["result"]
    );
    assert_eq!(body["result"]["content"][0]["text"], "hi there");
}
