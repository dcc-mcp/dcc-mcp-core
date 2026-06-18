use serde_json::{Value, json};

use super::rest_impl_tests::test_gateway_state;

/// Spawn a fake backend that responds to /v1/describe and /v1/call.
async fn spawn_echo_backend() -> (u16, tokio::sync::oneshot::Sender<()>) {
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/v1/describe",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| async move {
                let slug = body.get("tool_slug").and_then(Value::as_str).unwrap_or("");
                axum::Json(json!({
                    "entry": {
                        "slug": slug,
                        "skill": "test-skill",
                        "action": "echo",
                        "dcc": "maya",
                        "loaded": true
                    },
                    "description": "Echo test tool",
                    "input_schema": {"type": "object", "properties": {}},
                    "annotations": {
                        "readOnlyHint": false,
                        "destructiveHint": false,
                        "openWorldHint": true
                    }
                }))
            }),
        )
        .route(
            "/v1/call",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| async move {
                let slug = body.get("tool_slug").and_then(Value::as_str).unwrap_or("");
                axum::Json(json!({
                    "content": [{
                        "type": "text",
                        "text": format!("called {slug} successfully")
                    }],
                    "isError": false
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (port, tx)
}

#[tokio::test]
async fn gateway_rest_v1_call_batch_mixed_success_failure_continues_on_error() {
    let (backend_port, _stop_backend) = spawn_echo_backend().await;

    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let instance_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    {
        let r = registry.read().await;
        let mut entry = dcc_mcp_transport::discovery::types::ServiceEntry::new(
            "maya",
            "127.0.0.1",
            backend_port,
        );
        entry.instance_id = instance_id;
        r.register(entry).unwrap();
    }

    let mut gs = test_gateway_state("1.2.3");
    // Replace the registry with one that points to our fake backend.
    gs.registry = registry;
    // Insert a loaded capability record so describe + call can route.
    let good_slug = format!("maya.{instance_id}.echo");
    let good_record = crate::gateway::capability::CapabilityRecord::new(
        good_slug.clone(),
        "echo".to_string(),
        "echo".to_string(),
        Some("test-skill".to_string()),
        "Echo test tool",
        vec![],
        "maya".to_string(),
        instance_id,
        true, // has_schema
        true, // loaded
        None,
    );
    gs.capability_index.upsert_instance(
        instance_id,
        vec![good_record],
        crate::gateway::capability::InstanceFingerprint(1),
    );

    let app = crate::gateway::router::build_gateway_router(gs);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway_port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{gateway_port}/v1/call");
    // Non-routable slug: no backend for "blender" DCC type → instance-offline error.
    let bad_slug = "blender.00000000-0000-0000-0000-000000000002.nonexistent";

    let resp = client
        .post(&url)
        .header("Accept", "application/json")
        .json(&json!({
            "calls": [
                {"id": "good-one", "tool_slug": good_slug, "arguments": {}},
                {"id": "bad-one", "tool_slug": bad_slug, "arguments": {}},
                {"id": "good-two", "tool_slug": good_slug, "arguments": {}}
            ],
            "stop_on_error": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();

    // Batch envelope
    assert_eq!(
        body["success"], false,
        "overall success false because one item failed"
    );
    assert_eq!(body["stop_on_error"], false);
    let results = body["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        3,
        "all 3 items should be present (stop_on_error=false)"
    );

    // Item 0: success (routed to fake backend)
    assert_eq!(results[0]["index"], 0);
    assert_eq!(results[0]["id"], "good-one");
    assert_eq!(results[0]["ok"], true);
    assert!(results[0].get("result").is_some());

    // Item 1: failure (no backend for blender DCC type)
    assert_eq!(results[1]["index"], 1);
    assert_eq!(results[1]["id"], "bad-one");
    assert_eq!(results[1]["ok"], false);
    assert!(results[1].get("error").is_some());

    // Item 2: success (continued after item 1 failure — stop_on_error=false)
    assert_eq!(results[2]["index"], 2);
    assert_eq!(results[2]["id"], "good-two");
    assert_eq!(results[2]["ok"], true);
    assert!(results[2].get("result").is_some());

    // Cleanup
    let _ = shutdown_tx.send(());
    let _ = server.await;
}
