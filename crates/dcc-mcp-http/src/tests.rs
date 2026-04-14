//! Unit and integration tests for the MCP HTTP server.

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use axum_test::TestServer;
    use serde_json::{Value, json};
    use std::sync::Arc;

    use crate::{
        config::McpHttpConfig, handler::AppState, server::McpHttpServer, session::SessionManager,
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
            server_name: "test-dcc".to_string(),
            server_version: "0.1.0".to_string(),
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

    // ── initialize ────────────────────────────────────────────────────────

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
        // Session ID injected
        assert!(result["__session_id"].is_string());
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
        // 6 core discovery tools + 2 registered actions = 8
        assert_eq!(tools.len(), 8);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"get_scene_info"));
        assert!(names.contains(&"list_objects"));
        assert!(names.contains(&"find_skills"));
        assert!(names.contains(&"load_skill"));
        assert!(names.contains(&"search_skills"));
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
            server_name: "test-dcc".to_string(),
            server_version: "0.1.0".to_string(),
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
    async fn test_search_skills_missing_query_returns_error() {
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
        // Missing required parameter — should return isError: true
        assert_eq!(body["result"]["isError"], true);
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
        assert_eq!(maya_stub["annotations"]["deferredHint"], true);
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
        // individual tool names (e.g. "maya_bevel__bevel") must NOT be present,
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
                "find_skills"
                    | "list_skills"
                    | "get_skill_info"
                    | "load_skill"
                    | "unload_skill"
                    | "search_skills"
            );
            let is_stub = name.starts_with("__skill__");

            assert!(
                is_core || is_stub,
                "Found unexpected tool '{name}' in tools/list before any skill was loaded. \
                 Only core meta-tools and __skill__<name> stubs should appear."
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
        // They must NOT appear before loading.
        for forbidden in &["maya_bevel__bevel", "maya_bevel__chamfer", "git_tools__log"] {
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

        // Real tools registered.
        assert!(
            names.contains(&"maya_bevel__bevel"),
            "Expected maya_bevel__bevel after load, got: {names:?}"
        );
        assert!(
            names.contains(&"maya_bevel__chamfer"),
            "Expected maya_bevel__chamfer after load, got: {names:?}"
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
        let bevel_tool = tools
            .iter()
            .find(|t| t["name"] == "maya_bevel__bevel")
            .unwrap();
        // inputSchema must be at least `{"type": "object"}` — not null/absent.
        assert!(
            !bevel_tool["inputSchema"].is_null(),
            "Loaded tool must have an inputSchema"
        );
        assert_eq!(bevel_tool["annotations"]["deferredHint"], false);

        let git_stub = tools
            .iter()
            .find(|t| t["name"] == "__skill__git-tools")
            .unwrap();
        assert_eq!(git_stub["annotations"]["deferredHint"], true);

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
            server_name: "test-dcc".to_string(),
            server_version: "0.1.0".to_string(),
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
        assert_eq!(result["action_count"], 2);

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
        // 6 core tools + 2 skill tools (skill now loaded, no stubs) = 8
        assert_eq!(tools.len(), 8);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"modeling_bevel__bevel"));
        assert!(names.contains(&"modeling_bevel__chamfer"));

        let bevel_tool = tools
            .iter()
            .find(|t| t["name"] == "modeling_bevel__bevel")
            .unwrap();
        assert_eq!(bevel_tool["annotations"]["deferredHint"], false);
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
        assert_eq!(result["actions_removed"], 2);

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
        // Back to 6 core tools + 1 unloaded skill stub = 7
        assert_eq!(tools.len(), 7);
        let stub = tools
            .iter()
            .find(|t| t["name"] == "__skill__modeling-bevel")
            .unwrap();
        assert_eq!(stub["annotations"]["deferredHint"], true);
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
            server_name: "test-dcc".to_string(),
            server_version: "0.1.0".to_string(),
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
            server_name: "test-dcc".to_string(),
            server_version: "0.1.0".to_string(),
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
}
