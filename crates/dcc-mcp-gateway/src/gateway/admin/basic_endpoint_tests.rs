//! Focused tests for basic Admin endpoint contracts.

#[cfg(all(test, feature = "admin"))]
mod endpoint_contracts {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::Router;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
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
            "/api/stats",
            "/api/governance",
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
