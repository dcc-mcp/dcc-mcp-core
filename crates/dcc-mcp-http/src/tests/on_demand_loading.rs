use super::*;

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
pub async fn test_tools_list_no_full_schemas_before_load() {
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
                // Dynamic tool management (issue #462)
                | "register_tool"
                | "deregister_tool"
                | "list_dynamic_tools"
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
pub async fn test_skill_tool_names_absent_before_load() {
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
pub async fn test_load_skill_then_tools_list_has_real_tools_not_stub() {
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
pub async fn test_on_demand_count_invariant() {
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
pub async fn test_skill_stub_call_returns_load_hint() {
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
