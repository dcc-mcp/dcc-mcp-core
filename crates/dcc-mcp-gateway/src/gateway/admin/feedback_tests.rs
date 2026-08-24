//! Focused tests for the persisted feedback aggregation API.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::{RwLock, broadcast, watch};
use tower::ServiceExt;

use crate::gateway::admin::application::router::build_admin_router;
use crate::gateway::admin::state::AdminState;
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;

fn make_gateway_state(registry_dir: &std::path::Path) -> GatewayState {
    let registry = Arc::new(FileRegistry::new(registry_dir).unwrap());
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(8);
    GatewayState {
        ingress: Arc::new(crate::gateway::http_limits::GatewayIngressState::from_env()),
        resilience: Arc::new(Default::default()),
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
        event_log: Arc::new(Default::default()),
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

fn admin_router(registry_dir: &std::path::Path) -> Router {
    build_admin_router(AdminState::new(make_gateway_state(registry_dir)))
}

async fn body_json(router: Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

fn feedback_dir(registry: &TempDir) -> std::path::PathBuf {
    let path = registry.path().join("feedback");
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[tokio::test]
async fn feedback_endpoint_aggregates_rotated_files_filters_and_limits_newest_first() {
    let registry = tempfile::tempdir().unwrap();
    let feedback = feedback_dir(&registry);
    let now = now_secs();
    std::fs::write(
        feedback.join("maya-101.jsonl.1"),
        format!(
            "{}\nnot-json\n",
            json!({
                "id": "maya-old",
                "timestamp": now - 60.0,
                "tool_name": "maya.mesh.inspect",
                "intent": "Inspect the mesh",
                "blocker": "No UV set",
                "severity": "blocked",
                "dcc_type": "maya"
            })
        ),
    )
    .unwrap();
    std::fs::write(
        feedback.join("maya-101.jsonl"),
        format!(
            "{}\n{}\n{}\n",
            json!({
                "id": "maya-new",
                "timestamp": now - 5.0,
                "tool_name": "maya.render.start",
                "intent": "Render a frame",
                "blocker": "Renderer unavailable",
                "severity": "blocked",
                "dcc_type": "maya"
            }),
            json!({
                "id": "maya-suggestion",
                "timestamp": now - 1.0,
                "tool_name": "maya.render.start",
                "intent": "Render a frame",
                "blocker": "Expose samples",
                "severity": "suggestion",
                "dcc_type": "maya"
            }),
            json!({
                "id": "maya-old",
                "timestamp": now - 60.0,
                "tool_name": "maya.mesh.inspect",
                "intent": "Inspect the mesh",
                "blocker": "No UV set",
                "severity": "blocked",
                "dcc_type": "maya"
            })
        ),
    )
    .unwrap();
    std::fs::write(
        feedback.join("blender-202.jsonl"),
        format!(
            "{}\n",
            json!({
                "id": "blender-new",
                "timestamp": now,
                "tool_name": "blender.render.start",
                "intent": "Render a frame",
                "blocker": "Renderer unavailable",
                "severity": "blocked",
                "dcc_type": "blender"
            })
        ),
    )
    .unwrap();

    let (status, body) = body_json(
        admin_router(registry.path()),
        "/api/feedback?dcc=maya&severity=blocked&range=24h&limit=1",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);
    assert_eq!(body["source"], "registry-jsonl");
    assert_eq!(body["total"], 2);
    assert_eq!(body["count"], 1);
    assert_eq!(body["truncated"], true);
    assert_eq!(body["skipped_invalid"], 1);
    assert_eq!(body["deduplicated"], 1);
    assert_eq!(body["files_scanned"], 3);
    assert_eq!(body["filters"]["dcc"], "maya");
    assert_eq!(body["filters"]["severity"], "blocked");
    assert_eq!(body["filters"]["range"], "24h");
    assert_eq!(body["filters"]["limit"], 1);
    assert_eq!(body["entries"][0]["id"], "maya-new");
}

#[tokio::test]
async fn feedback_endpoint_skips_oversized_lines_without_hiding_valid_records() {
    let registry = tempfile::tempdir().unwrap();
    let feedback = feedback_dir(&registry);
    let valid = json!({
        "id": "valid",
        "timestamp": now_secs(),
        "tool_name": "houdini.scene.inspect",
        "intent": "Inspect scene",
        "blocker": "No output node",
        "severity": "workaround_found"
    });
    let contents = format!(
        "{{\"oversized\":\"{}\"}}\n{valid}\n",
        "x".repeat(1024 * 1024)
    );
    std::fs::write(feedback.join("houdini-303.jsonl"), contents).unwrap();

    let (status, body) = body_json(
        admin_router(registry.path()),
        "/api/feedback?range=7d&dcc=houdini&limit=100",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 1);
    assert_eq!(body["skipped_invalid"], 1);
    assert_eq!(body["entries"][0]["id"], "valid");
    assert_eq!(body["entries"][0]["dcc_type"], "houdini");
}

#[tokio::test]
async fn feedback_endpoint_rejects_unbounded_or_unknown_queries() {
    let registry = tempfile::tempdir().unwrap();
    for uri in [
        "/api/feedback?range=30d",
        "/api/feedback?limit=1001",
        "/api/feedback?severity=critical",
    ] {
        let (status, body) = body_json(admin_router(registry.path()), uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "uri={uri} body={body}");
        assert_eq!(body["success"], false);
        assert_eq!(body["error"]["kind"], "invalid-feedback-query");
    }
}
