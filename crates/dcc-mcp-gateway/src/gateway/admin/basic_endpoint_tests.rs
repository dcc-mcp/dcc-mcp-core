//! Focused tests for basic Admin endpoint contracts.

#[cfg(all(test, feature = "admin"))]
mod endpoint_contracts {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::Router;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use serde_json::{Value, json};
    use tokio::sync::{RwLock, broadcast, watch};
    use tower::ServiceExt;

    use crate::gateway::admin::application::router::{build_admin_router, build_v1_debug_router};
    use crate::gateway::admin::state::AdminState;
    use crate::gateway::state::GatewayState;
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;

    fn make_gateway_state() -> GatewayState {
        let dir = tempfile::tempdir().unwrap();
        let registry = std::sync::Arc::new(FileRegistry::new(dir.path()).unwrap());
        let (yield_tx, _) = watch::channel(false);
        let (events_tx, _) = broadcast::channel::<String>(8);
        GatewayState {
            ingress: std::sync::Arc::new(
                crate::gateway::http_limits::GatewayIngressState::from_env(),
            ),
            resilience: std::sync::Arc::new(Default::default()),
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
            search_telemetry: Arc::new(
                crate::gateway::search_telemetry::SearchTelemetryStore::new(),
            ),
            debug_routes_enabled: false,
            auth: std::sync::Arc::new(crate::gateway::security::GatewayAuth::disabled()),
            update_manifest_url: None,
            gateway_persist: false,
            gateway_idle_timeout_secs: 30,
            semantic_search_enabled: false,
            #[cfg(feature = "admin-persist-sqlite")]
            admin_sqlite_lane: None,
        }
    }

    fn make_admin_state() -> AdminState {
        AdminState::new(make_gateway_state())
    }

    fn admin_router() -> Router {
        build_admin_router(make_admin_state())
    }

    fn debug_router() -> Router {
        build_v1_debug_router(make_admin_state())
    }

    fn gateway_router_with_admin(gateway: GatewayState) -> Router {
        crate::gateway::build_gateway_router_with_admin(
            gateway.clone(),
            Some(AdminState::new(gateway)),
            "/admin",
        )
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
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    async fn request_json(
        router: Router,
        method: &str,
        uri: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let resp = router
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    async fn body_html(router: Router, uri: &str) -> (StatusCode, String, String) {
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
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
        let body = String::from_utf8_lossy(&bytes).to_string();
        (status, ct, body)
    }

    #[tokio::test]
    async fn test_admin_ui_returns_html() {
        let (status, ct, _) = body_html(admin_router(), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(ct.contains("text/html"), "expected text/html, got {ct}");
    }

    #[tokio::test]
    async fn test_admin_html_has_title() {
        let (_, _, html) = body_html(admin_router(), "/").await;
        assert!(
            html.contains("<title>") && (html.contains("DCC-MCP") || html.contains("Admin")),
            "HTML missing expected <title> content"
        );
    }

    #[tokio::test]
    async fn test_admin_html_contains_api_references() {
        let (_, _, html) = body_html(admin_router(), "/").await;
        for endpoint in &["instances", "tools", "health", "traces", "stats"] {
            assert!(
                html.contains(endpoint),
                "HTML missing reference to '{endpoint}'"
            );
        }
    }

    #[tokio::test]
    async fn test_admin_html_contains_traces_and_stats_panels() {
        let (_, _, html) = body_html(admin_router(), "/").await;
        // Vite minifies JSX; assert stable API paths and panel strings from the bundle.
        for needle in [
            "/traces?limit=",
            "/stats?range=",
            "trace-row",
            "No traces recorded.",
        ] {
            assert!(html.contains(needle), "HTML missing {needle}");
        }
        assert!(
            html.contains("data-panel"),
            "HTML missing data-panel attribute hooks"
        );
    }

    #[tokio::test]
    async fn test_admin_html_is_valid_doctype() {
        let (_, _, html) = body_html(admin_router(), "/").await;
        let trimmed = html.trim_start().to_lowercase();
        assert!(
            trimmed.starts_with("<!doctype html>"),
            "HTML must start with <!DOCTYPE html>"
        );
    }

    #[tokio::test]
    async fn test_admin_instances_returns_json_array() {
        let (status, body) = body_json(admin_router(), "/api/instances").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body["instances"].is_array(),
            "expected 'instances' array, got {body}"
        );
    }

    #[tokio::test]
    async fn test_admin_instances_empty_without_dccs() {
        let (_, body) = body_json(admin_router(), "/api/instances").await;
        assert!(body["instances"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_admin_health_returns_ok() {
        let (status, body) = body_json(admin_router(), "/api/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["status"].as_str(),
            Some("ok"),
            "expected status=ok, got {body}"
        );
    }

    #[tokio::test]
    async fn test_admin_health_has_uptime_secs() {
        let (_, body) = body_json(admin_router(), "/api/health").await;
        assert!(
            body["uptime_secs"].as_u64().is_some(),
            "expected uptime_secs >= 0"
        );
    }

    #[tokio::test]
    async fn test_admin_health_instances_total_is_zero() {
        let (_, body) = body_json(admin_router(), "/api/health").await;
        assert_eq!(body["instances_total"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn test_admin_health_has_instances_ready_field() {
        let (_, body) = body_json(admin_router(), "/api/health").await;
        assert!(
            body.get("instances_ready").is_some(),
            "expected instances_ready field"
        );
    }

    #[tokio::test]
    async fn test_admin_health_includes_job_persistence_summary() {
        let (_, body) = body_json(admin_router(), "/api/health").await;
        assert_eq!(body["job_persistence"]["instances"], serde_json::json!([]));
        assert_eq!(body["job_persistence"]["degraded_instances"], 0);
        assert_eq!(body["job_persistence"]["disabled_instances"], 0);
    }

    #[tokio::test]
    async fn test_admin_and_debug_health_expose_the_same_persistence_shape() {
        let (_, admin) = body_json(admin_router(), "/api/health").await;
        let (_, debug) = body_json(debug_router(), "/v1/debug/health").await;
        assert_eq!(admin["job_persistence"], debug["job_persistence"]);
    }

    #[tokio::test]
    async fn unsafe_http_registration_is_not_reported_ready_by_admin_routes() {
        let mut gateway = make_gateway_state();
        gateway.backend_timeout = Duration::from_millis(25);
        let app = gateway_router_with_admin(gateway);

        let (register_status, _) = request_json(
            app.clone(),
            "POST",
            "/v1/instances/register",
            json!({
                "instance_id": "11111111-1111-4111-8111-111111111111",
                "dcc_type": "maya",
                "mcp_url": "http://127.0.0.1:9/mcp"
            }),
        )
        .await;
        assert_eq!(register_status, StatusCode::OK);

        let (_, health) = body_json(app.clone(), "/admin/api/health").await;
        assert_eq!(health["instances_ready"], 0, "{health}");
        assert_eq!(health["instances_total"], 1, "{health}");
        assert_eq!(health["status"], "degraded", "{health}");

        let (_, reliability) = body_json(app.clone(), "/admin/api/reliability").await;
        assert_eq!(
            reliability["capability_funnel"]["instances_ready"], 0,
            "{reliability}"
        );
        assert_eq!(
            reliability["capability_funnel"]["instances_total"], 1,
            "{reliability}"
        );
        assert_eq!(reliability["status"], "degraded", "{reliability}");

        let (dispatch_status, dispatch) = request_json(
            app,
            "POST",
            "/mcp/dcc/maya",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "health-contract-test", "version": "1.0"}
                }
            }),
        )
        .await;
        assert_eq!(dispatch_status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(dispatch["kind"], "unsafe-backend-target", "{dispatch}");
    }

    #[tokio::test]
    async fn mixed_safe_and_unsafe_instances_count_only_dispatchable_backend_as_ready() {
        let mut gateway = make_gateway_state();
        gateway.backend_timeout = Duration::from_millis(25);

        let mut safe =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("blender", "127.0.0.1", 18812);
        safe.metadata.insert(
            "mcp_url".to_string(),
            "http://127.0.0.1:18812/mcp".to_string(),
        );
        safe.metadata.insert(
            "discovery_mcp_url".to_string(),
            "http://127.0.0.1:18813/mcp".to_string(),
        );
        gateway.registry.register(safe).unwrap();

        let app = gateway_router_with_admin(gateway);
        let (register_status, _) = request_json(
            app.clone(),
            "POST",
            "/v1/instances/register",
            json!({
                "instance_id": "22222222-2222-4222-8222-222222222222",
                "dcc_type": "photoshop",
                "mcp_url": "http://127.0.0.1:9/mcp"
            }),
        )
        .await;
        assert_eq!(register_status, StatusCode::OK);

        let (_, health) = body_json(app.clone(), "/admin/api/health").await;
        assert_eq!(health["instances_ready"], 1, "{health}");
        assert_eq!(health["instances_total"], 2, "{health}");
        assert_eq!(health["status"], "ok", "{health}");

        let (_, reliability) = body_json(app, "/admin/api/reliability").await;
        assert_eq!(
            reliability["capability_funnel"]["instances_ready"], 1,
            "{reliability}"
        );
        assert_eq!(
            reliability["capability_funnel"]["instances_total"], 2,
            "{reliability}"
        );
        assert_eq!(reliability["status"], "ok", "{reliability}");
    }

    #[tokio::test]
    async fn test_health_collector_projects_multiple_backends_and_failures() {
        let healthy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let healthy_port = healthy_listener.local_addr().unwrap().port();
        let healthy_app = Router::new().route(
            "/health",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({
                    "job_persistence": {
                        "state": "healthy",
                        "consecutive_failures": 0,
                        "last_error_kind": "readonly"
                    }
                }))
            }),
        );
        let healthy_task = tokio::spawn(async move {
            axum::serve(healthy_listener, healthy_app).await.unwrap();
        });

        let malformed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let malformed_port = malformed_listener.local_addr().unwrap().port();
        let malformed_app = Router::new().route(
            "/health",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({
                    "job_persistence": {
                        "state": "not-a-state",
                        "last_error_kind": {"path": "C:/secret/jobs.sqlite3"}
                    }
                }))
            }),
        );
        let malformed_task = tokio::spawn(async move {
            axum::serve(malformed_listener, malformed_app)
                .await
                .unwrap();
        });

        let timeout_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let timeout_port = timeout_listener.local_addr().unwrap().port();
        let timeout_task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = timeout_listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    drop(socket);
                });
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let mut gateway = make_gateway_state();
        gateway.registry = Arc::new(FileRegistry::new(dir.path()).unwrap());
        gateway.backend_timeout = Duration::from_millis(25);
        gateway
            .registry
            .register(dcc_mcp_transport::discovery::types::ServiceEntry::new(
                "maya",
                "127.0.0.1",
                healthy_port,
            ))
            .unwrap();
        gateway
            .registry
            .register(dcc_mcp_transport::discovery::types::ServiceEntry::new(
                "blender",
                "127.0.0.1",
                malformed_port,
            ))
            .unwrap();
        gateway
            .registry
            .register(dcc_mcp_transport::discovery::types::ServiceEntry::new(
                "photoshop",
                "127.0.0.1",
                timeout_port,
            ))
            .unwrap();

        let (_, body) = body_json(
            build_admin_router(AdminState::new(gateway.clone())),
            "/api/health",
        )
        .await;
        let instances = body["job_persistence"]["instances"].as_array().unwrap();
        assert_eq!(instances.len(), 3, "{body}");
        let maya = instances.iter().find(|v| v["dcc_type"] == "maya").unwrap();
        assert_eq!(maya["state"], "healthy");
        assert_eq!(maya["last_error_kind"], "readonly");
        let blender = instances
            .iter()
            .find(|v| v["dcc_type"] == "blender")
            .unwrap();
        assert_eq!(blender["state"], "unavailable");
        assert_eq!(blender["last_error_kind"], "backend");
        let photoshop = instances
            .iter()
            .find(|v| v["dcc_type"] == "photoshop")
            .unwrap();
        assert_eq!(photoshop["state"], "unavailable");
        assert!(photoshop["last_error_kind"].is_null());

        healthy_task.abort();
        malformed_task.abort();
        timeout_task.abort();
    }

    #[tokio::test]
    async fn test_admin_health_includes_limits_and_circuits() {
        let (_, body) = body_json(admin_router(), "/api/health").await;
        assert!(body.get("limits").is_some(), "expected limits object");
        assert!(body.get("circuits").is_some(), "expected circuits object");
        assert!(body.get("rss_bytes").is_some(), "expected rss_bytes field");
        assert_eq!(body["response_format"]["default"], "toon");
        assert_eq!(
            body["response_format"]["token_estimator"],
            "dcc-mcp-byte4-v1"
        );
    }

    #[tokio::test]
    async fn test_admin_tools_returns_json_array() {
        let (status, body) = body_json(admin_router(), "/api/tools").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body["tools"].is_array(),
            "expected 'tools' array, got {body}"
        );
    }

    #[tokio::test]
    async fn test_admin_tools_empty_without_dccs() {
        let (_, body) = body_json(admin_router(), "/api/tools").await;
        assert!(body["tools"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_admin_unknown_path_returns_404() {
        let resp = admin_router()
            .oneshot(
                Request::builder()
                    .uri("/api/doesnotexist")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_json_endpoints_content_type() {
        for uri in [
            "/api/instances",
            "/api/health",
            "/api/tools",
            "/api/skills",
            "/api/calls",
            "/api/logs",
            "/api/feedback",
            "/api/stats",
            "/api/governance",
            "/api/artifacts",
            "/api/traces",
            "/api/workflows",
        ] {
            let resp = admin_router()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let ct = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(
                ct.contains("application/json"),
                "endpoint {uri} must return application/json, got '{ct}'"
            );
        }
    }
}
