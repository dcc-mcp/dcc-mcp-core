//! Focused tests for Admin instance update APIs.

#![allow(clippy::await_holding_lock)] // Intentional: parking_lot Mutex serializes env-var tests.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode, header};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, oneshot, watch};
use tower::ServiceExt;

use crate::gateway::admin::router::build_admin_router;
use crate::gateway::admin::state::AdminState;
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;

/// Update staging tests mutate platform data-dir env vars.
static UPDATE_ENV_LOCK: Mutex<()> = Mutex::new(());

struct ScopedUpdateDataDir {
    previous: Vec<(&'static str, Option<String>)>,
    dir: tempfile::TempDir,
}

impl ScopedUpdateDataDir {
    fn new() -> Self {
        const KEYS: &[&str] = &["APPDATA", "XDG_DATA_HOME", "HOME"];
        let previous = KEYS
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect::<Vec<_>>();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_string_lossy().to_string();
        // SAFETY: tests using these vars are serialized by `UPDATE_ENV_LOCK`.
        unsafe {
            std::env::set_var("APPDATA", &p);
            std::env::set_var("XDG_DATA_HOME", &p);
            std::env::set_var("HOME", &p);
        }
        Self { previous, dir }
    }

    fn root(&self) -> &std::path::Path {
        self.dir.path()
    }

    fn pending_marker(&self, binary_name: &str) -> std::path::PathBuf {
        #[cfg(target_os = "macos")]
        {
            self.root()
                .join("Library")
                .join("Application Support")
                .join("update")
                .join(binary_name)
                .join("pending.marker")
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.root()
                .join("update")
                .join(binary_name)
                .join("pending.marker")
        }
    }
}

impl Drop for ScopedUpdateDataDir {
    fn drop(&mut self) {
        // SAFETY: same as `new` - guarded by the test mutex.
        unsafe {
            for (key, value) in &self.previous {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

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

async fn post_json(router: Router, uri: &str, payload: Value) -> (StatusCode, Value) {
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn spawn_update_manifest(manifest: Value) -> (String, oneshot::Sender<()>) {
    let app = Router::new().route(
        "/manifest.json",
        axum::routing::get(move || {
            let manifest = manifest.clone();
            async move { axum::Json(manifest) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/manifest.json",
        listener.local_addr().unwrap().port()
    );
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    (url, tx)
}

async fn spawn_update_manifest_with_binary(
    version: &'static str,
    binary_body: &'static [u8],
) -> (String, oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let download_url = format!("http://127.0.0.1:{port}/dcc-mcp-server.bin");
    let manifest = json!({
        "dcc-mcp-server": {
            "version": version,
            "url": download_url,
            "sha256": null,
            "release_notes": "Server update"
        }
    });
    let app = Router::new()
        .route(
            "/manifest.json",
            axum::routing::get(move || {
                let manifest = manifest.clone();
                async move { axum::Json(manifest) }
            }),
        )
        .route(
            "/dcc-mcp-server.bin",
            axum::routing::get(move || {
                let body = binary_body.to_vec();
                async move { ([(header::CONTENT_TYPE, "application/octet-stream")], body) }
            }),
        );
    let url = format!("http://127.0.0.1:{port}/manifest.json");
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    (url, tx)
}

async fn spawn_update_manifest_response(
    status: StatusCode,
    content_type: &'static str,
    body: &'static str,
) -> (String, oneshot::Sender<()>) {
    let app = Router::new().route(
        "/manifest.json",
        axum::routing::get(move || async move {
            axum::response::Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, content_type)
                .body(axum::body::Body::from(body))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/manifest.json",
        listener.local_addr().unwrap().port()
    );
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    (url, tx)
}

fn make_service_entry(
    dcc_type: &str,
    host: &str,
    port: u16,
    pid: Option<u32>,
) -> dcc_mcp_transport::discovery::types::ServiceEntry {
    use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
    use std::time::SystemTime;
    let now = SystemTime::now();
    ServiceEntry {
        dcc_type: dcc_type.into(),
        instance_id: uuid::Uuid::new_v4(),
        host: host.into(),
        port,
        transport_address: None,
        version: Some("2024.0".into()),
        adapter_version: Some("0.3.0".into()),
        adapter_dcc: Some(dcc_type.into()),
        scene: None,
        documents: vec![],
        pid,
        sentinel_path: None,
        display_name: Some(format!("{dcc_type}-test")),
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

#[tokio::test]
async fn test_admin_instance_update_reports_missing_manifest_config() {
    let gs = make_gateway_state();
    let instance_id = {
        let reg = gs.registry.write().await;
        let entry = make_service_entry("maya", "127.0.0.1", 18813, Some(4242));
        let instance_id = entry.instance_id.to_string();
        reg.register(entry).unwrap();
        instance_id
    };

    let state = AdminState::new(gs);
    let router = build_admin_router(state);
    let (status, body) = post_json(
        router,
        &format!("/api/instances/{instance_id}/update"),
        json!({ "apply": true, "current_version": "0.18.0" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(body["status"], "not_configured");
    assert_eq!(body["binary_name"], "dcc-mcp-server");
    assert_eq!(body["current_version"], "0.18.0");
    assert_eq!(body["current_version_source"], "request");
    assert_eq!(body["requires_restart"], false);
}

#[tokio::test]
async fn test_admin_instance_update_requires_binary_version_for_non_server_binary() {
    let gs = make_gateway_state();
    let instance_id = {
        let reg = gs.registry.write().await;
        let entry = make_service_entry("maya", "127.0.0.1", 18813, Some(4242));
        let instance_id = entry.instance_id.to_string();
        reg.register(entry).unwrap();
        instance_id
    };

    let state = AdminState::new(gs);
    let router = build_admin_router(state);
    let (status, body) = post_json(
        router,
        &format!("/api/instances/{instance_id}/update"),
        json!({ "binary": "dcc-mcp-cli", "apply": false }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["status"], "version_required");
    assert_eq!(body["error"], "current_version_required");
    assert_eq!(body["binary_name"], "dcc-mcp-cli");
    assert_eq!(body["update_available"], false);
}

#[tokio::test]
async fn test_admin_instance_update_checks_manifest_without_manual_cli() {
    let (manifest_url, shutdown) = spawn_update_manifest(json!({
        "dcc-mcp-server": {
            "version": env!("CARGO_PKG_VERSION"),
            "url": null,
            "sha256": null,
            "release_notes": "Already current"
        }
    }))
    .await;

    let mut gs = make_gateway_state();
    gs.update_manifest_url = Some(manifest_url);
    let instance_id = {
        let reg = gs.registry.write().await;
        let entry = make_service_entry("maya", "127.0.0.1", 18813, Some(4242));
        let instance_id = entry.instance_id.to_string();
        reg.register(entry).unwrap();
        instance_id
    };

    let state = AdminState::new(gs);
    let router = build_admin_router(state);
    let (status, body) = post_json(
        router,
        &format!("/api/instances/{instance_id}/update"),
        json!({ "apply": true, "current_version": env!("CARGO_PKG_VERSION") }),
    )
    .await;
    let _ = shutdown.send(());

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "up_to_date");
    assert_eq!(body["update_available"], false);
    assert_eq!(body["current_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["current_version_source"], "request");
    assert_eq!(body["latest_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["requires_restart"], false);
}

#[tokio::test]
async fn test_admin_instance_update_reports_missing_download_url() {
    let (manifest_url, shutdown) = spawn_update_manifest(json!({
        "dcc-mcp-server": {
            "version": "999.0.0",
            "url": null,
            "sha256": null,
            "release_notes": "Update metadata without a downloadable binary"
        }
    }))
    .await;

    let mut gs = make_gateway_state();
    gs.update_manifest_url = Some(manifest_url);
    let instance_id = {
        let reg = gs.registry.write().await;
        let entry = make_service_entry("maya", "127.0.0.1", 18813, Some(4242));
        let instance_id = entry.instance_id.to_string();
        reg.register(entry).unwrap();
        instance_id
    };

    let state = AdminState::new(gs);
    let router = build_admin_router(state);
    let (status, body) = post_json(
        router,
        &format!("/api/instances/{instance_id}/update"),
        json!({ "apply": true, "current_version": "0.18.0" }),
    )
    .await;
    let _ = shutdown.send(());

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["status"], "download_failed");
    assert_eq!(body["error"], "download_url_not_configured");
    assert_eq!(body["binary_name"], "dcc-mcp-server");
    assert_eq!(body["current_version"], "0.18.0");
    assert_eq!(body["current_version_source"], "request");
    assert_eq!(body["latest_version"], "999.0.0");
    assert_eq!(body["update_available"], true);
    assert_eq!(body["requires_restart"], false);
}

#[tokio::test]
async fn test_admin_instance_update_can_check_without_staging() {
    let (manifest_url, shutdown) =
        spawn_update_manifest_with_binary("999.0.0", b"server-binary").await;

    let mut gs = make_gateway_state();
    gs.update_manifest_url = Some(manifest_url);
    let instance_id = {
        let reg = gs.registry.write().await;
        let entry = make_service_entry("maya", "127.0.0.1", 18813, Some(4242));
        let instance_id = entry.instance_id.to_string();
        reg.register(entry).unwrap();
        instance_id
    };

    let state = AdminState::new(gs);
    let router = build_admin_router(state);
    let (status, body) = post_json(
        router,
        &format!("/api/instances/{instance_id}/update"),
        json!({ "apply": false, "current_version": "0.18.0" }),
    )
    .await;
    let _ = shutdown.send(());

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "available");
    assert_eq!(body["binary_name"], "dcc-mcp-server");
    assert_eq!(body["current_version"], "0.18.0");
    assert_eq!(body["current_version_source"], "request");
    assert_eq!(body["latest_version"], "999.0.0");
    assert_eq!(body["update_available"], true);
    assert_eq!(body["requires_restart"], false);
}

#[tokio::test]
async fn test_admin_instance_update_stages_server_binary() {
    let _guard = UPDATE_ENV_LOCK.lock();
    let data_dir = ScopedUpdateDataDir::new();
    let (manifest_url, shutdown) =
        spawn_update_manifest_with_binary("999.0.0", b"server-binary").await;

    let mut gs = make_gateway_state();
    gs.update_manifest_url = Some(manifest_url);
    let instance_id = {
        let reg = gs.registry.write().await;
        let entry = make_service_entry("maya", "127.0.0.1", 18813, Some(4242));
        let instance_id = entry.instance_id.to_string();
        reg.register(entry).unwrap();
        instance_id
    };

    let state = AdminState::new(gs);
    let router = build_admin_router(state);
    let (status, body) = post_json(
        router,
        &format!("/api/instances/{instance_id}/update"),
        json!({ "apply": true, "current_version": "0.18.0" }),
    )
    .await;
    let _ = shutdown.send(());

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "staged");
    assert_eq!(body["binary_name"], "dcc-mcp-server");
    assert_eq!(body["current_version"], "0.18.0");
    assert_eq!(body["current_version_source"], "request");
    assert_eq!(body["latest_version"], "999.0.0");
    assert_eq!(body["update_available"], true);
    assert_eq!(body["requires_restart"], true);
    assert!(
        data_dir.pending_marker("dcc-mcp-server").exists(),
        "staging should write the pending update marker"
    );
}

#[tokio::test]
async fn test_admin_instance_update_reports_manifest_http_error() {
    let (manifest_url, shutdown) = spawn_update_manifest_response(
        StatusCode::NOT_FOUND,
        "text/html",
        "<!doctype html><title>missing</title>",
    )
    .await;

    let mut gs = make_gateway_state();
    gs.update_manifest_url = Some(manifest_url);
    let instance_id = {
        let reg = gs.registry.write().await;
        let entry = make_service_entry("maya", "127.0.0.1", 18813, Some(4242));
        let instance_id = entry.instance_id.to_string();
        reg.register(entry).unwrap();
        instance_id
    };

    let state = AdminState::new(gs);
    let router = build_admin_router(state);
    let (status, body) = post_json(
        router,
        &format!("/api/instances/{instance_id}/update"),
        json!({ "apply": true, "current_version": "0.18.0" }),
    )
    .await;
    let _ = shutdown.send(());

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["status"], "manifest_error");
    assert_eq!(body["binary_name"], "dcc-mcp-server");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|detail| detail.contains("404"))
    );
    assert_eq!(body["requires_restart"], false);
}
