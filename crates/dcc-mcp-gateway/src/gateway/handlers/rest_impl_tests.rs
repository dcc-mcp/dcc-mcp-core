use super::*;
use axum::body::to_bytes;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{RwLock, broadcast, watch};

#[derive(Default)]
struct CaptureSink(Mutex<Vec<crate::gateway::middleware::AuditEntry>>);

impl crate::gateway::middleware::AuditSink for CaptureSink {
    fn record(&self, entry: crate::gateway::middleware::AuditEntry) {
        self.0.lock().unwrap().push(entry);
    }
}

struct ReplaceArgs(serde_json::Value);

impl crate::gateway::middleware::BeforeCallMiddleware for ReplaceArgs {
    fn before_call<'a>(
        &'a self,
        ctx: &'a mut crate::gateway::middleware::CallContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<(), crate::gateway::middleware::MiddlewareError>,
                > + Send
                + 'a,
        >,
    > {
        ctx.args = self.0.clone();
        Box::pin(async move { Ok::<(), crate::gateway::middleware::MiddlewareError>(()) })
    }
}

fn test_gateway_state(server_version: &str) -> GatewayState {
    test_gateway_state_with_debug_routes(server_version, false)
}

fn test_gateway_state_with_debug_routes(
    server_version: &str,
    debug_routes_enabled: bool,
) -> GatewayState {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(8);
    GatewayState {
        registry,
        stale_timeout: Duration::from_secs(30),
        backend_timeout: Duration::from_secs(10),
        async_dispatch_timeout: Duration::from_secs(60),
        wait_terminal_timeout: Duration::from_secs(600),
        server_name: "test".into(),
        server_version: server_version.into(),
        own_host: "127.0.0.1".into(),
        own_port: 9765,
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(HashMap::new())),
        pending_calls: Arc::new(RwLock::new(HashMap::new())),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        adapter_version: None,
        adapter_dcc: None,
        capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
        middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
        instance_diagnostics: Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        debug_routes_enabled,
        #[cfg(feature = "prometheus")]
        gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
    }
}

async fn response_json(resp: Response) -> (StatusCode, Value) {
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap();
    (status, body)
}

async fn response_text(resp: Response) -> (StatusCode, String) {
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn response_text_with_headers(resp: Response) -> (StatusCode, HeaderMap, String) {
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    (status, headers, String::from_utf8_lossy(&bytes).to_string())
}

#[tokio::test]
async fn gateway_readyz_summarises_instance_readiness_bits() {
    let gs = test_gateway_state("1.2.3");
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    entry.instance_id = uuid::Uuid::parse_str("abcdef01-2345-6789-abcd-ef0123456789").unwrap();
    {
        let registry = gs.registry.read().await;
        registry.register(entry.clone()).unwrap();
    }
    gs.instance_diagnostics.record_readiness(
        entry.instance_id,
        dcc_mcp_skill_rest::ReadinessReport {
            process: true,
            dcc: true,
            skill_catalog: true,
            dispatcher: true,
            host_execution_bridge: false,
            main_thread_executor: false,
        },
    );

    let (status, body) = response_json(handle_v1_readyz(State(gs)).await.into_response()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["live_instance_count"], 1);
    assert_eq!(body["ready_instance_count"], 1);
    assert_eq!(body["instances"][0]["instance_short"], "abcdef01");
    assert_eq!(body["instances"][0]["readiness"]["skill_catalog"], true);
    assert_eq!(
        body["instances"][0]["readiness"]["host_execution_bridge"],
        false
    );
}

#[tokio::test]
async fn gateway_docs_serves_scalar_openapi_ui() {
    let (status, body) =
        response_text(handle_v1_docs(State(test_gateway_state("1.2.3"))).await).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("scalar") || body.contains("Scalar"));
    assert!(body.contains("dcc-mcp-gateway"));
    assert!(body.contains("/v1/search"));
    assert!(!body.contains("/v1/debug/instances"));
}

#[tokio::test]
#[cfg(feature = "admin")]
async fn gateway_docs_lists_debug_routes_when_runtime_debug_routes_enabled() {
    let (status, body) = response_text(
        handle_v1_docs(State(test_gateway_state_with_debug_routes("1.2.3", true))).await,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("/v1/debug/instances"));
}

#[tokio::test]
#[cfg(feature = "admin")]
async fn gateway_openapi_lists_stable_debug_routes() {
    let (status, doc) = response_json(
        handle_v1_openapi(State(test_gateway_state_with_debug_routes("1.2.3", true)))
            .await
            .into_response(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    for path in [
        "/v1/debug/instances",
        "/v1/debug/activity",
        "/v1/debug/calls",
        "/v1/debug/traces",
        "/v1/debug/traces/{request_id}",
        "/v1/debug/trace-context/{lookup_id}",
        "/v1/debug/tasks",
        "/v1/debug/bundles/{request_id}",
        "/v1/debug/issue-reports/{request_id}",
        "/v1/debug/logs",
        "/v1/debug/deregistered",
        "/v1/debug/stats",
        "/v1/debug/health",
    ] {
        assert!(
            doc["paths"].get(path).is_some(),
            "gateway OpenAPI doc missing debug path {path}: {doc:#}"
        );
    }
    assert!(
        doc["tags"]
            .as_array()
            .is_some_and(|tags| tags.iter().any(|tag| tag["name"] == "debug"))
    );
    assert!(
        doc["components"]["schemas"]
            .get("GatewayDebugPayload")
            .is_some()
    );
}

#[tokio::test]
#[cfg(feature = "admin")]
async fn gateway_openapi_omits_debug_routes_when_runtime_admin_disabled() {
    let (status, doc) = response_json(
        handle_v1_openapi(State(test_gateway_state("1.2.3")))
            .await
            .into_response(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(doc["paths"].get("/v1/search").is_some());
    assert!(doc["paths"].get("/v1/debug/instances").is_none());
    assert!(
        doc["tags"]
            .as_array()
            .is_none_or(|tags| !tags.iter().any(|tag| tag["name"] == "debug"))
    );
    assert!(
        doc["components"]["schemas"]
            .get("GatewayDebugPayload")
            .is_none()
    );
}

#[tokio::test]
#[cfg(not(feature = "admin"))]
async fn gateway_openapi_omits_debug_routes_without_admin_feature() {
    let (status, doc) = response_json(
        handle_v1_openapi(State(test_gateway_state("1.2.3")))
            .await
            .into_response(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(doc["paths"].get("/v1/search").is_some());
    assert!(doc["paths"].get("/v1/debug/instances").is_none());
    assert!(
        doc["tags"]
            .as_array()
            .is_none_or(|tags| !tags.iter().any(|tag| tag["name"] == "debug"))
    );
    assert!(
        doc["components"]["schemas"]
            .get("GatewayDebugPayload")
            .is_none()
    );
}

#[tokio::test]
async fn gateway_yield_missing_challenger_is_structured_optional_capability() {
    let (status, body) = response_json(
        handle_gateway_yield(
            State(test_gateway_state("1.2.3")),
            axum::body::Bytes::from_static(b"{}"),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["success"], false);
    assert_eq!(body["fallback"], "polling");
    assert_eq!(body["error"]["kind"], "optional-capability-unsupported");
    assert_eq!(body["error"]["capability"], "cooperative_yield");
}

#[tokio::test]
async fn gateway_yield_same_version_is_structured_optional_capability() {
    let (status, body) = response_json(
        handle_gateway_yield(
            State(test_gateway_state("1.2.3")),
            axum::body::Bytes::from_static(br#"{"challenger_version":"1.2.3"}"#),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["current_version"], "1.2.3");
    assert_eq!(body["challenger_version"], "1.2.3");
    assert_eq!(body["error"]["kind"], "optional-capability-unsupported");
}

#[tokio::test]
async fn gateway_yield_newer_challenger_still_accepts() {
    let (status, body) = response_json(
        handle_gateway_yield(
            State(test_gateway_state("1.2.3")),
            axum::body::Bytes::from_static(br#"{"challenger_version":"1.2.4"}"#),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn rest_describe_bad_request_can_return_compact_toon() {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::ACCEPT,
        crate::gateway::response_codec::TOON_MIME.parse().unwrap(),
    );

    let (status, response_headers, body) = response_text_with_headers(
        handle_v1_describe(State(test_gateway_state("1.2.3")), headers, Json(json!({}))).await,
    )
    .await;
    let decoded: Value = toon_format::decode_default(&body).unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        response_headers
            .get(crate::gateway::response_codec::HEADER_RESPONSE_FORMAT)
            .and_then(|value| value.to_str().ok()),
        Some("toon")
    );
    assert_eq!(decoded["error"]["kind"], "bad-request");
    assert_eq!(
        decoded["error"]["message"],
        "missing required field: tool_slug"
    );
}

#[tokio::test]
async fn rest_call_bad_request_json_override_wins_over_accept() {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::ACCEPT,
        crate::gateway::response_codec::TOON_MIME.parse().unwrap(),
    );
    let body = json!({
        "response_format": "json",
        "arguments": {}
    });

    let (status, response_headers, body_text) = response_text_with_headers(
        handle_v1_call(State(test_gateway_state("1.2.3")), headers, Json(body)).await,
    )
    .await;
    let decoded: Value = serde_json::from_str(&body_text).unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        response_headers
            .get(crate::gateway::response_codec::HEADER_RESPONSE_FORMAT)
            .and_then(|value| value.to_str().ok()),
        Some("json")
    );
    assert_eq!(decoded["error"]["kind"], "bad-request");
}

#[tokio::test]
async fn rest_call_batch_bad_request_can_return_compact_toon() {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::ACCEPT,
        crate::gateway::response_codec::TOON_MIME.parse().unwrap(),
    );

    let (status, response_headers, body) = response_text_with_headers(
        handle_v1_call_batch(
            State(test_gateway_state("1.2.3")),
            headers,
            Json(json!({"calls": []})),
        )
        .await,
    )
    .await;
    let decoded: Value = toon_format::decode_default(&body).unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        response_headers
            .get(crate::gateway::response_codec::HEADER_RESPONSE_FORMAT)
            .and_then(|value| value.to_str().ok()),
        Some("toon")
    );
    assert_eq!(decoded["success"], false);
    assert_eq!(decoded["error"]["kind"], "bad-request");
    assert_eq!(
        decoded["error"]["message"],
        "calls must be a non-empty array"
    );
}

#[tokio::test]
async fn rest_trace_input_payload_uses_redacted_arguments() {
    use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain, RedactionMiddleware};

    let sink = Arc::new(CaptureSink::default());
    let chain = MiddlewareChain::new()
        .with_before(Arc::new(RedactionMiddleware::new(vec!["api_key", "token"])))
        .with_after(Arc::new(AuditMiddleware::new(sink.clone())));
    let mut gs = test_gateway_state("1.2.3");
    gs.middleware_chain = Arc::new(chain);

    let request_body = json!({
        "tool_slug": "maya.abcdef01.render",
        "arguments": {
            "api_key": "secret-key",
            "nested": {"token": "secret-token"}
        },
        "meta": {}
    });

    let result = call_service_with_admin_trace(
        &gs,
        &HeaderMap::new(),
        "v1/call",
        "maya.abcdef01.render",
        request_body["arguments"].clone(),
        request_body.get("meta").cloned(),
        &request_body,
    )
    .await;

    assert!(result.is_err());
    let entries = sink.0.lock().unwrap();
    assert_eq!(entries.len(), 1);
    let input = entries[0].input_payload.as_ref().unwrap().content.clone();
    assert!(input.contains("[REDACTED]"));
    assert!(!input.contains("secret-key"));
    assert!(!input.contains("secret-token"));
}

#[tokio::test]
async fn rest_traceparent_does_not_replace_request_id() {
    use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain};

    let sink = Arc::new(CaptureSink::default());
    let chain = MiddlewareChain::new().with_after(Arc::new(AuditMiddleware::new(sink.clone())));
    let mut gs = test_gateway_state("1.2.3");
    gs.middleware_chain = Arc::new(chain);

    let mut headers = HeaderMap::new();
    headers.insert("x-request-id", "req-rest-1".parse().unwrap());
    headers.insert(
        "traceparent",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
            .parse()
            .unwrap(),
    );
    let request_body = json!({
        "tool_slug": "maya.abcdef01.render",
        "arguments": {},
        "meta": {}
    });

    let _ = call_service_with_admin_trace(
        &gs,
        &headers,
        "v1/call",
        "maya.abcdef01.render",
        json!({}),
        request_body.get("meta").cloned(),
        &request_body,
    )
    .await;

    let entries = sink.0.lock().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].request_id, "req-rest-1");
    assert_eq!(
        entries[0].trace_context.trace_id,
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
    assert_eq!(
        entries[0].trace_context.parent_span_id.as_deref(),
        Some("00f067aa0ba902b7")
    );
    let root_span_id = entries[0]
        .trace_context
        .span_id
        .as_deref()
        .expect("REST trace context should create a gateway root span id");
    assert_eq!(root_span_id.len(), 16);
    let backend_span = entries[0]
        .trace_spans
        .iter()
        .find(|span| span.name == "backend.execute")
        .expect("REST call should record backend.execute span");
    let backend_span_id = backend_span
        .span_id
        .as_deref()
        .expect("backend.execute should carry its own span id");
    assert_eq!(backend_span_id.len(), 16);
    assert_ne!(backend_span_id, root_span_id);
    assert_eq!(backend_span.parent_span_id.as_deref(), Some(root_span_id));
}

#[tokio::test]
async fn rest_call_batch_uses_arguments_mutated_by_before_middleware() {
    use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain, RedactionMiddleware};

    let sink = Arc::new(CaptureSink::default());
    let rewritten = json!({
        "calls": [{
            "tool_slug": "maya.abcdef01.render",
            "arguments": {"token": "secret-token"}
        }]
    });
    let chain = MiddlewareChain::new()
        .with_before(Arc::new(ReplaceArgs(rewritten)))
        .with_before(Arc::new(RedactionMiddleware::new(vec!["token"])))
        .with_after(Arc::new(AuditMiddleware::new(sink.clone())));
    let mut gs = test_gateway_state("1.2.3");
    gs.middleware_chain = Arc::new(chain);

    let result = call_batch_with_admin_trace(&gs, &HeaderMap::new(), &json!({"calls": []})).await;

    let body = result.expect("batch should use middleware-mutated args");
    assert_eq!(body["results"][0]["tool_slug"], "maya.abcdef01.render");
    let entries = sink.0.lock().unwrap();
    assert_eq!(entries.len(), 1);
    let input = entries[0].input_payload.as_ref().unwrap().content.clone();
    assert!(input.contains("[REDACTED]"));
    assert!(!input.contains("secret-token"));
}
