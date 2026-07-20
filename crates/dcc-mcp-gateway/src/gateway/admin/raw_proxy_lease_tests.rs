use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, oneshot, watch};
use tower::ServiceExt;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};

use crate::gateway::router::build_gateway_router_with_admin;
use crate::gateway::state::GatewayState;

fn make_gateway_state() -> GatewayState {
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
        server_version: "0.0.0-test".into(),
        own_host: "127.0.0.1".into(),
        own_port: 9765,
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        client_attribution: Arc::new(
            crate::gateway::caller_attribution::ClientAttributionStore::default(),
        ),
        pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
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
        #[cfg(feature = "prometheus")]
        gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
        instance_diagnostics: Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        search_telemetry: Arc::new(crate::gateway::search_telemetry::SearchTelemetryStore::new()),
        debug_routes_enabled: false,
        auth: Arc::new(crate::gateway::security::GatewayAuth::disabled()),
        update_manifest_url: None,
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
        semantic_search_enabled: false,
        #[cfg(feature = "admin-persist-sqlite")]
        admin_sqlite_lane: None,
    }
}

fn make_service_entry(port: u16) -> ServiceEntry {
    let now = SystemTime::now();
    ServiceEntry {
        dcc_type: "maya".into(),
        instance_id: uuid::Uuid::new_v4(),
        host: "127.0.0.1".into(),
        port,
        transport_address: None,
        version: Some("2024.0".into()),
        adapter_version: Some("0.3.0".into()),
        adapter_dcc: Some("maya".into()),
        scene: None,
        documents: vec![],
        pid: None,
        sentinel_path: None,
        display_name: Some("maya-test".into()),
        status: ServiceStatus::Available,
        registered_at: now,
        last_heartbeat: now,
        metadata: Default::default(),
        extras: Default::default(),
        capacity: 1,
        lease_owner: None,
        current_job_id: None,
        lease_expires_at: None,
    }
}

async fn post_json(router: Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn spawn_sidecar_backend() -> (u16, oneshot::Sender<()>, Arc<Mutex<Vec<Value>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let route_calls = calls.clone();
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { StatusCode::OK }))
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(request): axum::Json<Value>| {
                let calls = route_calls.clone();
                async move {
                    calls.lock().push(request.clone());
                    let id = request.get("id").cloned().unwrap_or(json!("test"));
                    axum::Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {"content": [], "isError": false}
                    }))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await;
    });
    (port, stop_tx, calls)
}

#[tokio::test]
async fn raw_mcp_proxy_enforces_active_lease_owner_before_dispatch() {
    let state = make_gateway_state();
    let (sidecar_port, stop_sidecar, sidecar_calls) = spawn_sidecar_backend().await;
    let mut entry = make_service_entry(sidecar_port);
    entry.metadata.insert(
        crate::gateway::http_registration::MCP_URL_METADATA_KEY.to_string(),
        format!("http://127.0.0.1:{sidecar_port}/mcp"),
    );
    entry.acquire_lease(
        "workflow-a",
        Some("job-a".to_string()),
        Some(SystemTime::now() + Duration::from_secs(60)),
    );
    let instance_id = entry.instance_id;
    state.registry.write().await.register(entry).unwrap();
    let router = build_gateway_router_with_admin(state, None, "/admin");
    let exact_uri = format!("/mcp/{instance_id}");

    let (status, body) = post_json(
        router.clone(),
        &exact_uri,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "maya_scene__open_scene", "arguments": {}}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"]["data"]["kind"], "instance-leased");
    assert!(sidecar_calls.lock().is_empty());

    let (status, body) = post_json(
        router.clone(),
        "/mcp/dcc/maya",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "maya_scene__open_scene",
                "arguments": {},
                "_meta": {"lease_owner": "workflow-b"}
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"]["data"]["kind"], "lease-owner-mismatch");
    assert!(sidecar_calls.lock().is_empty());

    let (status, body) = post_json(
        router.clone(),
        &exact_uri,
        json!([
            {"jsonrpc": "2.0", "id": 20, "method": "tools/list"},
            {
                "jsonrpc": "2.0",
                "id": 21,
                "method": "tools/call",
                "params": {"name": "maya_scene__open_scene", "arguments": {}}
            },
            {
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {"name": "maya_scene__open_scene", "arguments": {}}
            }
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().map(Vec::len), Some(2));
    assert_eq!(body[0]["error"]["data"]["kind"], "batch-rejected");
    assert_eq!(body[1]["error"]["data"]["kind"], "instance-leased");
    assert!(sidecar_calls.lock().is_empty());

    let (status, body) = post_json(
        router.clone(),
        &exact_uri,
        json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": "maya_scene__open_scene", "arguments": {}}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body, Value::Null);
    assert!(sidecar_calls.lock().is_empty());

    for (id, uri) in [(3, exact_uri.as_str()), (4, "/mcp/dcc/maya")] {
        let (status, body) = post_json(
            router.clone(),
            uri,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": "maya_scene__open_scene",
                    "arguments": {},
                    "_meta": {"lease_owner": "workflow-a"}
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["isError"], false);
    }

    let (status, body) = post_json(
        router,
        &exact_uri,
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/list"}),
    )
    .await;
    let _ = stop_sidecar.send(());
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("result").is_some());
    assert_eq!(sidecar_calls.lock().len(), 3);
}
