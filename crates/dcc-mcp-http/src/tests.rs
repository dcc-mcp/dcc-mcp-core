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
    use dcc_mcp_actions::{ActionMeta, ActionRegistry};
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
        AppState {
            registry,
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
        let server = TestServer::new(make_router()).unwrap();

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
        let server = TestServer::new(make_router()).unwrap();

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
        // 5 core discovery tools + 2 registered actions = 7
        assert_eq!(tools.len(), 7);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"get_scene_info"));
        assert!(names.contains(&"list_objects"));
        assert!(names.contains(&"find_skills"));
        assert!(names.contains(&"load_skill"));
    }

    // ── tools/call known ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_tools_call_known_tool() {
        let server = TestServer::new(make_router()).unwrap();

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
        assert_eq!(body["result"]["isError"], false);
        assert!(body["result"]["content"].as_array().unwrap().len() > 0);
    }

    // ── tools/call unknown ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_tools_call_unknown_tool() {
        let server = TestServer::new(make_router()).unwrap();

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
        let server = TestServer::new(make_router()).unwrap();

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
        let server = TestServer::new(make_router()).unwrap();

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
        let server = TestServer::new(make_router()).unwrap();

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
        let server = TestServer::new(make_router()).unwrap();

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
        let server = TestServer::new(make_router()).unwrap();

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
        let server = TestServer::new(make_router()).unwrap();

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
            registry,
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
        let server = TestServer::new(make_router_with_skill()).unwrap();

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
        let server = TestServer::new(make_router_with_skill()).unwrap();

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
        let server = TestServer::new(make_router_with_skill()).unwrap();

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
        let server = TestServer::new(make_router_with_skill()).unwrap();

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
        // 5 core tools + 2 skill tools = 7
        assert_eq!(tools.len(), 7);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"modeling_bevel__bevel"));
        assert!(names.contains(&"modeling_bevel__chamfer"));
    }

    #[tokio::test]
    async fn test_unload_skill_removes_tools() {
        let server = TestServer::new(make_router_with_skill()).unwrap();

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
        // Back to just 5 core tools
        assert_eq!(tools.len(), 5);
    }

    #[tokio::test]
    async fn test_initialize_reports_list_changed_true() {
        let server = TestServer::new(make_router()).unwrap();

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
}
