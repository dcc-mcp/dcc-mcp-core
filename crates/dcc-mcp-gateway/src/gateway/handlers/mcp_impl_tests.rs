use super::*;
use axum::body::to_bytes;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::{RwLock, broadcast, watch};

#[derive(Default)]
struct CaptureSink(Mutex<Vec<crate::gateway::middleware::AuditEntry>>);

impl crate::gateway::middleware::AuditSink for CaptureSink {
    fn record(&self, entry: crate::gateway::middleware::AuditEntry) {
        self.0.lock().unwrap().push(entry);
    }
}

struct TransformAfter {
    text: Option<&'static str>,
    is_error: Option<bool>,
}

impl crate::gateway::middleware::AfterCallMiddleware for TransformAfter {
    fn after_call<'a>(
        &'a self,
        _ctx: &'a crate::gateway::middleware::CallContext,
        result: &'a mut crate::gateway::middleware::CallResult,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<(), crate::gateway::middleware::MiddlewareError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            if let Some(text) = self.text {
                result.text = text.to_string();
            }
            if let Some(is_error) = self.is_error {
                result.is_error = is_error;
            }
            Ok(())
        })
    }
}

struct RejectAfter;

impl crate::gateway::middleware::AfterCallMiddleware for RejectAfter {
    fn after_call<'a>(
        &'a self,
        _ctx: &'a crate::gateway::middleware::CallContext,
        _result: &'a mut crate::gateway::middleware::CallResult,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<(), crate::gateway::middleware::MiddlewareError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            Err(
                crate::gateway::middleware::MiddlewareError::PolicyViolation(
                    "image blocked".to_string(),
                ),
            )
        })
    }
}

fn test_gateway_state() -> GatewayState {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(8);
    GatewayState {
        registry,
        http_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::http_registration::HttpInstanceRegistry::default(),
        )),

        mdns_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::mdns_registration::MdnsInstanceRegistry::default(),
        )),
        relay_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::relay_registration::RelayInstanceRegistry::default(),
        )),
        stale_timeout: Duration::from_secs(30),
        backend_timeout: Duration::from_secs(10),
        async_dispatch_timeout: Duration::from_secs(60),
        wait_terminal_timeout: Duration::from_secs(600),
        server_name: "test-gateway".into(),
        server_version: env!("CARGO_PKG_VERSION").into(),
        own_host: "127.0.0.1".into(),
        own_port: 9765,
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(HashMap::new())),
        client_attribution: Arc::new(
            crate::gateway::caller_attribution::ClientAttributionStore::default(),
        ),
        pending_calls: Arc::new(RwLock::new(HashMap::new())),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        policy: Arc::new(crate::gateway::GatewayPolicy::default()),
        adapter_version: None,
        adapter_dcc: None,
        capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        search_cache: Arc::new(crate::gateway::capability::search_cache::SearchCache::new(
            Default::default(),
        )),
        event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
        middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
        instance_diagnostics: Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        search_telemetry: Arc::new(crate::gateway::search_telemetry::SearchTelemetryStore::new()),
        debug_routes_enabled: false,
        auth: std::sync::Arc::new(crate::gateway::security::GatewayAuth::disabled()),
        update_manifest_url: None,
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
        semantic_search_enabled: false,
        #[cfg(feature = "prometheus")]
        gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        #[cfg(feature = "admin-persist-sqlite")]
        admin_sqlite_lane: None,
    }
}

fn request(method: &str, id: Value, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: Some("2.0".into()),
        id: Some(id),
        method: method.into(),
        params,
    }
}

async fn dispatch(req: &JsonRpcRequest) -> Value {
    let gs = test_gateway_state();
    dispatch_single_request(&gs, req, "test-session", &HeaderMap::new())
        .await
        .expect("request has id")
}

const TEST_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9ZQmcAAAAASUVORK5CYII=";

async fn rich_image_gateway_state() -> (
    GatewayState,
    Arc<CaptureSink>,
    tempfile::TempDir,
    tokio::sync::oneshot::Sender<()>,
    String,
) {
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/v1/call",
            axum::routing::post(|| async {
                axum::Json(json!({
                    "success": true,
                    "output": {
                        "success": true,
                        "context": {
                            "__rich__": {
                                "kind": "image",
                                "mime": "image/png",
                                "data": TEST_PNG_BASE64
                            }
                        }
                    }
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });

    let registry_dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(registry_dir.path()).unwrap()));
    let instance_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    {
        let registry = registry.read().await;
        let mut entry = dcc_mcp_transport::discovery::types::ServiceEntry::new(
            "maya",
            "127.0.0.1",
            backend_port,
        );
        entry.instance_id = instance_id;
        registry.register(entry).unwrap();
    }

    let sink = Arc::new(CaptureSink::default());
    let audit = Arc::new(crate::gateway::middleware::AuditMiddleware::new(
        sink.clone(),
    ));
    let mut gs = test_gateway_state();
    gs.registry = registry;
    gs.middleware_chain = Arc::new(
        crate::gateway::middleware::MiddlewareChain::new()
            .with_before(audit.clone())
            .with_after(audit),
    );

    let tool_slug = format!("maya.{instance_id}.ui_control__snapshot");
    let record = crate::gateway::capability::CapabilityRecord::new(
        tool_slug.clone(),
        "ui_control__snapshot".to_string(),
        "ui_control__snapshot".to_string(),
        Some("ui-control".to_string()),
        "Capture an application screenshot",
        vec![],
        "maya".to_string(),
        instance_id,
        true,
        true,
        None,
    );
    gs.capability_index.upsert_instance(
        instance_id,
        vec![record],
        crate::gateway::capability::InstanceFingerprint(1),
    );

    tokio::time::sleep(Duration::from_millis(25)).await;
    (gs, sink, registry_dir, shutdown_tx, tool_slug)
}

fn decode_toon_result(result: &Value) -> Value {
    let text = result["text"].as_str().expect("compact result has text");
    toon_format::decode_default(text).expect("compact result is valid TOON")
}

#[tokio::test]
async fn initialize_advertises_mcp_compact_response_capability() {
    let req = request(
        "initialize",
        json!(1),
        Some(json!({"protocolVersion": "2025-03-26"})),
    );
    let response = dispatch(&req).await;

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(
        response["result"]["capabilities"]["experimental"]["dcc-mcp"]["compactResponses"]["formats"]
            [1],
        "toon"
    );
    assert!(
        response["result"].get("text").is_none(),
        "initialize must remain legacy JSON so clients can negotiate capabilities"
    );
}

#[tokio::test]
async fn tools_list_legacy_response_stays_json() {
    let req = request("tools/list", json!(2), None);
    let response = dispatch(&req).await;

    let tools = response["result"]["tools"]
        .as_array()
        .expect("legacy tools/list returns tools array");
    assert_eq!(tools.len(), 4, "gateway tools/list must stay bounded");
    assert!(response["result"].get("text").is_none());
}

#[tokio::test]
async fn tools_list_can_return_json_rpc_safe_compact_toon() {
    let req = request(
        "tools/list",
        json!("compact-list"),
        Some(json!({"_meta": {"response_format": "toon"}})),
    );
    let response = dispatch(&req).await;

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "compact-list");
    assert_eq!(response["result"]["response_format"], "toon");
    assert_eq!(response["result"]["mimeType"], TOON_MIME);
    assert_eq!(
        response["result"]["_meta"]["token_accounting"]["response_format"],
        "toon"
    );

    let decoded = decode_toon_result(&response["result"]);
    let tools = decoded["tools"]
        .as_array()
        .expect("compact tools/list decodes to tools array");
    assert_eq!(
        tools.len(),
        4,
        "compact mode must not fan out backend tools"
    );
}

#[tokio::test]
async fn explicit_mcp_json_opt_out_wins_over_compact_alias() {
    let req = request(
        "tools/list",
        json!("json-opt-out"),
        Some(json!({"_meta": {"response_format": "json", "compact": true}})),
    );
    let response = dispatch(&req).await;

    let tools = response["result"]["tools"]
        .as_array()
        .expect("explicit JSON opt-out keeps the legacy tools/list shape");
    assert_eq!(tools.len(), 4);
    assert!(response["result"].get("text").is_none());
    assert!(response["result"].get("response_format").is_none());
}

#[tokio::test]
async fn batch_request_compacts_only_opted_in_items() {
    let gs = test_gateway_state();
    let batch = vec![
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {"_meta": {"compact": true}}
        }),
        json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
    ];

    let response = handle_batch_request(&gs, "test-session", &batch, &HeaderMap::new()).await;
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("batch response body");
    let body: Value = serde_json::from_slice(&bytes).expect("JSON-RPC batch body");
    let items = body.as_array().expect("batch response is array");

    assert_eq!(items[0]["result"]["response_format"], "toon");
    assert_eq!(items[1]["result"], json!({}));
    assert!(items[1]["result"].get("text").is_none());
}

#[tokio::test]
async fn resources_read_compact_preserves_content_hints_inside_toon() {
    let req = request(
        "resources/read",
        json!("docs"),
        Some(json!({
            "uri": "gateway://docs/agent-workflows",
            "_meta": {"response_format": "toon"}
        })),
    );
    let response = dispatch(&req).await;

    assert_eq!(response["result"]["response_format"], "toon");
    let decoded = decode_toon_result(&response["result"]);
    assert_eq!(
        decoded["contents"][0]["uri"],
        "gateway://docs/agent-workflows"
    );
    assert_eq!(decoded["contents"][0]["mimeType"], "application/json");
    assert!(decoded["contents"][0]["text"].as_str().is_some());
}

#[tokio::test]
async fn tools_call_search_compact_preserves_call_tool_result_shape() {
    let req = request(
        "tools/call",
        json!(3),
        Some(json!({
            "name": "search",
            "arguments": {"kind": "tool", "query": "sphere"},
            "_meta": {"responseFormat": "toon"}
        })),
    );
    let response = dispatch(&req).await;

    assert_eq!(response["result"]["isError"], false);
    assert_eq!(response["result"]["content"][0]["type"], "text");
    assert_eq!(response["result"]["content"][0]["mimeType"], TOON_MIME);
    assert_eq!(
        response["result"]["_meta"]["token_accounting"]["response_format"],
        "toon"
    );
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("compact tool content has text");
    let decoded: Value = toon_format::decode_default(text).expect("tool content is TOON");
    assert_eq!(decoded["total"], 0);
    assert!(decoded["hits"].as_array().is_some());
}

#[tokio::test]
async fn tools_call_search_records_meta_and_server_network_attribution() {
    let gs = test_gateway_state();
    let mut headers = HeaderMap::new();
    headers.insert(
        crate::gateway::caller_attribution::INTERNAL_SOURCE_IP_HEADER,
        "192.0.2.44".parse().unwrap(),
    );
    let req = request(
        "tools/call",
        json!("attributed-search"),
        Some(json!({
            "name": "search",
            "arguments": {"kind": "tool", "query": "sphere"},
            "_meta": {
                "agent_context": {
                    "actor_id": "artist-1",
                    "client_platform": "cursor",
                    "sourceIp": "203.0.113.100"
                }
            }
        })),
    );

    let response = dispatch_single_request(&gs, &req, "test-session", &headers)
        .await
        .expect("request has id");

    assert_eq!(response["result"]["isError"], false);
    let telemetry = gs.search_telemetry.snapshot(10);
    let agent = telemetry.recent[0]
        .agent_context
        .as_ref()
        .expect("MCP search should keep attribution");
    assert_eq!(agent.actor_id.as_deref(), Some("artist-1"));
    assert_eq!(agent.client_platform.as_deref(), Some("cursor"));
    assert_eq!(agent.source_ip.as_deref(), Some("192.0.2.44"));
}

#[tokio::test]
async fn gateway_call_routes_emit_native_image_without_audit_base64() {
    let (gs, sink, _registry_dir, _shutdown, tool_slug) = rich_image_gateway_state().await;

    for (index, route) in ["call", "call_tool"].into_iter().enumerate() {
        let req = request(
            "tools/call",
            json!(format!("rich-image-{index}")),
            Some(json!({
                "name": route,
                "arguments": {
                    "tool_slug": tool_slug,
                    "arguments": {}
                }
            })),
        );
        let response = dispatch_single_request(&gs, &req, "test-session", &HeaderMap::new())
            .await
            .expect("request has id");

        assert_eq!(response["result"]["isError"], false);
        let content = response["result"]["content"]
            .as_array()
            .expect("tools/call content array");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image", "route {route}");
        assert_eq!(content[1]["mimeType"], "image/png", "route {route}");
        assert_eq!(content[1]["data"], TEST_PNG_BASE64, "route {route}");

        let safe_text = content[0]["text"].as_str().expect("text content");
        assert!(
            !safe_text.contains(TEST_PNG_BASE64),
            "route {route} must remove image base64 from text"
        );
        let safe_payload: Value = serde_json::from_str(safe_text).expect("sanitized backend JSON");
        assert_eq!(
            safe_payload
                .pointer("/output/context/__rich__/data")
                .and_then(Value::as_str),
            Some(NATIVE_IMAGE_PLACEHOLDER),
            "route {route} keeps an explicit native-content placeholder"
        );
    }

    let entries = sink.0.lock().unwrap();
    assert_eq!(entries.len(), 2);
    for entry in entries.iter() {
        assert!(!entry.result_preview.contains(TEST_PNG_BASE64));
        assert!(
            !entry
                .output_payload
                .as_ref()
                .expect("captured output payload")
                .content
                .contains(TEST_PNG_BASE64)
        );
    }
}

#[tokio::test]
async fn after_call_transform_drops_preextracted_native_image() {
    let (mut gs, _sink, _registry_dir, _shutdown, tool_slug) = rich_image_gateway_state().await;

    for (index, middleware, expected_text, expected_error) in [
        (
            0,
            TransformAfter {
                text: Some("filtered by policy"),
                is_error: None,
            },
            Some("filtered by policy"),
            false,
        ),
        (
            1,
            TransformAfter {
                text: None,
                is_error: Some(true),
            },
            None,
            true,
        ),
    ] {
        gs.middleware_chain = Arc::new(
            crate::gateway::middleware::MiddlewareChain::new().with_after(Arc::new(middleware)),
        );
        let req = request(
            "tools/call",
            json!(format!("rich-image-transform-{index}")),
            Some(json!({
                "name": "call",
                "arguments": {"tool_slug": tool_slug, "arguments": {}}
            })),
        );

        let response = dispatch_single_request(&gs, &req, "test-session", &HeaderMap::new())
            .await
            .expect("request has id");
        let content = response["result"]["content"].as_array().unwrap();
        assert_eq!(
            content.len(),
            1,
            "middleware must re-approve native content"
        );
        assert_eq!(response["result"]["isError"], expected_error);
        if let Some(expected_text) = expected_text {
            assert_eq!(content[0]["text"], expected_text);
        }
    }
}

#[tokio::test]
async fn after_call_rejection_drops_preextracted_native_image() {
    let (mut gs, _sink, _registry_dir, _shutdown, tool_slug) = rich_image_gateway_state().await;
    gs.middleware_chain = Arc::new(
        crate::gateway::middleware::MiddlewareChain::new().with_after(Arc::new(RejectAfter)),
    );
    let req = request(
        "tools/call",
        json!("rich-image-rejected"),
        Some(json!({
            "name": "call",
            "arguments": {"tool_slug": tool_slug, "arguments": {}}
        })),
    );

    let response = dispatch_single_request(&gs, &req, "test-session", &HeaderMap::new())
        .await
        .expect("request has id");
    let content = response["result"]["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(response["result"]["isError"], true);
    assert!(
        content[0]["text"]
            .as_str()
            .unwrap()
            .contains("image blocked")
    );
    assert!(
        !serde_json::to_string(content)
            .unwrap()
            .contains(TEST_PNG_BASE64)
    );
}

#[test]
fn malformed_rich_image_is_redacted_without_native_content() {
    let raw = json!({
        "output": {
            "context": {
                "__rich__": {
                    "kind": "image",
                    "mime": "image/png",
                    "data": "not valid base64"
                }
            }
        }
    })
    .to_string();

    let (safe_text, images) = extract_native_rich_images(raw);

    assert!(images.is_empty());
    assert!(!safe_text.contains("not valid base64"));
    let safe: Value = serde_json::from_str(&safe_text).unwrap();
    assert_eq!(
        safe.pointer("/output/context/__rich__/data")
            .and_then(Value::as_str),
        Some(INVALID_IMAGE_PLACEHOLDER)
    );
    assert_eq!(
        safe.pointer("/output/context/__rich__/native_image_error")
            .and_then(Value::as_str),
        Some("invalid base64 image data")
    );
}

#[test]
fn sidecar_native_mcp_image_is_promoted_without_base64_in_outer_text() {
    let raw = json!({
        "content": [
            {"type": "text", "text": "captured"},
            {"type": "image", "mimeType": "image/png", "data": TEST_PNG_BASE64}
        ],
        "isError": false
    })
    .to_string();

    let (safe_text, images) = extract_native_rich_images(raw);

    assert_eq!(images.len(), 1);
    assert_eq!(images[0]["type"], "image");
    assert_eq!(images[0]["mimeType"], "image/png");
    assert_eq!(images[0]["data"], TEST_PNG_BASE64);
    assert!(!safe_text.contains(TEST_PNG_BASE64));
    let safe: Value = serde_json::from_str(&safe_text).unwrap();
    assert_eq!(safe["content"][1]["data"], NATIVE_IMAGE_PLACEHOLDER);
}

#[tokio::test]
async fn mcp_audit_deeply_redacts_ui_text_and_credentials_without_metadata() {
    let sink = Arc::new(CaptureSink::default());
    let audit = Arc::new(crate::gateway::middleware::AuditMiddleware::new(
        sink.clone(),
    ));
    let mut gs = test_gateway_state();
    gs.middleware_chain = Arc::new(
        crate::gateway::middleware::MiddlewareChain::new()
            .with_before(audit.clone())
            .with_after(audit),
    );
    let req = request(
        "tools/call",
        json!("sensitive-input"),
        Some(json!({
            "name": "call",
            "arguments": {
                "tool_slug": "maya.missing.ui_control__act",
                "arguments": {
                    "action": "set_text",
                    "text": "private typed value",
                    "password": "password-value",
                    "access_token": "token-value"
                }
            }
        })),
    );

    let response = dispatch_single_request(&gs, &req, "session", &HeaderMap::new())
        .await
        .unwrap();
    let encoded_response = serde_json::to_string(&response).unwrap();
    let entries = sink.0.lock().unwrap();
    let input = &entries[0].input_payload.as_ref().unwrap().content;

    for secret in ["private typed value", "password-value", "token-value"] {
        assert!(!input.contains(secret));
        assert!(!encoded_response.contains(secret));
    }
    assert!(input.contains("[REDACTED_SENSITIVE_INPUT]"));
}

#[tokio::test]
async fn initialize_client_info_flows_to_mcp_call_admin_stats() {
    let trace_log = Arc::new(crate::gateway::admin::TraceLog::new(10));
    let audit_log: Arc<crate::gateway::admin::AuditLog> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = Arc::new(
        crate::gateway::admin::AdminAuditSink::new(audit_log, 10).with_trace_log(trace_log.clone()),
    );
    let audit = Arc::new(crate::gateway::middleware::AuditMiddleware::new(sink));
    let mut gs = test_gateway_state();
    gs.middleware_chain = Arc::new(
        crate::gateway::middleware::MiddlewareChain::new()
            .with_before(audit.clone())
            .with_after(audit),
    );
    let init = request(
        "initialize",
        json!("init"),
        Some(json!({
            "protocolVersion": "2025-03-26",
            "clientInfo": {"name": "Codex Desktop", "version": "1.2.3"}
        })),
    );
    let init_response = dispatch_single_request(&gs, &init, "client-session-a", &HeaderMap::new())
        .await
        .expect("initialize request has id");
    assert_eq!(
        init_response["result"]["serverInfo"]["name"],
        "test-gateway"
    );

    let call = request(
        "tools/call",
        json!("client-call"),
        Some(json!({
            "name": "search",
            "arguments": {"kind": "tool", "query": "sphere"}
        })),
    );
    let call_response = dispatch_single_request(&gs, &call, "client-session-a", &HeaderMap::new())
        .await
        .expect("call request has id");
    assert_eq!(call_response["result"]["isError"], false);

    let traces = trace_log.recent(10);
    assert_eq!(traces.len(), 1);
    let agent = traces[0]
        .agent_context
        .as_ref()
        .expect("MCP call should inherit initialize client attribution");
    assert_eq!(agent.agent_name.as_deref(), Some("Codex Desktop"));
    assert_eq!(agent.agent_version.as_deref(), Some("1.2.3"));
    assert_eq!(agent.agent_kind.as_deref(), Some("mcp-client"));
    assert_eq!(agent.client_platform.as_deref(), Some("Codex Desktop"));

    let stats = crate::gateway::admin::StatsAggregator::new(trace_log)
        .compute(crate::gateway::admin::StatsRange::All);
    assert_eq!(stats.top_agents[0].name, "Codex Desktop@1.2.3");
    assert_eq!(stats.top_client_platforms[0].name, "Codex Desktop");
}

#[tokio::test]
async fn tools_call_audit_records_compact_token_accounting() {
    let sink = Arc::new(CaptureSink::default());
    let audit_middleware = Arc::new(crate::gateway::middleware::AuditMiddleware::new(
        sink.clone(),
    ));
    let mut gs = test_gateway_state();
    gs.middleware_chain = Arc::new(
        crate::gateway::middleware::MiddlewareChain::new()
            .with_before(audit_middleware.clone())
            .with_after(audit_middleware),
    );
    let req = request(
        "tools/call",
        json!("compact-audit"),
        Some(json!({
            "name": "call",
            "arguments": {
                "tool_slug": "maya.abcdef01.render",
                "arguments": {}
            },
            "_meta": {"response_format": "toon"}
        })),
    );

    let response = dispatch_single_request(&gs, &req, "test-session", &HeaderMap::new())
        .await
        .expect("request has id");

    assert_eq!(
        response["result"]["_meta"]["token_accounting"]["response_format"],
        "toon"
    );
    let entries = sink.0.lock().unwrap();
    let tokens = entries[0]
        .token_accounting
        .as_ref()
        .expect("MCP audit should capture compact token accounting");
    assert_eq!(tokens.response_format, "toon");
    assert_eq!(tokens.token_estimator, "dcc-mcp-byte4-v1");
    assert!(tokens.original_tokens >= tokens.returned_tokens);
}

#[tokio::test]
async fn tools_call_compact_preserves_text_error_payloads() {
    let req = request(
        "tools/call",
        json!("describe-error"),
        Some(json!({
            "name": "describe",
            "arguments": {},
            "_meta": {"response_format": "toon"}
        })),
    );
    let response = dispatch(&req).await;

    assert_eq!(response["result"]["isError"], true);
    assert_eq!(response["result"]["content"][0]["mimeType"], TOON_MIME);
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("compact tool error has text");
    let decoded: Value = toon_format::decode_default(text).expect("tool error is TOON");
    assert!(
        decoded["text"]
            .as_str()
            .is_some_and(|message| message.contains("describe requires")),
        "compact error should preserve original message, got {decoded}"
    );
}

#[tokio::test]
async fn compact_tools_call_keeps_json_rpc_errors_unchanged() {
    let req = request(
        "tools/call",
        json!(4),
        Some(json!({
            "name": "search",
            "arguments": [1, 2, 3],
            "_meta": {"response_format": "toon"}
        })),
    );
    let response = dispatch(&req).await;

    assert_eq!(
        response["error"]["code"],
        dcc_mcp_jsonrpc::error_codes::INVALID_PARAMS
    );
    assert!(response.get("result").is_none());
    assert!(response["error"].get("_meta").is_none());
}

// ── kind=all compact tests (PIP-2454 regression) ──────────────────

#[test]
fn compact_kind_all_preserves_tools_and_skills_subtrees() {
    // Simulate a kind=all response payload (as produced by tools.rs
    // after compact_tools_hits + compact_skills_list).
    // The compact_tool_text_payload must detect the kind=all shape
    // and preserve both tools and skills subtrees instead of
    // falling through to legacy_payload.clone().
    let payload = json!({
        "search_id": "search-001",
        "ranker_version": "v2.1.0",
        "index_generation": "gen-001",
        "tools": {
            "total": 2,
            "hits": [
                {"tool_slug": "blender.abc.create_cube", "backend_tool": "create_cube", "dcc_type": "blender", "score": 98, "loaded": true},
                {"tool_slug": "blender.abc.render", "backend_tool": "render", "dcc_type": "blender", "score": 85, "loaded": true}
            ],
            "capped": false
        },
        "skills": {
            "skills": [
                {"tool_slug": "blender.abc.blender-export", "skill_name": "blender-export", "dcc_type": "blender", "score": 90},
                {"tool_slug": "blender.abc.blender-import", "skill_name": "blender-import", "dcc_type": "blender", "score": 80}
            ],
            "total": 2,
            "capped": false
        }
    });

    let compact = compact_tool_text_payload(Some("search"), &payload);

    // Must have both tools and skills keys (not just tools alone)
    assert!(
        compact.get("tools").is_some(),
        "kind=all compact must keep tools"
    );
    assert!(
        compact.get("skills").is_some(),
        "kind=all compact must keep skills"
    );

    // Must strip the raw text metadata fields that are only
    // part of the legacy full response
    let compact_str = serde_json::to_string(&compact).unwrap();
    assert!(
        compact_str.len() < 2000,
        "kind=all compact payload must be well under 10K chars (got {} chars)",
        compact_str.len()
    );
}

#[test]
fn compact_kind_all_strips_extra_metadata_keys() {
    // kind=all response may carry extra keys from the internal
    // response serialization. compact must only keep the five
    // canonical keys.
    let payload = json!({
        "search_id": "search-001",
        "ranker_version": "v2.1.0",
        "index_generation": "gen-001",
        "tools": {"total": 0, "hits": []},
        "skills": {"skills": [], "total": 0},
        "internal_scratch": "should-be-dropped",
        "raw_jsonrpc": "should-be-dropped",
    });

    let compact = compact_tool_text_payload(Some("search"), &payload);

    assert_eq!(compact.as_object().unwrap().len(), 5);
    assert!(compact.get("internal_scratch").is_none());
    assert!(compact.get("raw_jsonrpc").is_none());
    assert!(compact.get("search_id").is_some());
    assert!(compact.get("tools").is_some());
    assert!(compact.get("skills").is_some());
}

#[test]
fn compact_single_kind_hits_still_works() {
    // Regression: single-kind (hits-based) compact path must
    // not be broken by the kind=all addition.
    let payload = json!({
        "total": 3,
        "hits": [
            {"tool_slug": "maya.abc.sphere", "backend_tool": "sphere", "callable_id": "sphere", "dcc_type": "maya", "score": 99, "tags": ["modeling"], "loaded": true},
            {"tool_slug": "maya.abc.cube", "backend_tool": "cube", "callable_id": "cube", "dcc_type": "maya", "score": 80, "tags": [], "loaded": true},
            {"tool_slug": "maya.abc.render", "backend_tool": "render", "callable_id": "render", "dcc_type": "maya", "score": 60, "tags": [], "loaded": true}
        ]
    });

    let compact = compact_tool_text_payload(Some("search"), &payload);

    assert_eq!(compact["total"], 3);
    assert_eq!(compact["hits"].as_array().unwrap().len(), 3);
    // compact_search_payload strips redundant callable_id
    // (callable_id == backend_tool, so it's omitted)
    assert!(compact["hits"][0].get("callable_id").is_none());
    // but preserves tool_slug
    assert_eq!(compact["hits"][0]["tool_slug"], "maya.abc.sphere");
}
