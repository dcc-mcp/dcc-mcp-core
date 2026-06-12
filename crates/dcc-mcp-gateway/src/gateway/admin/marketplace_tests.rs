//! Focused tests for Admin marketplace APIs.

#![allow(clippy::await_holding_lock)] // Intentional: parking_lot Mutex serializes env-var tests.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};
use tower::ServiceExt;

use crate::gateway::admin::router::build_admin_router;
use crate::gateway::admin::state::AdminState;
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;

/// Marketplace install/uninstall tests mutate DCC_MCP_MARKETPLACE_INSTALL_ROOT.
static MARKETPLACE_ENV_LOCK: Mutex<()> = Mutex::new(());

struct ScopedMarketplaceInstallRoot {
    previous: Option<String>,
    previous_no_default_sources: Option<String>,
}

impl ScopedMarketplaceInstallRoot {
    fn new(root: &std::path::Path) -> Self {
        let previous = std::env::var("DCC_MCP_MARKETPLACE_INSTALL_ROOT").ok();
        let previous_no_default_sources =
            std::env::var("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES").ok();
        // SAFETY: tests are serialized with MARKETPLACE_ENV_LOCK.
        unsafe {
            std::env::set_var(
                "DCC_MCP_MARKETPLACE_INSTALL_ROOT",
                root.to_string_lossy().as_ref(),
            );
            std::env::set_var("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1");
        }
        Self {
            previous,
            previous_no_default_sources,
        }
    }
}

impl Drop for ScopedMarketplaceInstallRoot {
    fn drop(&mut self) {
        // SAFETY: same as `new` - guarded by the test mutex.
        unsafe {
            match &self.previous {
                Some(v) => std::env::set_var("DCC_MCP_MARKETPLACE_INSTALL_ROOT", v),
                None => std::env::remove_var("DCC_MCP_MARKETPLACE_INSTALL_ROOT"),
            }
            match &self.previous_no_default_sources {
                Some(v) => std::env::set_var("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", v),
                None => std::env::remove_var("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES"),
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
        auth: std::sync::Arc::new(crate::gateway::security::GatewayAuth::disabled()),
        update_manifest_url: None,
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
    }
}

fn make_admin_state() -> AdminState {
    AdminState::new(make_gateway_state())
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

#[tokio::test]
async fn test_marketplace_install_triggers_skill_paths_reload_hook() {
    let _env_guard = MARKETPLACE_ENV_LOCK.lock();
    let tmp = tempfile::tempdir().unwrap();
    let marketplace_root = tmp.path().join("marketplace");
    let _scoped_root = ScopedMarketplaceInstallRoot::new(&marketplace_root);

    let skill_src = tmp.path().join("skill-src");
    std::fs::create_dir_all(&skill_src).unwrap();
    std::fs::write(skill_src.join("SKILL.md"), "---\nname: test-skill\n---\n").unwrap();

    let catalog_path = tmp.path().join("catalog.json");
    let skill_src_str = skill_src.display().to_string();
    let catalog = serde_json::json!({
        "version": "1",
        "entries": [{
            "name": "test-skill",
            "description": "test",
            "dcc": ["maya"],
            "tags": [],
            "install": {
                "type": "path",
                "url": skill_src_str
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let reload_calls = Arc::new(AtomicUsize::new(0));
    let reload_calls_for_hook = reload_calls.clone();
    let state = make_admin_state().with_skill_paths_reload(Some(Arc::new(move || {
        reload_calls_for_hook.fetch_add(1, Ordering::SeqCst);
    })));
    let router = build_admin_router(state);

    let source_path = catalog_path.display().to_string();
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/marketplace/install")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "name": "test-skill",
                        "dcc": "maya",
                        "source": source_path
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(reload_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_marketplace_uninstall_triggers_skill_paths_reload_hook() {
    let _env_guard = MARKETPLACE_ENV_LOCK.lock();
    let tmp = tempfile::tempdir().unwrap();
    let marketplace_root = tmp.path().join("marketplace");
    let _scoped_root = ScopedMarketplaceInstallRoot::new(&marketplace_root);

    let dcc_root = marketplace_root.join("maya");
    let pkg_root = dcc_root.join("test-skill");
    std::fs::create_dir_all(&pkg_root).unwrap();
    std::fs::write(pkg_root.join("SKILL.md"), "---\nname: test-skill\n---\n").unwrap();
    std::fs::write(
        marketplace_root.join("installed.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "packages": [{
                "name": "test-skill",
                "dcc": "maya",
                "version": "0.1.0",
                "path": pkg_root.display().to_string(),
                "source_name": "test",
                "source_url": "file://test",
                "install_type": "path",
                "installed_at_ms": 1
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let reload_calls = Arc::new(AtomicUsize::new(0));
    let reload_calls_for_hook = reload_calls.clone();
    let state = make_admin_state().with_skill_paths_reload(Some(Arc::new(move || {
        reload_calls_for_hook.fetch_add(1, Ordering::SeqCst);
    })));
    let router = build_admin_router(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/marketplace/uninstall")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "name": "test-skill",
                        "dcc": "maya"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(reload_calls.load(Ordering::SeqCst), 1);
}

// ── PIP-699 M1: sources / outdated / update / force / error envelope ──────

#[tokio::test]
async fn test_marketplace_sources_returns_builtin_config_and_env() {
    let _env_guard = MARKETPLACE_ENV_LOCK.lock();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("marketplace");
    std::fs::create_dir_all(&root).unwrap();
    let _scoped_root = ScopedMarketplaceInstallRoot::new(&root);
    // This test specifically needs builtin sources; re-enable them.
    unsafe {
        std::env::remove_var("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES");
    }

    let state = make_admin_state();
    let router = build_admin_router(state);

    let (status, json) = body_json(router, "/api/marketplace/sources").await;
    assert_eq!(status, StatusCode::OK);
    let sources = json["sources"]
        .as_array()
        .expect("sources should be an array");
    assert!(!sources.is_empty(), "should have at least builtin source");
    // builtin source has origin "builtin"
    let builtin = sources
        .iter()
        .find(|s| s["origin"].as_str() == Some("builtin"));
    assert!(builtin.is_some(), "should include builtin source");
}

#[tokio::test]
async fn test_marketplace_add_source_persists_and_is_visible() {
    let _env_guard = MARKETPLACE_ENV_LOCK.lock();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("marketplace");
    std::fs::create_dir_all(&root).unwrap();
    let _scoped_root = ScopedMarketplaceInstallRoot::new(&root);

    let state = make_admin_state();
    let router = build_admin_router(state);

    // Add a new source
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/marketplace/sources")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&json!({"source": "studio/my-catalog"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Read back — should include the new source
    let (status, json) = body_json(router, "/api/marketplace/sources").await;
    assert_eq!(status, StatusCode::OK);
    let sources = json["sources"]
        .as_array()
        .expect("sources should be an array");
    let added = sources
        .iter()
        .find(|s| s["name"].as_str() == Some("studio/my-catalog"));
    assert!(added.is_some(), "should include the newly added source");
}

#[tokio::test]
async fn test_marketplace_add_source_duplicate_is_idempotent() {
    let _env_guard = MARKETPLACE_ENV_LOCK.lock();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("marketplace");
    std::fs::create_dir_all(&root).unwrap();
    let _scoped_root = ScopedMarketplaceInstallRoot::new(&root);

    let state = make_admin_state();
    let router = build_admin_router(state);

    let make_body = || {
        axum::body::Body::from(
            serde_json::to_vec(&json!({"source": "studio/dupe-catalog"})).unwrap(),
        )
    };

    // Add twice
    for _ in 0..2 {
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/marketplace/sources")
                    .header("content-type", "application/json")
                    .body(make_body())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Sources should not have duplicate entries
    let (status, json) = body_json(router, "/api/marketplace/sources").await;
    assert_eq!(status, StatusCode::OK);
    let sources = json["sources"].as_array().expect("sources");
    let dupes: Vec<_> = sources
        .iter()
        .filter(|s| s["name"].as_str() == Some("studio/dupe-catalog"))
        .collect();
    assert_eq!(dupes.len(), 1, "duplicate add should be a no-op");
}

#[tokio::test]
async fn test_marketplace_install_supports_force_parameter() {
    let _env_guard = MARKETPLACE_ENV_LOCK.lock();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("marketplace");
    std::fs::create_dir_all(&root).unwrap();
    let _scoped_root = ScopedMarketplaceInstallRoot::new(&root);

    // Source skill dir — must be separate from the install destination
    let skill_src = tmp.path().join("skill-src-force");
    std::fs::create_dir_all(&skill_src).unwrap();
    std::fs::write(
        skill_src.join("SKILL.md"),
        "---\nname: test-force-skill\n---\n",
    )
    .unwrap();

    let catalog_path = tmp.path().join("catalog-force.json");
    let src_str = skill_src.display().to_string();
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "test-force-skill",
            "description": "test force",
            "dcc": ["maya"],
            "tags": [],
            "install": { "type": "path", "url": src_str }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let state = make_admin_state();
    let router = build_admin_router(state);
    let source_path = catalog_path.display().to_string();

    // First install
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/marketplace/install")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&json!({
                        "name": "test-force-skill",
                        "dcc": "maya",
                        "source": source_path
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Second install without force → should fail (already installed)
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/marketplace/install")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&json!({
                        "name": "test-force-skill",
                        "dcc": "maya",
                        "source": source_path,
                        "force": false
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let err_json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err_json["error"]["kind"], "already_installed");

    // Third install with force → should succeed
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/marketplace/install")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&json!({
                        "name": "test-force-skill",
                        "dcc": "maya",
                        "source": source_path,
                        "force": true
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_marketplace_outdated_returns_empty_when_nothing_installed() {
    let _env_guard = MARKETPLACE_ENV_LOCK.lock();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("marketplace");
    std::fs::create_dir_all(&root).unwrap();
    let _scoped_root = ScopedMarketplaceInstallRoot::new(&root);

    let state = make_admin_state();
    let router = build_admin_router(state);

    let (status, json) = body_json(router, "/api/marketplace/outdated").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["count"].as_u64(), Some(0));
}

#[tokio::test]
async fn test_marketplace_error_envelope_has_kind_and_message() {
    let _env_guard = MARKETPLACE_ENV_LOCK.lock();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("marketplace");
    std::fs::create_dir_all(&root).unwrap();
    let _scoped_root = ScopedMarketplaceInstallRoot::new(&root);

    let state = make_admin_state();
    let router = build_admin_router(state);

    // Try installing a package that doesn't exist in any source
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/marketplace/install")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&json!({
                        "name": "nonexistent-pkg",
                        "dcc": "maya"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let err_json: Value = serde_json::from_slice(&bytes).unwrap();
    let error = &err_json["error"];
    assert!(
        error["kind"].is_string(),
        "error should have a 'kind' string: {err_json}"
    );
    assert!(
        error["message"].is_string(),
        "error should have a 'message' string: {err_json}"
    );
    assert_eq!(error["kind"], "not_found");
}

#[tokio::test]
async fn test_marketplace_update_triggers_skill_paths_reload_hook() {
    let _env_guard = MARKETPLACE_ENV_LOCK.lock();
    let tmp = tempfile::tempdir().unwrap();
    let marketplace_root = tmp.path().join("marketplace");
    std::fs::create_dir_all(&marketplace_root).unwrap();
    let _scoped_root = ScopedMarketplaceInstallRoot::new(&marketplace_root);

    // Skill source directory with SKILL.md
    let skill_src = tmp.path().join("skill-src-update");
    std::fs::create_dir_all(&skill_src).unwrap();
    std::fs::write(
        skill_src.join("SKILL.md"),
        "---\nname: test-update-skill\n---\n",
    )
    .unwrap();

    // Catalog with version "0.2.0"
    let catalog_path = tmp.path().join("catalog-update.json");
    let src_str = skill_src.display().to_string();
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "test-update-skill",
            "description": "test update",
            "dcc": ["maya"],
            "version": "0.2.0",
            "tags": [],
            "install": { "type": "path", "url": src_str }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let catalog_url = format!("file://{}", catalog_path.display());

    // Pre-write sources.json
    std::fs::write(
        marketplace_root.join("sources.json"),
        serde_json::to_string_pretty(&json!({
            "sources": [{
                "name": "test-source",
                "url": catalog_url,
                "origin": "explicit"
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    // Pre-write installed.json with older version
    std::fs::write(
        marketplace_root.join("installed.json"),
        serde_json::to_string_pretty(&json!({
            "packages": [{
                "name": "test-update-skill",
                "dcc": "maya",
                "version": "0.1.0",
                "path": marketplace_root.join("maya").join("test-update-skill")
                    .display().to_string(),
                "source_name": "test-source",
                "source_url": catalog_url,
                "install_type": "path",
                "install_url": null,
                "install_ref": null,
                "installed_at_ms": 1
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    // Set up reload hook
    let reload_calls = Arc::new(AtomicUsize::new(0));
    let reload_calls_for_hook = reload_calls.clone();
    let state = make_admin_state().with_skill_paths_reload(Some(Arc::new(move || {
        reload_calls_for_hook.fetch_add(1, Ordering::SeqCst);
    })));
    let router = build_admin_router(state);

    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/marketplace/update")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&json!({
                        "name": "test-update-skill",
                        "dcc": "maya"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(reload_calls.load(Ordering::SeqCst), 1);
}
