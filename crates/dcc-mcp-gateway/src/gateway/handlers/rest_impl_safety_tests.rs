use super::rest_impl_tests::{CaptureSink, policy_record, response_json, test_gateway_state};
use super::*;
use std::sync::Arc;

async fn seed_call_backend(
    gs: &GatewayState,
    payload: Value,
) -> (String, tokio::sync::oneshot::Sender<()>) {
    let app = axum::Router::new().route(
        "/v1/call",
        axum::routing::post(move || {
            let payload = payload.clone();
            async move { Json(payload) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });

    let instance_id = uuid::Uuid::new_v4();
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", port);
    entry.instance_id = instance_id;
    entry
        .metadata
        .insert("mcp_url".into(), format!("http://127.0.0.1:{port}/mcp"));
    gs.registry.read().await.register(entry).unwrap();
    let record = policy_record("maya", instance_id, "capture", "app-ui", true);
    let slug = record.tool_slug.clone();
    gs.capability_index.upsert_instance(
        instance_id,
        vec![record],
        crate::gateway::capability::InstanceFingerprint(1),
    );
    (slug, tx)
}

#[tokio::test]
async fn transport_success_with_tool_failure_stays_http_ok_but_fails_mcp_and_batch() {
    use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain};

    let sink = Arc::new(CaptureSink::default());
    let mut gs = test_gateway_state("1.2.3");
    gs.middleware_chain =
        Arc::new(MiddlewareChain::new().with_after(Arc::new(AuditMiddleware::new(sink.clone()))));
    let (slug, shutdown) = seed_call_backend(
        &gs,
        json!({
            "success": true,
            "output": {"success": false, "message": "tool domain failure"}
        }),
    )
    .await;
    let mut headers = HeaderMap::new();
    headers.insert("accept", "application/json".parse().unwrap());

    let (status, rest_body) = response_json(
        handle_v1_call(
            State(gs.clone()),
            headers,
            Json(json!({
                "tool_slug": slug,
                "arguments": {
                    "action": "type",
                    "text": "rest-private-text",
                    "password": "rest-password",
                    "access_token": "rest-token"
                }
            })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "backend transport succeeded");
    assert_eq!(rest_body["output"]["success"], false);
    {
        let entries = sink.0.lock().unwrap();
        assert!(entries[0].is_error);
        let input = &entries[0].input_payload.as_ref().unwrap().content;
        for secret in ["rest-private-text", "rest-password", "rest-token"] {
            assert!(!input.contains(secret));
            assert!(!serde_json::to_string(&rest_body).unwrap().contains(secret));
        }
    }

    let (mcp_text, mcp_is_error) = crate::gateway::tools::tool_call_tool(
        &gs,
        &json!({"tool_slug": slug, "arguments": {}}),
        None,
        None,
        None,
    )
    .await;
    assert!(
        mcp_is_error,
        "MCP must expose the domain failure via isError"
    );
    assert!(mcp_text.contains("tool domain failure"));

    let batch = crate::gateway::tools::gateway_call_batch_inner(
        &gs,
        &json!({"calls": [{"tool_slug": slug, "arguments": {}}]}),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(batch["success"], false);
    assert_eq!(batch["results"][0]["ok"], false);
    assert_eq!(batch["results"][0]["error"]["kind"], "tool-error");
    assert_eq!(batch["results"][0]["result"]["output"]["success"], false);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn rest_single_keeps_images_in_response_but_redacts_audit_payloads() {
    use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain};

    let rich_image = format!("RICH_IMAGE_{}", "A".repeat(8192));
    let mcp_image = format!("MCP_IMAGE_{}", "B".repeat(8192));
    let backend_output = json!({
        "success": true,
        "output": {
            "context": {
                "__rich__": {
                    "kind": "image",
                    "mime": "image/png",
                    "data": rich_image,
                }
            },
            "content": [{
                "type": "image",
                "mimeType": "image/png",
                "data": mcp_image,
            }]
        }
    });
    let sink = Arc::new(CaptureSink::default());
    let mut gs = test_gateway_state("1.2.3");
    gs.middleware_chain =
        Arc::new(MiddlewareChain::new().with_after(Arc::new(AuditMiddleware::new(sink.clone()))));
    let (slug, shutdown) = seed_call_backend(&gs, backend_output).await;
    let request_body = json!({
        "tool_slug": slug,
        "arguments": {},
        "response_format": "json",
    });
    let headers = HeaderMap::new();

    let response = call_service_with_admin_trace(
        &gs,
        &headers,
        RestCallTraceRequest {
            method: "v1/call",
            slug: request_body["tool_slug"].as_str().unwrap(),
            arguments: json!({}),
            meta: None,
            request_body: &request_body,
            trace_context: crate::gateway::admin::trace::TraceContext::from_headers(&headers),
        },
    )
    .await
    .expect("single call should preserve the backend response");

    assert!(
        response
            .pointer("/output/context/__rich__/data")
            .and_then(Value::as_str)
            .is_some_and(|data| data.starts_with("RICH_IMAGE_"))
    );
    assert!(
        response
            .pointer("/output/content/0/data")
            .and_then(Value::as_str)
            .is_some_and(|data| data.starts_with("MCP_IMAGE_"))
    );
    let entries = sink.0.lock().unwrap();
    let entry = entries.first().expect("single call should be audited");
    assert!(!entry.result_preview.contains("RICH_IMAGE_"));
    assert!(!entry.result_preview.contains("MCP_IMAGE_"));
    let output = &entry.output_payload.as_ref().unwrap().content;
    assert!(!output.contains("RICH_IMAGE_"));
    assert!(!output.contains("MCP_IMAGE_"));
    assert!(output.contains(INLINE_IMAGE_TRACE_PLACEHOLDER));
    assert!(entry.token_accounting.as_ref().unwrap().original_bytes < rich_image.len());
    drop(entries);
    let _ = shutdown.send(());
}

#[tokio::test]
async fn rest_batch_keeps_images_in_response_but_redacts_audit_payloads() {
    use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain};

    let rich_image = format!("BATCH_RICH_IMAGE_{}", "A".repeat(8192));
    let mcp_image = format!("BATCH_MCP_IMAGE_{}", "B".repeat(8192));
    let backend_output = json!({
        "success": true,
        "output": {
            "context": {
                "__rich__": {
                    "kind": "image",
                    "mime": "image/png",
                    "data": rich_image,
                }
            },
            "content": [{
                "type": "image",
                "mimeType": "image/png",
                "data": mcp_image,
            }]
        }
    });
    let sink = Arc::new(CaptureSink::default());
    let mut gs = test_gateway_state("1.2.3");
    gs.middleware_chain =
        Arc::new(MiddlewareChain::new().with_after(Arc::new(AuditMiddleware::new(sink.clone()))));
    let (slug, shutdown) = seed_call_backend(&gs, backend_output).await;
    let request_body = json!({
        "calls": [{"tool_slug": slug, "arguments": {}}],
        "response_format": "toon",
    });
    let headers = HeaderMap::new();

    let response = call_batch_with_admin_trace(
        &gs,
        &headers,
        &request_body,
        crate::gateway::admin::trace::TraceContext::from_headers(&headers),
    )
    .await
    .expect("batch call should preserve the backend response");

    assert!(
        response
            .pointer("/results/0/result/output/context/__rich__/data")
            .and_then(Value::as_str)
            .is_some_and(|data| data.starts_with("BATCH_RICH_IMAGE_"))
    );
    assert!(
        response
            .pointer("/results/0/result/output/content/0/data")
            .and_then(Value::as_str)
            .is_some_and(|data| data.starts_with("BATCH_MCP_IMAGE_"))
    );
    let entries = sink.0.lock().unwrap();
    let entry = entries.first().expect("batch call should be audited");
    assert!(!entry.result_preview.contains("BATCH_RICH_IMAGE_"));
    assert!(!entry.result_preview.contains("BATCH_MCP_IMAGE_"));
    let output = &entry.output_payload.as_ref().unwrap().content;
    assert!(!output.contains("BATCH_RICH_IMAGE_"));
    assert!(!output.contains("BATCH_MCP_IMAGE_"));
    assert!(output.contains(INLINE_IMAGE_TRACE_PLACEHOLDER));
    let tokens = entry.token_accounting.as_ref().unwrap();
    assert_eq!(tokens.response_format, "toon");
    assert!(tokens.original_bytes < rich_image.len());
    assert!(tokens.returned_bytes < rich_image.len());
    drop(entries);
    let _ = shutdown.send(());
}
