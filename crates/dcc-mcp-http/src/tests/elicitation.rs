use super::*;

#[tokio::test]
pub async fn test_initialize_advertises_elicitation_for_2025_06_18_only() {
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
pub async fn test_elicitation_create_requires_2025_06_18() {
    let server = TestServer::new(make_router());
    let session_id = "elicitation-gate-session";

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
pub async fn test_elicitation_create_roundtrip_via_sse_response() {
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
