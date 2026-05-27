use super::*;
use axum::body::{Body, to_bytes};
use axum::http::Request;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{RwLock, broadcast, watch};
use tower::ServiceExt;

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
        http_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::http_registration::HttpInstanceRegistry::default(),
        )),
        mdns_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::mdns_discovery::MdnsInstanceRegistry::default(),
        )),
        relay_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::relay_discovery::RelayInstanceRegistry::default(),
        )),
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
        client_attribution: Arc::new(
            crate::gateway::caller_attribution::ClientAttributionStore::default(),
        ),
        pending_calls: Arc::new(RwLock::new(HashMap::new())),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        policy: Arc::new(crate::gateway::GatewayPolicy::default()),
        security: Arc::new(crate::gateway::GatewaySecurityPolicy::disabled()),
        adapter_version: None,
        adapter_dcc: None,
        capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
        middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
        instance_diagnostics: Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        search_telemetry: Arc::new(crate::gateway::search_telemetry::SearchTelemetryStore::new()),
        debug_routes_enabled,
        #[cfg(feature = "prometheus")]
        gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
    }
}

async fn response_json(resp: Response) -> (StatusCode, Value) {
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body = response_value(&headers, &bytes);
    (status, body)
}

async fn response_json_with_headers(resp: Response) -> (StatusCode, HeaderMap, Value) {
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body = response_value(&headers, &bytes);
    (status, headers, body)
}

fn response_value(headers: &HeaderMap, bytes: &[u8]) -> Value {
    let is_toon = headers
        .get(crate::gateway::response_codec::HEADER_RESPONSE_FORMAT)
        .and_then(|value| value.to_str().ok())
        == Some("toon")
        || headers
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with(crate::gateway::response_codec::TOON_MIME));
    if is_toon {
        let text = std::str::from_utf8(bytes).unwrap();
        toon_format::decode_default(text).unwrap()
    } else {
        serde_json::from_slice(bytes).unwrap()
    }
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

fn trace_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("x-request-id", "req-rest-meta".parse().unwrap());
    headers.insert(
        "traceparent",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
            .parse()
            .unwrap(),
    );
    headers
}

fn attributed_trace_headers() -> HeaderMap {
    let mut headers = trace_headers();
    headers.insert("x-dcc-mcp-actor-id", "artist-1".parse().unwrap());
    headers.insert(
        crate::gateway::caller_attribution::INTERNAL_SOURCE_IP_HEADER,
        "192.0.2.44".parse().unwrap(),
    );
    headers.insert(
        crate::gateway::caller_attribution::INTERNAL_FORWARDED_FOR_HEADER,
        "198.51.100.7, 203.0.113.9".parse().unwrap(),
    );
    headers
}

fn assert_trace_headers(headers: &HeaderMap) {
    assert_eq!(
        headers
            .get(crate::gateway::response_codec::HEADER_REQUEST_ID)
            .and_then(|value| value.to_str().ok()),
        Some("req-rest-meta")
    );
    assert_eq!(
        headers
            .get(crate::gateway::response_codec::HEADER_TRACE_ID)
            .and_then(|value| value.to_str().ok()),
        Some("4bf92f3577b34da6a3ce929d0e0e4736")
    );
    assert!(
        headers
            .get(crate::gateway::response_codec::HEADER_INDEX_GENERATION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| !value.is_empty())
    );
}

fn assert_body_metadata(body: &Value) {
    assert_eq!(body["request_id"], "req-rest-meta");
    assert_eq!(body["trace_id"], "4bf92f3577b34da6a3ce929d0e0e4736");
    assert!(
        body["index_generation"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
}

fn seed_unloaded_render_capability(gs: &GatewayState) {
    gs.capability_index.set_unloaded_records(vec![
        crate::gateway::capability::CapabilityRecord::from_skill_tool(
            "maya-render",
            "render",
            "Render the current scene",
            "maya",
        ),
    ]);
}

fn policy_record(
    dcc_type: &str,
    instance_id: uuid::Uuid,
    tool: &str,
    skill_name: &str,
    read_only: bool,
) -> crate::gateway::capability::CapabilityRecord {
    crate::gateway::capability::CapabilityRecord::new(
        crate::gateway::capability::tool_slug(dcc_type, &instance_id, tool),
        tool.to_string(),
        tool.to_string(),
        Some(skill_name.to_string()),
        "Policy test capability",
        Vec::new(),
        dcc_type.to_string(),
        instance_id,
        true,
        true,
    )
    .with_surface_metadata(
        Some(crate::gateway::capability::CapabilityAnnotations {
            title: None,
            read_only_hint: Some(read_only),
            destructive_hint: Some(!read_only),
            idempotent_hint: None,
            open_world_hint: None,
        }),
        None,
    )
}

async fn seed_policy_records(gs: &GatewayState) -> (String, String, String, String) {
    let maya = uuid::Uuid::parse_str("abcdef01-2345-6789-abcd-ef0123456789").unwrap();
    let custom = uuid::Uuid::parse_str("12345678-1234-5678-9abc-123456789abc").unwrap();
    {
        let registry = gs.registry.read().await;
        let mut maya_entry = ServiceEntry::new("maya", "127.0.0.1", 18801);
        maya_entry.instance_id = maya;
        registry.register(maya_entry).unwrap();
        let mut custom_entry = ServiceEntry::new("customhost", "127.0.0.1", 18802);
        custom_entry.instance_id = custom;
        registry.register(custom_entry).unwrap();
    }
    let maya_read = policy_record("maya", maya, "safe_read_scene", "safe-maya", true);
    let custom_read = policy_record("customhost", custom, "safe_read_state", "safe-custom", true);
    let maya_write = policy_record("maya", maya, "unsafe_write_scene", "unsafe-maya", false);
    let custom_write = policy_record(
        "customhost",
        custom,
        "safe_write_state",
        "safe-custom",
        false,
    );
    let maya_read_slug = maya_read.tool_slug.clone();
    let custom_read_slug = custom_read.tool_slug.clone();
    let maya_write_slug = maya_write.tool_slug.clone();
    let custom_write_slug = custom_write.tool_slug.clone();
    gs.capability_index.upsert_instance(
        maya,
        vec![maya_read, maya_write],
        crate::gateway::capability::InstanceFingerprint(1),
    );
    gs.capability_index.upsert_instance(
        custom,
        vec![custom_read, custom_write],
        crate::gateway::capability::InstanceFingerprint(2),
    );
    (
        maya_read_slug,
        custom_read_slug,
        maya_write_slug,
        custom_write_slug,
    )
}

fn seed_policy_unloaded_records(gs: &GatewayState) -> (String, String, String, String) {
    let maya = uuid::Uuid::parse_str("abcdef01-2345-6789-abcd-ef0123456789").unwrap();
    let custom = uuid::Uuid::parse_str("12345678-1234-5678-9abc-123456789abc").unwrap();
    let mut maya_read = policy_record("maya", maya, "safe_read_scene", "safe-maya", true);
    let mut custom_read =
        policy_record("customhost", custom, "safe_read_state", "safe-custom", true);
    let mut maya_write = policy_record("maya", maya, "unsafe_write_scene", "unsafe-maya", false);
    let mut custom_write = policy_record(
        "customhost",
        custom,
        "safe_write_state",
        "safe-custom",
        false,
    );
    maya_read.loaded = false;
    custom_read.loaded = false;
    maya_write.loaded = false;
    custom_write.loaded = false;
    let maya_read_slug = maya_read.tool_slug.clone();
    let custom_read_slug = custom_read.tool_slug.clone();
    let maya_write_slug = maya_write.tool_slug.clone();
    let custom_write_slug = custom_write.tool_slug.clone();
    gs.capability_index.set_unloaded_records(vec![
        maya_read,
        custom_read,
        maya_write,
        custom_write,
    ]);
    (
        maya_read_slug,
        custom_read_slug,
        maya_write_slug,
        custom_write_slug,
    )
}

fn policy_for_safe_reads() -> crate::gateway::GatewayPolicy {
    crate::gateway::GatewayPolicy {
        allowed_dcc_types: vec!["maya".to_string(), "customhost".to_string()],
        allowed_skill_families: vec!["safe-".to_string()],
        allowed_tool_slug_prefixes: vec![
            "maya.abcdef01.safe_read".to_string(),
            "customhost.12345678.safe_read".to_string(),
        ],
        ..Default::default()
    }
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
    assert!(!body.contains("/v1/dcc/{dcc_type}/call"));
}

#[tokio::test]
async fn gateway_openapi_lists_gateway_routes_not_per_dcc_routes() {
    let (status, doc) = response_json(
        handle_v1_openapi(State(test_gateway_state("1.2.3")))
            .await
            .into_response(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let paths = doc["paths"].as_object().expect("paths object");
    for route in crate::gateway::rest_openapi::GATEWAY_OPENAPI_ROUTES {
        let path_item = paths.get(route.path);
        assert!(
            path_item.is_some(),
            "gateway OpenAPI doc missing {} {}: {doc:#}",
            route.method,
            route.path
        );
        assert!(
            path_item
                .and_then(|item| item.get(route.method.to_ascii_lowercase()))
                .is_some(),
            "gateway OpenAPI doc missing operation {} {}: {doc:#}",
            route.method,
            route.path
        );
    }
    for forbidden in [
        "/v1/resources",
        "/v1/resources/{uri}",
        "/v1/prompts",
        "/v1/prompts/{name}",
        "/v1/jobs/{id}/events",
        "/v1/jobs/{id}",
        "/v1/dcc/{dcc_type}/call",
    ] {
        assert!(
            paths.get(forbidden).is_none(),
            "gateway OpenAPI doc must not advertise per-DCC-only path {forbidden}: {doc:#}"
        );
    }
}

#[tokio::test]
async fn gateway_openapi_canonical_routes_are_mounted_by_router() {
    let app = crate::gateway::router::build_gateway_router(test_gateway_state("1.2.3"));

    for route in crate::gateway::rest_openapi::GATEWAY_OPENAPI_ROUTES {
        let uri = materialized_gateway_route(route.path);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(route.method)
                    .uri(uri.as_str())
                    .header("content-type", "application/json")
                    .body(route_body(route.path))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{} {} is documented but mounted with a different method",
            route.method,
            route.path
        );
        if status == StatusCode::NOT_FOUND {
            assert!(
                !body.is_empty(),
                "{} {} returned Axum's empty 404 and is likely not mounted",
                route.method,
                route.path
            );
        }
    }
}

fn materialized_gateway_route(path: &str) -> String {
    let mut materialized = path
        .replace("{slug}", "maya.abcdef01.example_tool")
        .replace("{dcc_type}", "maya")
        .replace("{instance_id}", "abcdef01");
    if path == "/v1/dcc/{dcc_type}/instances/{instance_id}/describe" {
        materialized.push_str("?backend_tool=example_tool");
    }
    materialized
}

fn route_body(path: &str) -> Body {
    let body = match path {
        "/v1/call_batch" => r#"{"calls":[]}"#,
        "/v1/dcc/{dcc_type}/instances/{instance_id}/call" => {
            r#"{"backend_tool":"example_tool","arguments":{}}"#
        }
        _ => "{}",
    };
    Body::from(body)
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
        "/v1/debug/traffic",
        "/v1/debug/traffic/export",
        "/v1/debug/traces/{request_id}",
        "/v1/debug/trace-context/{lookup_id}",
        "/v1/debug/agent-traces/{lookup_id}",
        "/v1/debug/tasks",
        "/v1/debug/bundles/{request_id}",
        "/v1/debug/issue-reports/{request_id}",
        "/v1/debug/logs",
        "/v1/debug/deregistered",
        "/v1/debug/stats",
        "/v1/debug/search-telemetry",
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
    assert!(
        doc["paths"]["/v1/debug/bundles/{request_id}"]["get"]["responses"]["200"]["content"]
            .get(crate::gateway::response_codec::TOON_MIME)
            .is_some()
    );
    assert!(
        doc["paths"]["/v1/debug/bundles/{request_id}"]["get"]["parameters"]
            .as_array()
            .is_some_and(|params| params.iter().any(|param| param["name"] == "compact"))
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
async fn gateway_rest_workflow_responses_expose_trace_and_index_metadata() {
    let gs = test_gateway_state("1.2.3");
    seed_unloaded_render_capability(&gs);

    let (status, response_headers, search) = response_json_with_headers(
        handle_v1_search(
            State(gs.clone()),
            trace_headers(),
            Json(json!({"query": "render"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_trace_headers(&response_headers);
    assert_body_metadata(&search);
    let search_id = search["search_id"]
        .as_str()
        .expect("search response should expose search_id")
        .to_string();
    assert_eq!(
        response_headers
            .get(crate::gateway::response_codec::HEADER_SEARCH_ID)
            .and_then(|value| value.to_str().ok()),
        Some(search_id.as_str())
    );
    assert_eq!(
        response_headers
            .get(crate::gateway::response_codec::HEADER_RANKER_VERSION)
            .and_then(|value| value.to_str().ok()),
        Some(crate::gateway::search_telemetry::RANKER_VERSION)
    );
    assert_eq!(
        search["ranker_version"],
        crate::gateway::search_telemetry::RANKER_VERSION
    );
    assert_eq!(search["hits"][0]["rank"], 1);
    assert_eq!(
        search["hits"][0]["next_step"]["arguments"]["meta"]["search_id"],
        search_id
    );
    let slug = search["hits"][0]["tool_slug"]
        .as_str()
        .expect("search should return seeded render capability")
        .to_string();
    let search_meta = json!({"search_id": search_id});

    let (status, response_headers, describe) = response_json_with_headers(
        handle_v1_describe(
            State(gs.clone()),
            trace_headers(),
            Json(json!({"tool_slug": slug.clone(), "meta": search_meta.clone()})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_trace_headers(&response_headers);
    assert_body_metadata(&describe);

    let (status, response_headers, load) = response_json_with_headers(
        handle_v1_load_skill(
            State(gs.clone()),
            trace_headers(),
            Json(json!({"skill_name": "maya-render", "dcc_type": "maya", "meta": search_meta.clone()})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_trace_headers(&response_headers);
    assert_body_metadata(&load);

    let (status, response_headers, call) = response_json_with_headers(
        handle_v1_call(
            State(gs.clone()),
            trace_headers(),
            Json(json!({"tool_slug": slug.clone(), "arguments": {}, "meta": search_meta.clone()})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_trace_headers(&response_headers);
    assert!(
        call.get("request_id").is_none(),
        "/v1/call must keep backend/error envelope bodies unwrapped"
    );

    let (status, response_headers, batch) = response_json_with_headers(
        handle_v1_call_batch(
            State(gs.clone()),
            trace_headers(),
            Json(json!({"calls": [{"id": "client-step-1", "tool_slug": slug.clone(), "arguments": {}, "meta": search_meta.clone()}]})),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_trace_headers(&response_headers);
    assert_body_metadata(&batch);
    assert_eq!(batch["results"][0]["index"], 0);
    assert_eq!(batch["results"][0]["id"], "client-step-1");
    assert_eq!(batch["results"][0]["ok"], false);

    let telemetry = gs.search_telemetry.snapshot(10);
    assert_eq!(telemetry.stats.total_searches, 1);
    assert_eq!(telemetry.stats.describe_after_search_rate, 1.0);
    assert_eq!(telemetry.stats.load_after_search_rate, 1.0);
    assert_eq!(telemetry.stats.call_after_search_rate, 1.0);
    assert_eq!(telemetry.stats.success_after_search_rate, 0.0);
    assert_eq!(telemetry.stats.top1_hit_rate, 1.0);
    assert!(
        telemetry.recent[0]
            .followups
            .iter()
            .any(|followup| followup.kind == "call")
    );
}

#[tokio::test]
async fn rest_call_rejects_missing_gateway_token_when_security_enabled() {
    let mut state = test_gateway_state("1.2.3");
    state.security = Arc::new(crate::gateway::GatewaySecurityPolicy::new(
        crate::gateway::GatewaySecurityConfig::with_api_keys(["secret-token"]),
    ));

    let (status, body) = response_json(
        handle_v1_call(
            State(state),
            HeaderMap::new(),
            Json(json!({
                "tool_slug": "maya.12345678.modeling__create_cube",
                "arguments": {}
            })),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthorized");
    assert_eq!(body["error_detail"]["kind"], "unauthorized");
}

#[tokio::test]
async fn rest_search_records_server_network_attribution() {
    let gs = test_gateway_state("1.2.3");
    let headers = attributed_trace_headers();

    let (status, _body) = response_json(
        handle_v1_search(
            State(gs.clone()),
            headers,
            Json(json!({
                "query": "render",
                "meta": {
                    "agent_context": {
                        "client_platform": "custom-http",
                        "sourceIp": "203.0.113.100"
                    }
                }
            })),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let telemetry = gs.search_telemetry.snapshot(10);
    let agent = telemetry.recent[0]
        .agent_context
        .as_ref()
        .expect("search telemetry should keep attribution");
    assert_eq!(agent.actor_id.as_deref(), Some("artist-1"));
    assert_eq!(agent.client_platform.as_deref(), Some("custom-http"));
    assert_eq!(agent.source_ip.as_deref(), Some("192.0.2.44"));
    assert_eq!(
        agent.forwarded_for,
        vec!["198.51.100.7".to_string(), "203.0.113.9".to_string()]
    );
}

#[tokio::test]
async fn mcp_search_followups_correlate_describe_load_call_and_batch() {
    let gs = test_gateway_state("1.2.3");
    seed_unloaded_render_capability(&gs);
    let trace = crate::gateway::admin::trace::TraceContext::from_headers(&trace_headers());

    let search_args = json!({"query": "render"});
    let (search_text, search_is_error) = crate::gateway::aggregator::route_tools_call(
        &gs,
        "search",
        &search_args,
        None,
        Some("session-mcp"),
        Some(&trace),
        None,
    )
    .await;
    assert!(!search_is_error);
    let search: Value = serde_json::from_str(&search_text).unwrap();
    let search_id = search["search_id"].as_str().unwrap().to_string();
    let slug = search["hits"][0]["tool_slug"].as_str().unwrap().to_string();
    assert_eq!(search["hits"][0]["rank"], 1);
    assert_eq!(
        search["hits"][0]["next_step"]["arguments"]["meta"]["search_id"],
        search_id
    );

    let meta = json!({"search_id": search_id});
    let describe_args = json!({"tool_slug": slug.clone()});
    let (_describe_text, describe_is_error) = crate::gateway::aggregator::route_tools_call(
        &gs,
        "describe",
        &describe_args,
        Some(&meta),
        Some("session-mcp"),
        Some(&trace),
        None,
    )
    .await;
    assert!(describe_is_error);

    let load_args = json!({"skill_name": "maya-render", "dcc_type": "maya"});
    let (_load_text, load_is_error) = crate::gateway::aggregator::route_tools_call(
        &gs,
        "load_skill",
        &load_args,
        Some(&meta),
        Some("session-mcp"),
        Some(&trace),
        None,
    )
    .await;
    assert!(load_is_error);

    let call_args = json!({"tool_slug": slug.clone(), "arguments": {}});
    let (_call_text, call_is_error) = crate::gateway::aggregator::route_tools_call(
        &gs,
        "call",
        &call_args,
        Some(&meta),
        Some("session-mcp"),
        Some(&trace),
        None,
    )
    .await;
    assert!(call_is_error);

    let batch_args =
        json!({"calls": [{"id": "batch-1", "tool_slug": slug, "arguments": {}, "meta": meta}]});
    let (_batch_text, batch_is_error) = crate::gateway::aggregator::route_tools_call(
        &gs,
        "call",
        &batch_args,
        None,
        Some("session-mcp"),
        Some(&trace),
        None,
    )
    .await;
    assert!(batch_is_error);

    let telemetry = gs.search_telemetry.snapshot(10);
    assert_eq!(telemetry.stats.total_searches, 1);
    assert_eq!(telemetry.stats.describe_after_search_rate, 1.0);
    assert_eq!(telemetry.stats.load_after_search_rate, 1.0);
    assert_eq!(telemetry.stats.call_after_search_rate, 1.0);
    assert_eq!(telemetry.stats.top1_hit_rate, 1.0);
    assert!(
        telemetry.recent[0]
            .followups
            .iter()
            .filter(|followup| followup.kind == "call")
            .count()
            >= 2
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
async fn gateway_yield_broadcasts_handoff_and_marks_sentinel_shutting_down() {
    let gs = test_gateway_state("1.2.3");
    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
    sentinel.version = Some("1.2.3".to_string());
    sentinel
        .metadata
        .insert("gateway_role".to_string(), "active".to_string());
    let sentinel_key = sentinel.key();
    let sentinel_id = sentinel.instance_id.to_string();
    {
        let registry = gs.registry.read().await;
        registry.register(sentinel).unwrap();
    }

    let mut events_rx = gs.events_tx.subscribe();
    let (status, body) = response_json(
        handle_gateway_yield(
            State(gs.clone()),
            axum::body::Bytes::from_static(
                br#"{"challenger_version":"1.2.4","suggested_successor":"peer-123"}"#,
            ),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["handoff"], true);

    let raw_event = tokio::time::timeout(Duration::from_secs(1), events_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let event: Value = serde_json::from_str(&raw_event).unwrap();
    assert_eq!(event["method"], "notifications/gateway/handoff");
    assert_eq!(event["params"]["from"], sentinel_id);
    assert_eq!(event["params"]["reason"], "version_preempt");
    assert_eq!(event["params"]["challenger_version"], "1.2.4");
    assert_eq!(event["params"]["suggested_successor"], "peer-123");
    assert_eq!(event["params"]["endpoint_after_handoff_will_be_same"], true);
    assert!(event["params"]["deadline_unix_secs"].as_f64().unwrap() > 0.0);

    {
        let registry = gs.registry.read().await;
        let updated = registry.get(&sentinel_key).unwrap();
        assert_eq!(updated.status, ServiceStatus::ShuttingDown);
    }

    let events = gs.event_log.recent_events(1);
    assert_eq!(
        events[0].event,
        crate::gateway::event_log::EventKind::VoluntaryYield
    );
    assert_eq!(events[0].dcc_type, GATEWAY_SENTINEL_DCC_TYPE);
    assert!(
        events[0]
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("challenger_version=1.2.4")
    );
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
async fn rest_call_batch_quota_rejection_returns_throttled_429() {
    use crate::gateway::middleware::{MiddlewareChain, QuotaMiddleware};

    let mut gs = test_gateway_state("1.2.3");
    gs.middleware_chain =
        Arc::new(MiddlewareChain::new().with_before(Arc::new(QuotaMiddleware::new(0))));

    let (status, body) = response_json(
        handle_v1_call_batch(
            State(gs),
            HeaderMap::new(),
            Json(json!({
                "calls": [{
                    "tool_slug": "maya.abcdef01.render",
                    "arguments": {}
                }]
            })),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["success"], false);
    assert_eq!(body["error"]["kind"], "throttled");
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
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::ACCEPT,
        "application/toon".parse().unwrap(),
    );

    let result = call_service_with_admin_trace(
        &gs,
        &headers,
        RestCallTraceRequest {
            method: "v1/call",
            slug: "maya.abcdef01.render",
            arguments: request_body["arguments"].clone(),
            meta: request_body.get("meta").cloned(),
            request_body: &request_body,
            trace_context: crate::gateway::admin::trace::TraceContext::from_headers(&headers),
        },
    )
    .await;

    assert!(result.is_err());
    let entries = sink.0.lock().unwrap();
    assert_eq!(entries.len(), 1);
    let input = entries[0].input_payload.as_ref().unwrap().content.clone();
    assert!(input.contains("[REDACTED]"));
    assert!(!input.contains("secret-key"));
    assert!(!input.contains("secret-token"));
    let tokens = entries[0]
        .token_accounting
        .as_ref()
        .expect("REST audit should capture compact token accounting");
    assert_eq!(tokens.response_format, "toon");
    assert_eq!(tokens.token_estimator, "dcc-mcp-byte4-v1");
    assert!(tokens.original_tokens >= tokens.returned_tokens);
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
        "meta": {},
        "response_format": "json"
    });

    let _ = call_service_with_admin_trace(
        &gs,
        &headers,
        RestCallTraceRequest {
            method: "v1/call",
            slug: "maya.abcdef01.render",
            arguments: json!({}),
            meta: request_body.get("meta").cloned(),
            request_body: &request_body,
            trace_context: crate::gateway::admin::trace::TraceContext::from_headers(&headers),
        },
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
    let tokens = entries[0]
        .token_accounting
        .as_ref()
        .expect("REST audit should capture legacy JSON token accounting");
    assert_eq!(tokens.response_format, "json");
    assert_eq!(tokens.saved_tokens, 0);
    assert_eq!(tokens.original_tokens, tokens.returned_tokens);
}

#[tokio::test]
async fn rest_audit_rows_include_server_network_attribution() {
    use crate::gateway::middleware::{AuditMiddleware, MiddlewareChain};

    let sink = Arc::new(CaptureSink::default());
    let chain = MiddlewareChain::new().with_after(Arc::new(AuditMiddleware::new(sink.clone())));
    let mut gs = test_gateway_state("1.2.3");
    gs.middleware_chain = Arc::new(chain);

    let headers = attributed_trace_headers();
    let request_body = json!({
        "tool_slug": "maya.abcdef01.render",
        "arguments": {},
        "meta": {"agent_context": {"agent_id": "agent-1"}},
        "response_format": "json"
    });

    let _ = call_service_with_admin_trace(
        &gs,
        &headers,
        RestCallTraceRequest {
            method: "v1/call",
            slug: "maya.abcdef01.render",
            arguments: json!({}),
            meta: request_body.get("meta").cloned(),
            request_body: &request_body,
            trace_context: crate::gateway::admin::trace::TraceContext::from_headers(&headers),
        },
    )
    .await;

    let entries = sink.0.lock().unwrap();
    let agent = entries[0]
        .agent_context
        .as_ref()
        .expect("audit should keep attribution");
    assert_eq!(agent.agent_id.as_deref(), Some("agent-1"));
    assert_eq!(agent.actor_id.as_deref(), Some("artist-1"));
    assert_eq!(agent.source_ip.as_deref(), Some("192.0.2.44"));
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

    let headers = HeaderMap::new();
    let result = call_batch_with_admin_trace(
        &gs,
        &headers,
        &json!({"calls": []}),
        crate::gateway::admin::trace::TraceContext::from_headers(&headers),
    )
    .await;

    let body = result.expect("batch should use middleware-mutated args");
    assert_eq!(body["results"][0]["tool_slug"], "maya.abcdef01.render");
    let entries = sink.0.lock().unwrap();
    assert_eq!(entries.len(), 1);
    let input = entries[0].input_payload.as_ref().unwrap().content.clone();
    assert!(input.contains("[REDACTED]"));
    assert!(!input.contains("secret-token"));
}

#[tokio::test]
async fn rest_search_hides_capabilities_denied_by_gateway_policy() {
    let mut gs = test_gateway_state("1.2.3");
    gs.policy = Arc::new(policy_for_safe_reads());
    let (maya_read, custom_read, maya_write, custom_write) = seed_policy_unloaded_records(&gs);

    let (status, body) =
        response_json(handle_v1_search(State(gs), HeaderMap::new(), Json(json!({}))).await).await;

    assert_eq!(status, StatusCode::OK);
    let slugs: Vec<&str> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|hit| hit["tool_slug"].as_str())
        .collect();
    assert!(slugs.contains(&maya_read.as_str()));
    assert!(slugs.contains(&custom_read.as_str()));
    assert!(!slugs.contains(&maya_write.as_str()));
    assert!(!slugs.contains(&custom_write.as_str()));
}

#[tokio::test]
async fn rest_describe_rejects_direct_slug_denied_by_gateway_policy() {
    let mut gs = test_gateway_state("1.2.3");
    gs.policy = Arc::new(policy_for_safe_reads());
    let (_, _, maya_write, _) = seed_policy_unloaded_records(&gs);

    let (status, body) = response_json(
        handle_v1_describe(
            State(gs),
            HeaderMap::new(),
            Json(json!({"tool_slug": maya_write})),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["kind"], "policy-denied");
    assert_eq!(body["error"]["policy"]["reason"], "skill-allowlist");
    assert_eq!(body["error"]["policy"]["operation"], "describe");
}

#[tokio::test]
async fn rest_load_skill_is_denied_in_read_only_gateway_policy() {
    let mut gs = test_gateway_state("1.2.3");
    gs.policy = Arc::new(crate::gateway::GatewayPolicy {
        read_only: true,
        ..Default::default()
    });
    let iid = uuid::Uuid::parse_str("abcdef01-2345-6789-abcd-ef0123456789").unwrap();
    {
        let registry = gs.registry.read().await;
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18801);
        entry.instance_id = iid;
        registry.register(entry).unwrap();
    }

    let (status, body) = response_json(
        handle_v1_load_skill(
            State(gs),
            HeaderMap::new(),
            Json(json!({
                "skill_name": "maya-modeling",
                "dcc_type": "maya",
                "instance_id": iid.to_string()
            })),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["kind"], "policy-denied");
    assert_eq!(body["error"]["policy"]["reason"], "read-only");
    assert_eq!(body["error"]["policy"]["operation"], "load_skill");
}

#[tokio::test]
async fn rest_call_batch_preserves_batch_shape_for_policy_denied_items() {
    let mut gs = test_gateway_state("1.2.3");
    gs.policy = Arc::new(crate::gateway::GatewayPolicy {
        read_only: true,
        allowed_dcc_types: vec!["maya".to_string()],
        allowed_skill_families: vec!["unsafe-".to_string()],
        allowed_tool_slug_prefixes: vec!["maya.abcdef01.unsafe_write".to_string()],
        ..Default::default()
    });
    let (_, _, maya_write, _) = seed_policy_records(&gs).await;

    let (status, body) = response_json(
        handle_v1_call_batch(
            State(gs),
            HeaderMap::new(),
            Json(json!({
                "calls": [{
                    "id": "write-1",
                    "tool_slug": maya_write,
                    "arguments": {}
                }],
                "stop_on_error": true
            })),
        )
        .await,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], false);
    assert_eq!(body["results"][0]["id"], "write-1");
    assert_eq!(body["results"][0]["ok"], false);
    assert_eq!(
        body["results"][0]["error"]["error"]["kind"],
        "policy-denied"
    );
    assert_eq!(
        body["results"][0]["error"]["error"]["policy"]["reason"],
        "read-only"
    );
}

#[tokio::test]
async fn mcp_search_and_call_apply_gateway_policy() {
    let mut search_gs = test_gateway_state("1.2.3");
    search_gs.policy = Arc::new(crate::gateway::GatewayPolicy {
        read_only: true,
        ..policy_for_safe_reads()
    });
    let (maya_read, custom_read, maya_write, custom_write) =
        seed_policy_unloaded_records(&search_gs);

    let (search_text, search_is_error) = crate::gateway::aggregator::route_tools_call(
        &search_gs,
        "search",
        &json!({}),
        None,
        None,
        None,
        None,
    )
    .await;
    assert!(!search_is_error);
    let search_body: Value = serde_json::from_str(&search_text).unwrap();
    let slugs: Vec<&str> = search_body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|hit| hit["tool_slug"].as_str())
        .collect();
    assert!(slugs.contains(&maya_read.as_str()));
    assert!(slugs.contains(&custom_read.as_str()));
    assert!(!slugs.contains(&maya_write.as_str()));
    assert!(!slugs.contains(&custom_write.as_str()));

    let mut call_gs = test_gateway_state("1.2.3");
    call_gs.policy = Arc::new(crate::gateway::GatewayPolicy {
        read_only: true,
        ..policy_for_safe_reads()
    });
    let (_, _, maya_write, _) = seed_policy_records(&call_gs).await;
    let (call_text, call_is_error) = crate::gateway::aggregator::route_tools_call(
        &call_gs,
        "call",
        &json!({"tool_slug": maya_write, "arguments": {}}),
        None,
        None,
        None,
        None,
    )
    .await;
    assert!(call_is_error);
    let call_body: Value = serde_json::from_str(&call_text).unwrap();
    assert_eq!(call_body["error"]["kind"], "policy-denied");
    assert_eq!(call_body["error"]["policy"]["reason"], "skill-allowlist");
}
