use axum::http::HeaderValue;
use axum_test::TestServer;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{handler::AppState, session::SessionManager};
use dcc_mcp_actions::{ActionDispatcher, ActionMeta, ActionRegistry};
use dcc_mcp_models::NextTools;
use dcc_mcp_skills::SkillCatalog;

fn make_state(next_tools: NextTools, with_handler: bool) -> AppState {
    let registry = Arc::new({
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "sample".into(),
            description: "sample tool".into(),
            dcc: "test_dcc".into(),
            version: "1.0.0".into(),
            next_tools,
            ..Default::default()
        });
        reg
    });
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    if with_handler {
        dispatcher.register_handler("sample", |_p| Ok(json!({"ok": true})));
    } else {
        dispatcher.register_handler("sample", |_p| Err("boom".to_string()));
    }
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

fn make_router(state: AppState) -> (axum::Router, SessionManager) {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};
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

async fn call_sample(router: axum::Router, sid: &str) -> Value {
    let server = TestServer::new(router);
    let resp = server
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
            "id": 1,
            "method": "tools/call",
            "params": {"name": "sample", "arguments": {}}
        }))
        .await;
    resp.assert_status_ok();
    resp.json()
}

#[tokio::test]
async fn success_attaches_on_success_list_only() {
    let nt = NextTools {
        on_success: vec!["foo__bar".into(), "baz__qux".into()],
        on_failure: vec!["debug__trace".into()],
    };
    let state = make_state(nt, true);
    let (router, sessions) = make_router(state);
    let sid = sessions.create();
    let body = call_sample(router, &sid).await;

    assert_eq!(body["result"]["isError"], false, "body: {body}");
    let meta = &body["result"]["_meta"]["dcc.next_tools"];
    assert!(
        meta.is_object(),
        "expected _meta.\"dcc.next_tools\" object on success, got: {body}",
    );
    let on_success = meta["on_success"].as_array().expect("on_success array");
    assert_eq!(on_success.len(), 2);
    assert_eq!(on_success[0], "foo__bar");
    assert!(
        meta.get("on_failure").is_none(),
        "on_failure must NOT be present on success results",
    );
}

#[tokio::test]
async fn failure_attaches_on_failure_list_only() {
    let nt = NextTools {
        on_success: vec!["foo__bar".into()],
        on_failure: vec!["diagnostics__screenshot".into()],
    };
    let state = make_state(nt, false);
    let (router, sessions) = make_router(state);
    let sid = sessions.create();
    let body = call_sample(router, &sid).await;

    assert_eq!(body["result"]["isError"], true, "body: {body}");
    let meta = &body["result"]["_meta"]["dcc.next_tools"];
    assert!(
        meta.is_object(),
        "expected _meta.\"dcc.next_tools\" on failure, got: {body}",
    );
    assert_eq!(
        meta["on_failure"][0], "diagnostics__screenshot",
        "on_failure list must surface",
    );
    assert!(
        meta.get("on_success").is_none(),
        "on_success must NOT be present on error results",
    );
}

#[tokio::test]
async fn no_next_tools_declared_omits_meta_entirely() {
    let state = make_state(NextTools::default(), true);
    let (router, sessions) = make_router(state);
    let sid = sessions.create();
    let body = call_sample(router, &sid).await;

    assert_eq!(body["result"]["isError"], false);
    assert!(
        body["result"].get("_meta").is_none(),
        "no next-tools means no _meta slot at all (got {body})",
    );
}

#[tokio::test]
async fn success_without_on_success_list_omits_meta() {
    // on-failure declared but the call succeeded — we must NOT
    // emit an empty on_success, and must NOT leak on_failure on a
    // success result.
    let nt = NextTools {
        on_success: vec![],
        on_failure: vec!["diagnostics__screenshot".into()],
    };
    let state = make_state(nt, true);
    let (router, sessions) = make_router(state);
    let sid = sessions.create();
    let body = call_sample(router, &sid).await;

    assert_eq!(body["result"]["isError"], false);
    assert!(
        body["result"].get("_meta").is_none(),
        "success result must not leak on_failure, got {body}",
    );
}
