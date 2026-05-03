//! Tests for ``search_tools`` (issue #677).
//!
//! Covers the two acceptance criteria of the issue:
//!
//! 1. Default search results never include `__skill__*` or `__group__*`
//!    progressive-loading stubs.
//! 2. Domain-keyword queries can discover unloaded skills, returning
//!    them as `skill_candidate` entries with `requires_load_skill: true`
//!    and a ready-to-send `load_hint`.

use super::*;

use serde_json::json;

/// Parse the stringified JSON that `CallToolResult::text` packs into
/// `result.content[0].text`. `search_tools` always emits a single
/// text content block on success, so this is always safe here.
fn parse_tool_result_text(body: &Value) -> Value {
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .expect("search_tools always returns text content");
    serde_json::from_str(text).expect("search_tools emits valid JSON")
}

async fn call_search_tools(server: &TestServer, args: Value) -> Value {
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 100,
            "method": "tools/call",
            "params": {
                "name": "search_tools",
                "arguments": args,
            }
        }))
        .await;
    resp.assert_status_ok();
    resp.json()
}

// Build a fresh `AppState` that seeds the registry with tools whose
// names would collide with the progressive-loading stub naming scheme
// if they ever leaked past the dispatcher. The catalog is left empty
// so tests that target stub filtering do not also exercise the
// skill-candidate branch.
fn make_app_state_with_stub_named_actions() -> AppState {
    use dcc_mcp_actions::ActionMeta;

    let registry = Arc::new(ActionRegistry::new());
    // Regular tool — expected to always appear.
    registry.register_action(ActionMeta {
        name: "create_sphere".into(),
        description: "Create a polygon sphere.".into(),
        category: "geometry".into(),
        tags: vec!["create".into(), "mesh".into()],
        dcc: "maya".into(),
        version: "1.0.0".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "radius": { "type": "number" }
            }
        }),
        ..Default::default()
    });
    // Stub-named action — should only surface when include_stubs=true.
    registry.register_action(ActionMeta {
        name: "__skill__maya-bevel".into(),
        description: "Stub for the maya-bevel skill.".into(),
        category: "stubs".into(),
        tags: vec!["sphere".into()],
        dcc: "maya".into(),
        version: "1.0.0".into(),
        ..Default::default()
    });
    registry.register_action(ActionMeta {
        name: "__group__modeling".into(),
        description: "Stub for the modeling group (sphere).".into(),
        category: "stubs".into(),
        tags: vec!["sphere".into()],
        dcc: "maya".into(),
        version: "1.0.0".into(),
        ..Default::default()
    });

    let catalog = Arc::new(dcc_mcp_skills::SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(dcc_mcp_actions::ActionDispatcher::new((*registry).clone()));
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

fn make_router_with_stub_named_actions() -> axum::Router {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};
    Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(make_app_state_with_stub_named_actions())
}

// ── 1. Default stub filtering (acceptance criterion 1) ────────────────

#[tokio::test]
pub async fn test_search_tools_excludes_stubs_by_default() {
    let server = TestServer::new(make_router_with_stub_named_actions());

    // "sphere" matches all three registered actions via tag/description,
    // but only the real `create_sphere` should come back by default.
    let body = call_search_tools(&server, json!({ "query": "sphere" })).await;
    let result = parse_tool_result_text(&body);
    let names: Vec<&str> = result["tools"]
        .as_array()
        .expect("tools array present")
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"create_sphere"),
        "expected create_sphere in default hits, got: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.starts_with("__skill__")),
        "__skill__* stubs must be filtered by default: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.starts_with("__group__")),
        "__group__* stubs must be filtered by default: {names:?}"
    );
}

#[tokio::test]
pub async fn test_search_tools_include_stubs_flag_surfaces_stubs() {
    let server = TestServer::new(make_router_with_stub_named_actions());

    let body =
        call_search_tools(&server, json!({ "query": "sphere", "include_stubs": true })).await;
    let result = parse_tool_result_text(&body);
    let names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"__skill__maya-bevel"),
        "include_stubs=true must surface __skill__* stubs, got: {names:?}"
    );
    assert!(
        names.contains(&"__group__modeling"),
        "include_stubs=true must surface __group__* stubs, got: {names:?}"
    );
}

// ── 2. Schema-property indexing ───────────────────────────────────────

#[tokio::test]
pub async fn test_search_tools_matches_schema_property_names() {
    let server = TestServer::new(make_router_with_stub_named_actions());

    // `radius` appears only in the input schema of `create_sphere` —
    // not in its name, description, category, or tags.
    let body = call_search_tools(&server, json!({ "query": "radius" })).await;
    let result = parse_tool_result_text(&body);
    let names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"create_sphere"),
        "schema property `radius` must make `create_sphere` discoverable: {names:?}"
    );
}

// ── 3. Unloaded-skill candidates (acceptance criterion 2) ─────────────

#[tokio::test]
pub async fn test_search_tools_surfaces_unloaded_skill_as_candidate() {
    let server = TestServer::new(make_router_with_skills());

    // `make_router_with_skills()` seeds `maya-bevel` in the catalog
    // but never loads it, so the search must return it as a skill
    // candidate — NOT as a `__skill__maya-bevel` stub.
    let body = call_search_tools(&server, json!({ "query": "bevel" })).await;
    let result = parse_tool_result_text(&body);

    let candidates = result["skill_candidates"]
        .as_array()
        .expect("skill_candidates array present");
    let names: Vec<&str> = candidates
        .iter()
        .map(|c| c["skill_name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"maya-bevel"),
        "expected maya-bevel candidate, got: {names:?}"
    );

    let maya = candidates
        .iter()
        .find(|c| c["skill_name"] == "maya-bevel")
        .unwrap();
    assert_eq!(maya["kind"], "skill_candidate");
    assert_eq!(maya["requires_load_skill"], true);
    assert_eq!(maya["load_hint"]["tool"], "load_skill");
    assert_eq!(maya["load_hint"]["arguments"]["skill_name"], "maya-bevel");

    // The matching_tools array should list the tool declarations inside
    // the skill whose name or description contains the query — `bevel`.
    let matching: Vec<&str> = maya["matching_tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        matching.contains(&"bevel"),
        "expected bevel tool in matching_tools: {matching:?}"
    );

    // And — critically — the candidate must NOT leak as a stub.
    let tool_names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        !tool_names.iter().any(|n| n.starts_with("__skill__")),
        "unloaded skills must not appear as stubs in tools[]: {tool_names:?}"
    );
}

#[tokio::test]
pub async fn test_search_tools_include_unloaded_skills_false() {
    let server = TestServer::new(make_router_with_skills());

    let body = call_search_tools(
        &server,
        json!({ "query": "bevel", "include_unloaded_skills": false }),
    )
    .await;
    let result = parse_tool_result_text(&body);
    let candidates = result["skill_candidates"].as_array().unwrap();
    assert!(
        candidates.is_empty(),
        "include_unloaded_skills=false must suppress skill candidates: {candidates:?}"
    );
}

// ── 4. Empty-result envelope shape ────────────────────────────────────

#[tokio::test]
pub async fn test_search_tools_no_results_envelope() {
    let server = TestServer::new(make_router_with_stub_named_actions());

    let body = call_search_tools(&server, json!({ "query": "zzz-no-such-thing" })).await;
    // Still a normal success envelope — never an isError response.
    assert_eq!(body["result"]["isError"], false);
    let result = parse_tool_result_text(&body);
    assert_eq!(result["total"], 0);
    assert_eq!(result["tools"].as_array().unwrap().len(), 0);
    assert_eq!(result["skill_candidates"].as_array().unwrap().len(), 0);
}

// ── 5. Stub synthesis from the catalog (regression for PR #681 e2e bug) ──

/// Regression: stubs are **not** stored in `ActionRegistry`; they are
/// synthesised on demand by `tools/list` for unloaded skills and
/// inactive tool groups. When `include_stubs=true`, `search_tools` must
/// walk the catalog itself and emit the same synthetic entries — mere
/// pass-through of registry rows is not enough because realistic
/// deployments (e.g. `McpHttpServer.discover()`) never write stub-named
/// actions into the registry.
///
/// PR #681 caught this gap in CI when running against a server built
/// from the catalog alone:
///
/// ```text
/// AssertionError: include_stubs=true must surface at least one stub, got: []
/// ```
#[tokio::test]
pub async fn test_search_tools_include_stubs_synthesises_from_catalog() {
    // Catalog-only server: no stub names pre-registered. The only way
    // `include_stubs=true` can surface __skill__maya-bevel is by
    // synthesising it from `SkillCatalog::list_skills("unloaded")`.
    let server = TestServer::new(make_router_with_skills());

    let body = call_search_tools(&server, json!({ "query": "bevel", "include_stubs": true })).await;
    let result = parse_tool_result_text(&body);
    let names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"__skill__maya-bevel"),
        "include_stubs=true must synthesise __skill__maya-bevel from the \
         catalog even when no stub-named action exists in the registry, got: {names:?}"
    );
}
