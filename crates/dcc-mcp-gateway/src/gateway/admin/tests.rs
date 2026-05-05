//! Tests for the admin UI handlers.

#[cfg(all(test, feature = "admin"))]
mod admin_tests {
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

    fn make_admin_state() -> AdminState {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
        let (yield_tx, _) = watch::channel(false);
        let (events_tx, _) = broadcast::channel::<String>(8);
        let gw = GatewayState {
            registry,
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
            pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
            subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
            allow_unknown_tools: false,
            adapter_version: None,
            adapter_dcc: None,
            cursor_safe_tool_names: true,
            capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        };
        AdminState::new(gw)
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
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    /// `GET /` must return 200 with Content-Type text/html.
    #[tokio::test]
    async fn test_admin_ui_returns_html() {
        let router = admin_router();
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("text/html"), "expected text/html, got {ct}");
    }

    /// `GET /api/instances` must return 200 with a JSON array field.
    #[tokio::test]
    async fn test_admin_instances_returns_json_array() {
        let (status, body) = body_json(admin_router(), "/api/instances").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.get("instances").map(|v| v.is_array()).unwrap_or(false),
            "expected 'instances' array, got {body}"
        );
    }

    /// `GET /api/health` must return 200 with `{"status": "ok", ...}`.
    #[tokio::test]
    async fn test_admin_health_returns_ok() {
        let (status, body) = body_json(admin_router(), "/api/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body.get("status").and_then(Value::as_str),
            Some("ok"),
            "expected status=ok, got {body}"
        );
        assert!(
            body.get("uptime_secs").is_some(),
            "expected uptime_secs field, got {body}"
        );
    }

    /// `GET /api/tools` must return 200 with a `tools` array.
    #[tokio::test]
    async fn test_admin_tools_returns_json_array() {
        let (status, body) = body_json(admin_router(), "/api/tools").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.get("tools").map(|v| v.is_array()).unwrap_or(false),
            "expected 'tools' array, got {body}"
        );
    }

    /// `GET /api/calls` must return 200 with a `calls` array (empty when no AuditLog).
    #[tokio::test]
    async fn test_admin_calls_returns_empty_without_audit_log() {
        let (status, body) = body_json(admin_router(), "/api/calls").await;
        assert_eq!(status, StatusCode::OK);
        let calls = body.get("calls").and_then(Value::as_array);
        assert!(calls.is_some(), "expected 'calls' field, got {body}");
        assert!(
            calls.unwrap().is_empty(),
            "expected empty calls without AuditLog"
        );
    }

    /// `GET /api/logs` must return 200 with a `logs` array.
    #[tokio::test]
    async fn test_admin_logs_returns_json_array() {
        let (status, body) = body_json(admin_router(), "/api/logs").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.get("logs").map(|v| v.is_array()).unwrap_or(false),
            "expected 'logs' array, got {body}"
        );
    }
}
