use super::*;


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
