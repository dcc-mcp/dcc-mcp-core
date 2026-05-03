//! Issue #714 — the MCP `tools/call` handler MUST consult the same
//! [`ReadinessProbe`] as the REST `POST /v1/call` surface, so a backend
//! that is still booting refuses work up-front instead of silently
//! queuing it on `DeferredExecutor` / `QueueDispatcher` until the
//! gateway's deadline trips.

use super::*;
use std::sync::Arc;

use dcc_mcp_skill_rest::StaticReadiness;

/// Build an [`AppState`] whose dispatcher has `get_scene_info` wired,
/// returning an [`Arc<StaticReadiness>`] so individual tests can flip
/// the probe between red and green between requests.
fn make_state_with_probe() -> (AppState, Arc<StaticReadiness>) {
    let registry = Arc::new(make_registry());
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    dispatcher.register_handler("get_scene_info", |_params| {
        Ok(serde_json::json!({"scene": "test_scene", "objects": 3}))
    });

    let probe = Arc::new(StaticReadiness::new());
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
        readiness: probe.clone(),
    };
    (state, probe)
}

fn make_router_with_probe() -> (axum::Router, Arc<StaticReadiness>) {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};

    let (state, probe) = make_state_with_probe();
    let router = Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(state);
    (router, probe)
}

fn accept_json() -> HeaderValue {
    "application/json".parse::<HeaderValue>().unwrap()
}

// ── Red probe refuses DCC-touching tools ──────────────────────────────

#[tokio::test]
pub async fn red_probe_refuses_tools_call_with_backend_not_ready() {
    let (router, _probe) = make_router_with_probe();
    let server = TestServer::new(router);

    let resp = server
        .post("/mcp")
        .add_header(axum::http::header::ACCEPT, accept_json())
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 100,
            "method": "tools/call",
            "params": {"name": "get_scene_info", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok(); // JSON-RPC error still uses HTTP 200
    let body: Value = resp.json();

    // No result, structured error with the BACKEND_NOT_READY code
    assert!(
        body.get("result").is_none(),
        "expected no result, got {body}"
    );
    let err = body.get("error").expect("error envelope");
    assert_eq!(err["code"], json!(-32002), "BACKEND_NOT_READY code");
    let data = err.get("data").expect("data payload");
    assert_eq!(data["tool"], json!("get_scene_info"));
    // Default StaticReadiness::new() is process=true, dispatcher=false, dcc=false
    assert_eq!(data["readiness"]["process"], json!(true));
    assert_eq!(data["readiness"]["dispatcher"], json!(false));
    assert_eq!(data["readiness"]["dcc"], json!(false));
}

// ── Control tools bypass the gate ─────────────────────────────────────

#[tokio::test]
pub async fn red_probe_still_allows_discovery_tools() {
    let (router, _probe) = make_router_with_probe();
    let server = TestServer::new(router);

    // list_skills is a core control tool — should succeed even when
    // the probe is red, because an agent needs discovery during DCC
    // boot.
    let resp = server
        .post("/mcp")
        .add_header(axum::http::header::ACCEPT, accept_json())
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 101,
            "method": "tools/call",
            "params": {"name": "list_skills", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(
        body.get("error").is_none(),
        "list_skills must bypass the readiness gate; got error: {body}"
    );
    assert!(body.get("result").is_some());
}

// ── Flipping the probe green restores dispatch ────────────────────────

#[tokio::test]
pub async fn green_probe_permits_tools_call() {
    let (router, probe) = make_router_with_probe();
    let server = TestServer::new(router);

    // Flip both bits so the probe reports fully ready.
    probe.set_dispatcher_ready(true);
    probe.set_dcc_ready(true);

    let resp = server
        .post("/mcp")
        .add_header(axum::http::header::ACCEPT, accept_json())
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 102,
            "method": "tools/call",
            "params": {"name": "get_scene_info", "arguments": {}}
        }))
        .await;

    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(body.get("error").is_none(), "expected success, got {body}");
    let result = body.get("result").expect("result envelope");
    // Handler returned {"scene":"test_scene","objects":3}; the MCP
    // content envelope serialises that in the text field.
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("test_scene"),
        "expected handler payload, got: {text}"
    );
}

// ── No JobManager / queue side-effects when red ───────────────────────

#[tokio::test]
pub async fn red_probe_does_not_queue_on_job_manager() {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};

    let (state, _probe) = make_state_with_probe();
    let jobs = state.jobs.clone();
    let server = TestServer::new(
        Router::new()
            .route(
                "/mcp",
                routing::post(handle_post)
                    .get(handle_get)
                    .delete(handle_delete),
            )
            .with_state(state),
    );

    let before = jobs.list().len();
    let resp = server
        .post("/mcp")
        .add_header(axum::http::header::ACCEPT, accept_json())
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 103,
            "method": "tools/call",
            "params": {
                "name": "get_scene_info",
                "arguments": {},
                "_meta": {"dcc": {"execution": "async"}}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(
        body.get("error").is_some(),
        "expected BACKEND_NOT_READY error, got {body}"
    );

    let after = jobs.list().len();
    assert_eq!(
        before, after,
        "red readiness probe must not queue a JobManager row for refused tools/call"
    );
}

// ── REST and MCP share the same probe ────────────────────────────────

#[tokio::test]
pub async fn rest_v1_call_and_mcp_tools_call_share_one_probe() {
    // Build a full router that mounts both the MCP handler and the
    // REST `/v1/*` surface so we can observe the shared probe from
    // both entry points. We cannot easily call `McpHttpServer::start`
    // in unit tests (it binds a socket); instead we replicate the
    // relevant wiring locally.
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};

    let (state, probe) = make_state_with_probe();
    let probe_dyn: Arc<dyn dcc_mcp_skill_rest::ReadinessProbe> = probe.clone();

    let rest_config = dcc_mcp_skill_rest::SkillRestConfig::new(
        dcc_mcp_skill_rest::SkillRestService::from_catalog_and_dispatcher(
            state.catalog.clone(),
            state.dispatcher.clone(),
        ),
    )
    .with_readiness(probe_dyn);
    let rest_router = dcc_mcp_skill_rest::build_skill_rest_router(rest_config);

    let mcp_router = Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(state)
        .merge(rest_router);
    let server = TestServer::new(mcp_router);

    // Probe is red by default — both surfaces must refuse.
    let mcp_resp = server
        .post("/mcp")
        .add_header(axum::http::header::ACCEPT, accept_json())
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 104,
            "method": "tools/call",
            "params": {"name": "get_scene_info", "arguments": {}}
        }))
        .await;
    mcp_resp.assert_status_ok();
    let mcp_body: Value = mcp_resp.json();
    assert_eq!(
        mcp_body["error"]["code"],
        json!(-32002),
        "MCP must refuse while probe is red"
    );

    let rest_resp = server
        .post("/v1/call")
        .json(&json!({"tool_slug": "test_dcc.get_scene_info", "arguments": {}}))
        .await;
    assert_eq!(rest_resp.status_code().as_u16(), 503);
    let rest_body: Value = rest_resp.json();
    assert_eq!(rest_body["kind"], json!("not-ready"));

    // Flip the probe green — both surfaces must now accept.
    probe.set_dispatcher_ready(true);
    probe.set_dcc_ready(true);

    let mcp_ok = server
        .post("/mcp")
        .add_header(axum::http::header::ACCEPT, accept_json())
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 105,
            "method": "tools/call",
            "params": {"name": "get_scene_info", "arguments": {}}
        }))
        .await;
    mcp_ok.assert_status_ok();
    let mcp_ok_body: Value = mcp_ok.json();
    assert!(
        mcp_ok_body.get("error").is_none(),
        "MCP must accept once probe is green, got {mcp_ok_body}"
    );
}
