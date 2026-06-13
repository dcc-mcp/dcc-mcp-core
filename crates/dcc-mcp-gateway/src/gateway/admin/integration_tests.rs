//! Focused tests for the Admin integrations API.

#![allow(clippy::await_holding_lock)] // Intentional: parking_lot Mutex serializes env-var tests.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, oneshot, watch};
use tower::ServiceExt;

use crate::gateway::admin::integrations::INTEGRATIONS_TEST_ENV_LOCK;
use crate::gateway::admin::router::{build_admin_router, build_v1_debug_router};
use crate::gateway::admin::state::AdminState;
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;

struct ScopedIntegrationEnv {
    previous: Vec<(&'static str, Option<String>)>,
}

impl ScopedIntegrationEnv {
    fn new(values: &[(&'static str, Option<&str>)]) -> Self {
        const KEYS: &[&str] = &[
            "DCC_MCP_SENTRY_DSN",
            "DCC_MCP_SENTRY_ENVIRONMENT",
            "DCC_MCP_SENTRY_RELEASE",
            "DCC_MCP_SENTRY_SAMPLE_RATE",
            "DCC_MCP_ETC_DIR",
            "DCC_MCP_WEBHOOKS_CONFIG",
            "DCC_MCP_WECOM_WEBHOOK_URL",
            "DCC_MCP_WECOM_EVENTS",
            "DCC_MCP_WECOM_TEMPLATE",
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "OTEL_SERVICE_NAME",
            "OTEL_EXPORTER_OTLP_HEADERS",
        ];
        let previous = KEYS
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect::<Vec<_>>();
        // SAFETY: tests using these vars are serialized by INTEGRATIONS_TEST_ENV_LOCK.
        unsafe {
            for key in KEYS {
                std::env::remove_var(key);
            }
            for (key, value) in values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
        Self { previous }
    }
}

impl Drop for ScopedIntegrationEnv {
    fn drop(&mut self) {
        // SAFETY: same as `new` - guarded by the test mutex.
        unsafe {
            for (key, value) in &self.previous {
                match value {
                    Some(value) => std::env::set_var(key, value),
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
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn put_json(router: Router, uri: &str, payload: Value) -> (StatusCode, Value) {
    let resp = router
        .oneshot(
            Request::builder()
                .method("PUT")
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

async fn spawn_wecom_robot(
    reply: Value,
) -> (
    String,
    Arc<parking_lot::Mutex<Option<Value>>>,
    oneshot::Sender<()>,
) {
    let received = Arc::new(parking_lot::Mutex::new(None));
    let received_for_route = Arc::clone(&received);
    let app = Router::new().route(
        "/cgi-bin/webhook/send",
        axum::routing::post(move |axum::Json(payload): axum::Json<Value>| {
            let received = Arc::clone(&received_for_route);
            let reply = reply.clone();
            async move {
                *received.lock() = Some(payload);
                axum::Json(reply)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    (
        format!("http://127.0.0.1:{port}/cgi-bin/webhook/send?key=abc123"),
        received,
        tx,
    )
}

// ── Integrations API ─────────────────────────────────────────────────

#[tokio::test]
async fn integrations_endpoint_reports_real_env_and_config_state() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let webhooks_path = dir.path().join("webhooks.yaml");
    let etc_dir = dir.path().join("etc");
    std::fs::write(
        &webhooks_path,
        r#"
webhooks:
  - name: audit
url: https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123
authorization: Bearer webhook-token
events: ["tool.*"]
"#,
    )
    .unwrap();
    let webhooks_path_s = webhooks_path.to_string_lossy().to_string();
    let etc_dir_s = etc_dir.to_string_lossy().to_string();
    let write_path = etc_dir.join("webhooks.yaml");
    let write_path_s = write_path.to_string_lossy().to_string();
    let _env = ScopedIntegrationEnv::new(&[
        (
            "DCC_MCP_SENTRY_DSN",
            Some("https://abc123@sentry.example/42"),
        ),
        ("DCC_MCP_SENTRY_ENVIRONMENT", Some("ci")),
        ("DCC_MCP_ETC_DIR", Some(&etc_dir_s)),
        ("DCC_MCP_WEBHOOKS_CONFIG", Some(&webhooks_path_s)),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", Some("http://127.0.0.1:4317")),
        (
            "OTEL_EXPORTER_OTLP_HEADERS",
            Some("authorization=Bearer otlp-token,x-api-key=collector-key"),
        ),
    ]);

    let (status, json) = body_json(admin_router(), "/api/integrations").await;
    assert_eq!(status, StatusCode::OK);
    let integrations = json["integrations"].as_array().unwrap();
    assert_eq!(integrations.len(), 4);

    let sentry = integrations
        .iter()
        .find(|entry| entry["kind"] == "sentry")
        .unwrap();
    assert_eq!(sentry["status"], "active");
    assert_eq!(sentry["config"]["environment"], "ci");
    assert!(
        sentry["config"]["dsn"]
            .as_str()
            .unwrap()
            .contains("********")
    );
    assert!(
        sentry["env_locked_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["key"] == "dsn" && field["locked"] == true)
    );

    let webhooks = integrations
        .iter()
        .find(|entry| entry["kind"] == "webhooks")
        .unwrap();
    assert_eq!(webhooks["status"], "active");
    assert_eq!(webhooks["config"]["config_path"], webhooks_path_s);
    assert_eq!(webhooks["config"]["write_config_path"], write_path_s);
    assert_eq!(webhooks["config"]["webhook_count"], 1);
    let response_text = serde_json::to_string(&json).unwrap();
    assert!(!response_text.contains("abc123"));
    assert!(!response_text.contains("webhook-token"));
    assert!(!response_text.contains("otlp-token"));
    assert!(!response_text.contains("collector-key"));
    assert!(
        webhooks["config"]["config_text"]
            .as_str()
            .unwrap()
            .contains("key=********")
    );

    let otlp = integrations
        .iter()
        .find(|entry| entry["kind"] == "otlp")
        .unwrap();
    assert_eq!(otlp["status"], "active");
    assert_eq!(otlp["config"]["endpoint"], "http://127.0.0.1:4317");
    assert_eq!(
        otlp["config"]["headers"],
        "authorization=********,x-api-key=********"
    );

    let wecom = integrations
        .iter()
        .find(|entry| entry["kind"] == "wecom")
        .unwrap();
    assert_eq!(wecom["status"], "inactive");
}

#[tokio::test]
async fn v1_debug_integrations_mirrors_admin_integrations() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let _env = ScopedIntegrationEnv::new(&[
        (
            "DCC_MCP_WECOM_WEBHOOK_URL",
            Some("https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123"),
        ),
        (
            "OTEL_EXPORTER_OTLP_HEADERS",
            Some("authorization=Bearer debug-token"),
        ),
    ]);

    let (status, json) = body_json(
        build_v1_debug_router(AdminState::new(make_gateway_state())),
        "/v1/debug/integrations",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let integrations = json["integrations"].as_array().unwrap();
    assert_eq!(integrations.len(), 4);
    let wecom = integrations
        .iter()
        .find(|entry| entry["kind"] == "wecom")
        .unwrap();
    assert_eq!(wecom["status"], "active");
    assert_eq!(
        wecom["config"]["webhook_url"],
        "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=********"
    );
    let response_text = serde_json::to_string(&json).unwrap();
    assert!(!response_text.contains("abc123"));
    assert!(!response_text.contains("debug-token"));
}

#[tokio::test]
async fn integrations_put_stages_pending_restart_config() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let _env = ScopedIntegrationEnv::new(&[]);
    let router = admin_router();

    let (status, updated) = put_json(
        router.clone(),
        "/api/integrations",
        json!({
            "kind": "otlp",
            "config": {
                "endpoint": "http://collector.local:4317",
                "service_name": "dcc-mcp-gateway"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["kind"], "otlp");
    assert_eq!(updated["status"], "pending_restart");
    assert_eq!(updated["config"]["endpoint"], "http://collector.local:4317");

    let (status, json) = body_json(router, "/api/integrations").await;
    assert_eq!(status, StatusCode::OK);
    let otlp = json["integrations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "otlp")
        .unwrap();
    assert_eq!(otlp["status"], "pending_restart");
    assert_eq!(otlp["config"]["service_name"], "dcc-mcp-gateway");
}

#[tokio::test]
async fn integrations_put_overlays_pending_config_on_env_backed_integration() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let _env = ScopedIntegrationEnv::new(&[
        (
            "DCC_MCP_SENTRY_DSN",
            Some("https://abc123@sentry.example/42"),
        ),
        ("DCC_MCP_SENTRY_ENVIRONMENT", Some("ci")),
    ]);
    let router = admin_router();

    let (status, updated) = put_json(
        router.clone(),
        "/api/integrations",
        json!({
            "kind": "sentry",
            "config": {
                "environment": "staging"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["kind"], "sentry");
    assert_eq!(updated["status"], "pending_restart");
    assert_eq!(updated["config"]["environment"], "staging");
    assert!(
        updated["config"]["dsn"]
            .as_str()
            .unwrap()
            .contains("********")
    );

    let (status, json) = body_json(router, "/api/integrations").await;
    assert_eq!(status, StatusCode::OK);
    let sentry = json["integrations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "sentry")
        .unwrap();
    assert_eq!(sentry["status"], "pending_restart");
    assert_eq!(sentry["config"]["environment"], "staging");
    assert!(
        sentry["env_locked_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["key"] == "dsn" && field["locked"] == true)
    );
}

#[tokio::test]
async fn integrations_put_stages_wecom_message_push_config() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let etc_dir = dir.path().to_string_lossy().to_string();
    let _env = ScopedIntegrationEnv::new(&[("DCC_MCP_ETC_DIR", Some(&etc_dir))]);
    let router = admin_router();

    let (status, updated) = put_json(
        router.clone(),
        "/api/integrations",
        json!({
            "kind": "wecom",
            "config": {
                "webhook_url": "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123",
                "event_types": "tool.failed, gateway.instance.*",
                "template": "DCC-MCP $event\nDCC: $dcc-type\nURL: $url"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["kind"], "wecom");
    assert_eq!(updated["status"], "pending_restart");
    assert_eq!(
        updated["config"]["webhook_url"],
        "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=********"
    );
    assert_eq!(updated["config"]["event_types"][0], "tool.failed");
    assert_eq!(updated["config"]["event_types"][1], "gateway.instance.*");
    assert_eq!(
        updated["config"]["template"],
        "DCC-MCP $event\nDCC: $dcc-type\nURL: $url"
    );
    let saved = std::fs::read_to_string(dir.path().join("webhooks.yaml")).unwrap();
    assert!(saved.contains("wecom-message-push"));
    assert!(saved.contains("https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123"));
    assert!(saved.contains("gateway.instance.*"));

    let (status, invalid) = put_json(
        router,
        "/api/integrations",
        json!({
            "kind": "wecom",
            "config": {
                "webhook_url": "not-a-url"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid["error"], "invalid_integration_config");
}

#[tokio::test]
async fn integrations_test_sends_wecom_message_and_masks_secret() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let _env = ScopedIntegrationEnv::new(&[]);
    let (webhook_url, received, shutdown) = spawn_wecom_robot(json!({
        "errcode": 0,
        "errmsg": "ok"
    }))
    .await;
    let router = admin_router();

    let (status, result) = post_json(
        router,
        "/api/integrations/test",
        json!({
            "kind": "wecom",
            "config": {
                "webhook_url": webhook_url,
                "event_types": ["tool.failed"],
                "template": "DCC-MCP $event"
            }
        }),
    )
    .await;

    let _ = shutdown.send(());
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["kind"], "wecom");
    assert_eq!(result["status"], "sent");
    assert_eq!(result["message"], "ok");
    assert_eq!(result["wecom"]["errcode"], 0);
    assert!(
        result["webhook_url"]
            .as_str()
            .unwrap()
            .contains("key=********")
    );
    assert!(!serde_json::to_string(&result).unwrap().contains("abc123"));

    let sent = received
        .lock()
        .clone()
        .expect("WeCom robot received payload");
    assert_eq!(sent["msgtype"], "text");
    let content = sent["text"]["content"].as_str().unwrap();
    assert!(content.contains("DCC-MCP Admin WeCom test"));
    assert!(content.contains("Gateway: test-gateway"));
}

#[tokio::test]
async fn integrations_test_rejects_missing_wecom_url() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let _env = ScopedIntegrationEnv::new(&[]);
    let (status, result) = post_json(
        admin_router(),
        "/api/integrations/test",
        json!({
            "kind": "wecom",
            "config": {}
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(result["error"], "invalid_integration_test_config");
    assert_eq!(
        result["message"],
        "wecom webhook_url is required before sending a test message"
    );
}

#[tokio::test]
async fn integrations_test_rejects_non_wecom_webhook_url() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let _env = ScopedIntegrationEnv::new(&[]);
    let (status, result) = post_json(
        admin_router(),
        "/api/integrations/test",
        json!({
            "kind": "wecom",
            "config": {
                "webhook_url": "https://example.com/cgi-bin/webhook/send?key=abc123"
            }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(result["error"], "invalid_integration_test_config");
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("valid WeCom robot webhook URL")
    );
}

#[tokio::test]
async fn integrations_test_reports_wecom_rejection_without_extra_response_fields() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let _env = ScopedIntegrationEnv::new(&[]);
    let (webhook_url, _received, shutdown) = spawn_wecom_robot(json!({
        "errcode": 93000,
        "errmsg": "invalid webhook",
        "secret_echo": "should-not-return"
    }))
    .await;

    let (status, result) = post_json(
        admin_router(),
        "/api/integrations/test",
        json!({
            "kind": "wecom",
            "config": {
                "webhook_url": webhook_url
            }
        }),
    )
    .await;

    let _ = shutdown.send(());
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(result["error"], "wecom_test_failed");
    assert_eq!(result["wecom"]["errcode"], 93000);
    assert_eq!(result["wecom"]["errmsg"], "invalid webhook");
    assert!(result["wecom"].get("secret_echo").is_none());
    assert!(!serde_json::to_string(&result).unwrap().contains("abc123"));
}

#[tokio::test]
async fn integrations_put_persists_webhooks_yaml_to_local_etc() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let etc_dir = dir.path().to_string_lossy().to_string();
    let _env = ScopedIntegrationEnv::new(&[("DCC_MCP_ETC_DIR", Some(&etc_dir))]);
    let router = admin_router();

    let config_text = r#"
webhooks:
  - name: notify
url: https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=notify-secret
authorization: Bearer saved-token
events: ["tool.failed"]
"#;
    let (status, updated) = put_json(
        router.clone(),
        "/api/integrations",
        json!({
            "kind": "webhooks",
            "config": {
                "config_text": config_text
            }
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["kind"], "webhooks");
    assert_eq!(updated["status"], "pending_restart");
    let saved_path = dir.path().join("webhooks.yaml");
    assert_eq!(
        updated["config"]["config_path"].as_str(),
        Some(saved_path.to_string_lossy().as_ref())
    );
    assert_eq!(updated["config"]["webhook_count"], 1);
    assert_eq!(
        std::fs::read_to_string(&saved_path).unwrap(),
        format!("{}\n", config_text.trim())
    );

    let (status, json) = body_json(router, "/api/integrations").await;
    assert_eq!(status, StatusCode::OK);
    let webhooks = json["integrations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "webhooks")
        .unwrap();
    assert_eq!(webhooks["status"], "pending_restart");
    assert_eq!(webhooks["config"]["webhook_count"], 1);
    assert!(
        webhooks["config"]["config_text"]
            .as_str()
            .unwrap()
            .contains("name: notify")
    );
    let response_text = serde_json::to_string(&json).unwrap();
    assert!(!response_text.contains("notify-secret"));
    assert!(!response_text.contains("saved-token"));
    assert!(
        webhooks["config"]["config_text"]
            .as_str()
            .unwrap()
            .contains("key=********")
    );
}

#[tokio::test]
async fn integrations_put_webhooks_prefers_local_etc_over_runtime_env_path() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let etc_dir = dir.path().join("etc");
    let runtime_path = dir.path().join("runtime").join("webhooks.yaml");
    let etc_dir_s = etc_dir.to_string_lossy().to_string();
    let runtime_path_s = runtime_path.to_string_lossy().to_string();
    let _env = ScopedIntegrationEnv::new(&[
        ("DCC_MCP_ETC_DIR", Some(&etc_dir_s)),
        ("DCC_MCP_WEBHOOKS_CONFIG", Some(&runtime_path_s)),
    ]);
    let router = admin_router();

    let config_text = r#"
webhooks:
  - name: local-notify
url: http://127.0.0.1:9000/hook
events: ["tool.failed"]
"#;
    let (status, updated) = put_json(
        router,
        "/api/integrations",
        json!({
            "kind": "webhooks",
            "config": {
                "config_text": config_text
            }
        }),
    )
    .await;

    let saved_path = etc_dir.join("webhooks.yaml");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        updated["config"]["config_path"].as_str(),
        Some(saved_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        updated["config"]["write_config_path"].as_str(),
        Some(saved_path.to_string_lossy().as_ref())
    );
    assert!(saved_path.exists());
    assert!(!runtime_path.exists());
}

#[tokio::test]
async fn integrations_put_persists_sentry_and_otlp_json_to_local_etc() {
    let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
    let dir = tempfile::tempdir().unwrap();
    let etc_dir = dir.path().to_string_lossy().to_string();
    let _env = ScopedIntegrationEnv::new(&[("DCC_MCP_ETC_DIR", Some(&etc_dir))]);
    let router = admin_router();

    let (status, sentry) = put_json(
        router.clone(),
        "/api/integrations",
        json!({
            "kind": "sentry",
            "config": {
                "dsn": "https://abc123@sentry.example/42",
                "environment": "studio",
                "sample_rate": 0.5
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sentry["status"], "pending_restart");
    assert_eq!(
        sentry["config"]["dsn"],
        "https://********@sentry.example/42"
    );
    let sentry_path = dir.path().join("sentry.json");
    let sentry_file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&sentry_path).unwrap()).unwrap();
    assert_eq!(sentry_file["dsn"], "https://abc123@sentry.example/42");
    assert_eq!(sentry_file["environment"], "studio");

    let (status, otlp) = put_json(
        router,
        "/api/integrations",
        json!({
            "kind": "otlp",
            "config": {
                "endpoint": "http://collector.local:4317",
                "service_name": "dcc-mcp-gateway",
                "headers": "authorization=Bearer token"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(otlp["status"], "pending_restart");
    assert_eq!(otlp["config"]["endpoint"], "http://collector.local:4317");
    assert_eq!(otlp["config"]["headers"], "authorization=********");
    let otlp_path = dir.path().join("otlp.json");
    let otlp_file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&otlp_path).unwrap()).unwrap();
    assert_eq!(otlp_file["service_name"], "dcc-mcp-gateway");
    assert_eq!(otlp_file["headers"], "authorization=Bearer token");
}
