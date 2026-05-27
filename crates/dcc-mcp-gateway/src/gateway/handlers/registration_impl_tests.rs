use super::*;

use axum::body::{Body, to_bytes};
use axum::http::Request;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast, watch};
use tower::ServiceExt;
use uuid::Uuid;

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
            crate::gateway::mdns_discovery::MdnsInstanceRegistry::default(),
        )),
        relay_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::relay_discovery::RelayInstanceRegistry::default(),
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
        security: Arc::new(crate::gateway::GatewaySecurityPolicy::disabled()),
        adapter_version: None,
        adapter_dcc: None,
        capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
        #[cfg(feature = "prometheus")]
        gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
        instance_diagnostics: Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        search_telemetry: Arc::new(crate::gateway::search_telemetry::SearchTelemetryStore::new()),
        debug_routes_enabled: false,
    }
}

async fn request_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    request_json_with_auth(app, method, uri, body, None).await
}

async fn request_json_with_auth(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
    authorization: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(authorization) = authorization {
        builder = builder.header("authorization", authorization);
    }
    let response = app
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn register_rejects_missing_gateway_token_when_security_enabled() {
    let mut state = test_gateway_state();
    state.security = Arc::new(crate::gateway::GatewaySecurityPolicy::new(
        crate::gateway::GatewaySecurityConfig::with_api_keys(["secret-token"]),
    ));
    let app = crate::gateway::build_gateway_router(state);

    let (status, body) = request_json(
        app,
        "POST",
        "/v1/instances/register",
        json!({
            "instance_id": "44444444-4444-4444-8444-444444444444",
            "dcc_type": "maya",
            "mcp_url": "http://127.0.0.1:8765/mcp"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthorized");
    assert_eq!(body["error_detail"]["kind"], "unauthorized");
}

#[tokio::test]
async fn register_rejects_dcc_scope_mismatch() {
    let secret = "gateway-jwt-secret-for-tests";
    let claims = crate::gateway::GatewayTokenClaims::new(
        "agent-1",
        crate::gateway::security::now_secs() + 3600,
        vec!["maya".to_string()],
        vec!["register".to_string()],
    );
    let token = crate::gateway::security::issue_gateway_token(&claims, secret).unwrap();
    let mut state = test_gateway_state();
    state.security = Arc::new(crate::gateway::GatewaySecurityPolicy::new(
        crate::gateway::GatewaySecurityConfig::with_jwt_secret(secret),
    ));
    let app = crate::gateway::build_gateway_router(state);

    let (status, body) = request_json_with_auth(
        app,
        "POST",
        "/v1/instances/register",
        json!({
            "instance_id": "55555555-5555-4555-8555-555555555555",
            "dcc_type": "houdini",
            "mcp_url": "http://127.0.0.1:8765/mcp"
        }),
        Some(format!("Bearer {token}")),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "forbidden");
    assert_eq!(body["error_detail"]["kind"], "dcc-scope-denied");
    assert_eq!(body["error_detail"]["dcc_type"], "houdini");
}

#[tokio::test]
async fn register_heartbeat_and_deregister_remote_instance() {
    let state = test_gateway_state();
    let app = crate::gateway::build_gateway_router(state);
    let instance_id = "11111111-1111-4111-8111-111111111111";

    let (status, body) = request_json(
        app.clone(),
        "POST",
        "/v1/instances/register",
        json!({
            "instance_id": instance_id,
            "dcc_type": "maya",
            "mcp_url": "https://remote.example:9443/prefix/mcp",
            "capabilities_fingerprint": "fp-1",
            "adapter_version": "1.2.3",
            "scene": "shot-a.ma",
            "ttl_secs": 90
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["heartbeat_interval_secs"], 30);

    let (status, body) = request_json(app.clone(), "GET", "/v1/instances", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 1);
    assert_eq!(body["instances"][0]["instance_id"], instance_id);
    assert_eq!(body["instances"][0]["source"], "http");
    assert_eq!(
        body["instances"][0]["mcp_url"],
        "https://remote.example:9443/prefix/mcp"
    );

    let (status, _) = request_json(
        app.clone(),
        "POST",
        "/v1/instances/heartbeat",
        json!({
            "instance_id": instance_id,
            "capabilities_fingerprint": "fp-2",
            "scene": "shot-b.ma"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = request_json(app.clone(), "GET", "/v1/instances", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["instances"][0]["scene"], "shot-b.ma");
    assert_eq!(
        body["instances"][0]["metadata"]["capabilities_fingerprint"],
        "fp-2"
    );

    let (status, body) = request_json(
        app.clone(),
        "POST",
        "/v1/instances/deregister",
        json!({"instance_id": instance_id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["operation"], "deregistered");

    let (status, body) = request_json(app, "GET", "/v1/instances", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn http_registration_wins_over_file_row_for_same_instance_id() {
    let state = test_gateway_state();
    let instance_id = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    {
        let registry = state.registry.read().await;
        let mut file_entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        file_entry.instance_id = instance_id;
        registry.register(file_entry).unwrap();
    }
    {
        let mut http_registry = state.http_instance_registry.write();
        http_registry
            .register(
                HttpInstanceRegistrationRequest {
                    instance_id: instance_id.to_string(),
                    dcc_type: "maya".to_string(),
                    mcp_url: "http://remote.example:28812/mcp".to_string(),
                    capabilities_fingerprint: None,
                    adapter_version: None,
                    scene: None,
                    ttl_secs: None,
                },
                std::time::SystemTime::now(),
            )
            .unwrap();
    }

    let registry = state.registry.read().await;
    let live = state.live_instances(&registry);
    assert_eq!(live.len(), 1);
    let row = state.instance_json(&live[0]);
    assert_eq!(row["source"], "http");
    assert_eq!(row["mcp_url"], "http://remote.example:28812/mcp");
}

#[tokio::test]
async fn register_rejects_non_mcp_url() {
    let app = crate::gateway::build_gateway_router(test_gateway_state());

    let (status, body) = request_json(
        app,
        "POST",
        "/v1/instances/register",
        json!({
            "instance_id": "33333333-3333-4333-8333-333333333333",
            "dcc_type": "houdini",
            "mcp_url": "http://127.0.0.1:8765/v1/search"
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["kind"], "bad-request");
}
