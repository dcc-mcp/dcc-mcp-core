//! Focused tests for Admin skill path APIs.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use tokio::sync::{RwLock, broadcast, watch};
use tower::ServiceExt;

use crate::gateway::admin::router::build_admin_router;
use crate::gateway::admin::state::AdminState;
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;

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
    }
}

fn make_admin_state() -> AdminState {
    AdminState::new(make_gateway_state())
}

fn admin_router() -> Router {
    build_admin_router(make_admin_state())
}

async fn body_json(router: Router, uri: &str) -> (StatusCode, Value) {
    let resp = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn post_skill_path(router: Router, payload: Value) -> StatusCode {
    router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/skill-paths")
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn test_admin_skill_paths_returns_empty_snapshot() {
    let (status, body) = body_json(admin_router(), "/api/skill-paths").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["paths"].is_array());
    assert!(body["paths"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_admin_skill_paths_shows_snapshot_entries() {
    use crate::gateway::SkillPathEntry;

    let state = make_admin_state().with_skill_paths_snapshot(vec![
        SkillPathEntry {
            path: "/opt/skills/maya".into(),
            source: "cli".into(),
        },
        SkillPathEntry {
            path: "/opt/skills/blender".into(),
            source: "env:DCC_MCP_SKILL_PATHS".into(),
        },
    ]);
    let router = build_admin_router(state);
    let (status, body) = body_json(router, "/api/skill-paths").await;

    assert_eq!(status, StatusCode::OK);
    let paths = body["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0]["path"], "Cli · skills/maya");
    assert_eq!(paths[0]["display_path"], "Cli · skills/maya");
    assert_eq!(paths[0]["source_label"], "Cli");
    assert_eq!(paths[0]["path_tail"], "skills/maya");
    assert_eq!(paths[0]["path_redacted"], true);
    assert_ne!(paths[0]["path"], "/opt/skills/maya");
    assert_eq!(paths[0]["source"], "cli");
    assert_eq!(paths[0]["status"], "missing");
    assert_eq!(paths[1]["path"], "Env var · skills/blender");
    assert_eq!(paths[1]["display_path"], "Env var · skills/blender");
    assert_eq!(paths[1]["source_label"], "Env var");
    assert_eq!(paths[1]["path_tail"], "skills/blender");
    assert_eq!(paths[1]["path_redacted"], true);
    assert_eq!(paths[1]["source"], "env:DCC_MCP_SKILL_PATHS");
    assert!(
        !body.to_string().contains("/opt/skills"),
        "skill path payload should be safe to attach to public reports"
    );
}

#[cfg(feature = "admin-persist-sqlite")]
#[tokio::test]
async fn test_admin_skill_path_crud_via_api() {
    use crate::gateway::admin::sqlite_lane::AdminSqliteLane;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test_crud.sqlite");
    let lane = AdminSqliteLane::spawn(db_path, 30).expect("spawn lane");

    let reload_calls = Arc::new(AtomicUsize::new(0));
    let reload_calls_for_hook = reload_calls.clone();
    let state = make_admin_state()
        .with_admin_sqlite_lane(Some(lane))
        .with_skill_paths_reload(Some(Arc::new(move || {
            reload_calls_for_hook.fetch_add(1, Ordering::SeqCst);
        })));
    let router = build_admin_router(state);

    let status = post_skill_path(
        router.clone(),
        serde_json::json!({"path": "/tmp/new-skills"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reload_calls.load(Ordering::SeqCst), 1);

    let (status, body) = body_json(router.clone(), "/api/skill-paths").await;
    assert_eq!(status, StatusCode::OK);
    let paths = body["paths"].as_array().unwrap();
    let custom: Vec<_> = paths
        .iter()
        .filter(|p| p["source"] == "admin_custom")
        .collect();
    assert_eq!(custom.len(), 1, "expected 1 custom path, got {paths:?}");
    assert_eq!(custom[0]["path"], "Admin custom · tmp/new-skills");
    assert_eq!(custom[0]["source_label"], "Admin custom");
    assert_eq!(custom[0]["path_redacted"], true);
    assert!(
        !body.to_string().contains("/tmp/new-skills"),
        "custom path payload should not expose absolute local paths"
    );

    let id = custom[0]["id"]
        .as_i64()
        .expect("custom path should have id");
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/skill-paths/{id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(reload_calls.load(Ordering::SeqCst), 2);

    let (status, body) = body_json(router, "/api/skill-paths").await;
    assert_eq!(status, StatusCode::OK);
    let paths = body["paths"].as_array().unwrap();
    let custom: Vec<_> = paths
        .iter()
        .filter(|p| p["source"] == "admin_custom")
        .collect();
    assert!(
        custom.is_empty(),
        "expected no custom paths after delete, got {custom:?}"
    );
}

#[tokio::test]
async fn test_admin_skill_path_post_empty_returns_400() {
    let status = post_skill_path(admin_router(), serde_json::json!({"path": ""})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_admin_skill_path_post_without_lane_returns_503() {
    let status = post_skill_path(admin_router(), serde_json::json!({"path": "/valid/path"})).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
