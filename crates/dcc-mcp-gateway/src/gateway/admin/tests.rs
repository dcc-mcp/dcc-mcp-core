//! Tests for the admin UI handlers.

#[cfg(all(test, feature = "admin"))]
#[allow(clippy::await_holding_lock)] // Intentional: parking_lot Mutex for env-var test serialization
mod admin_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use axum::Router;
    use axum::body::to_bytes;
    use axum::http::{HeaderMap, Request, StatusCode, header};
    use parking_lot::Mutex;
    use serde_json::{Value, json};
    use tokio::sync::{RwLock, broadcast, oneshot, watch};
    use tower::ServiceExt;

    use crate::gateway::admin::integrations::INTEGRATIONS_TEST_ENV_LOCK;
    use dcc_mcp_gateway_core::naming::instance_short;

    use crate::gateway::admin::router::{build_admin_router, build_v1_debug_router};
    use crate::gateway::admin::state::{AdminAuditRecord, AdminState, AuditLog};
    use crate::gateway::admin::trace::{AgentContextTrust, TokenTelemetry};
    use crate::gateway::router::build_gateway_router_with_admin;
    use crate::gateway::state::GatewayState;
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;

    /// `handle_admin_logs` merges `DCC_MCP_LOG_DIR` (or the platform default). Parallel
    /// tests and developer machines with real log files make counts flaky unless we
    /// point at a non-existent directory for the duration of the request.
    static API_LOGS_ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Update staging tests mutate platform data-dir env vars.
    static UPDATE_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct ScopedNoDiskLogsDir {
        previous: Option<String>,
    }

    struct ScopedDiskLogsDir {
        previous: Option<String>,
        dir: tempfile::TempDir,
    }

    impl ScopedNoDiskLogsDir {
        fn new() -> Self {
            let previous = std::env::var("DCC_MCP_LOG_DIR").ok();
            let d = tempfile::tempdir().unwrap();
            let p = d.path().to_string_lossy().to_string();
            drop(d);
            // SAFETY: tests are serialized with `API_LOGS_ENV_LOCK`; no concurrent reads
            // of this env var in other threads during the critical section.
            unsafe {
                std::env::set_var("DCC_MCP_LOG_DIR", &p);
            }
            Self { previous }
        }
    }

    impl ScopedDiskLogsDir {
        fn new() -> Self {
            let previous = std::env::var("DCC_MCP_LOG_DIR").ok();
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().to_string_lossy().to_string();
            // SAFETY: tests are serialized with `API_LOGS_ENV_LOCK`; no concurrent reads
            // of this env var in other threads during the critical section.
            unsafe {
                std::env::set_var("DCC_MCP_LOG_DIR", &p);
            }
            Self { previous, dir }
        }

        fn path(&self) -> &std::path::Path {
            self.dir.path()
        }
    }

    impl Drop for ScopedNoDiskLogsDir {
        fn drop(&mut self) {
            // SAFETY: same as `new` — guarded by the test mutex.
            unsafe {
                match &self.previous {
                    Some(v) => std::env::set_var("DCC_MCP_LOG_DIR", v),
                    None => std::env::remove_var("DCC_MCP_LOG_DIR"),
                }
            }
        }
    }

    impl Drop for ScopedDiskLogsDir {
        fn drop(&mut self) {
            // SAFETY: same as `new` — guarded by the test mutex.
            unsafe {
                match &self.previous {
                    Some(v) => std::env::set_var("DCC_MCP_LOG_DIR", v),
                    None => std::env::remove_var("DCC_MCP_LOG_DIR"),
                }
            }
        }
    }

    struct ScopedIntegrationEnv {
        previous: Vec<(&'static str, Option<String>)>,
    }

    struct ScopedUpdateDataDir {
        previous: Vec<(&'static str, Option<String>)>,
        _dir: tempfile::TempDir,
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
            // SAFETY: tests using these vars are serialized by `INTEGRATIONS_TEST_ENV_LOCK`.
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
            // SAFETY: same as `new` — guarded by the test mutex.
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
            Self {
                previous,
                _dir: dir,
            }
        }

        fn root(&self) -> &std::path::Path {
            self._dir.path()
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
            // SAFETY: same as `new` — guarded by the test mutex.
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

    async fn body_text(router: Router, uri: &str) -> (StatusCode, String) {
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
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn body_text_with_accept(
        router: Router,
        uri: &str,
        accept: &str,
    ) -> (StatusCode, HeaderMap, String) {
        let resp = router
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::ACCEPT, accept)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        (status, headers, String::from_utf8(bytes.to_vec()).unwrap())
    }

    fn audit_record(
        request_id: &str,
        action: &str,
        success: bool,
        error: Option<&str>,
    ) -> AdminAuditRecord {
        AdminAuditRecord {
            timestamp: std::time::UNIX_EPOCH + Duration::from_millis(1),
            request_id: request_id.to_string(),
            trace_id: Some("trace-governance".to_string()),
            span_id: None,
            parent_span_id: None,
            method: Some("tools/call".to_string()),
            instance_id: Some("abcdef01-2345-6789-abcd-ef0123456789".to_string()),
            session_id: Some("session-governance".to_string()),
            transport: Some("rest".to_string()),
            agent_id: Some("agent-governance".to_string()),
            agent_name: Some("Governance Agent".to_string()),
            agent_model: Some("gpt-test".to_string()),
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            attribution_trust: None,
            parent_request_id: None,
            action: action.to_string(),
            dcc_type: Some("maya".to_string()),
            success,
            error: error.map(str::to_string),
            duration_ms: Some(12),
            token_accounting: None,
            llm_usage: None,
        }
    }

    fn token_telemetry(format: &str, original: usize, returned: usize) -> TokenTelemetry {
        let saved = original.saturating_sub(returned);
        TokenTelemetry {
            response_format: format.to_string(),
            token_estimator: "dcc-mcp-byte4-v1".to_string(),
            original_bytes: original * 4,
            returned_bytes: returned * 4,
            original_tokens: original,
            returned_tokens: returned,
            saved_tokens: saved,
            savings_pct: if original == 0 {
                0.0
            } else {
                (((saved as f64 / original as f64) * 100.0) * 100.0).round() / 100.0
            },
        }
    }

    fn governance_capture() -> crate::gateway::traffic::TrafficCapture {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let config_path = std::env::temp_dir().join(format!("dcc-mcp-governance-{suffix}.yaml"));
        let capture_path = std::env::temp_dir().join(format!("dcc-mcp-governance-{suffix}.jsonl"));
        let capture_path = capture_path.to_string_lossy().replace('\\', "/");
        std::fs::write(
            &config_path,
            format!(
                r#"
enabled: true
sinks:
  - kind: jsonl
    path: '{}'
redact:
  - body.data.params.arguments.api_key: "[REDACTED]"
"#,
                capture_path
            ),
        )
        .unwrap();
        crate::gateway::traffic::TrafficCapture::from_config_path(config_path).unwrap()
    }

    fn admin_live_capture() -> crate::gateway::traffic::TrafficCapture {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let config_path = std::env::temp_dir().join(format!("dcc-mcp-admin-live-{suffix}.yaml"));
        std::fs::write(
            &config_path,
            r#"
enabled: true
sinks:
  - kind: admin_live
    ring_buffer: 2
"#,
        )
        .unwrap();
        crate::gateway::traffic::TrafficCapture::from_config_path(config_path).unwrap()
    }

    fn filtered_admin_live_capture() -> crate::gateway::traffic::TrafficCapture {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let config_path =
            std::env::temp_dir().join(format!("dcc-mcp-admin-live-filtered-{suffix}.yaml"));
        std::fs::write(
            &config_path,
            r#"
enabled: true
sinks:
  - kind: admin_live
    ring_buffer: 2
filters:
  exclude:
    - mcp.method: tools/call
"#,
        )
        .unwrap();
        crate::gateway::traffic::TrafficCapture::from_config_path(config_path).unwrap()
    }

    fn traffic_frame(
        method: &'static str,
        request_id: &str,
    ) -> crate::gateway::traffic::TrafficFrame {
        crate::gateway::traffic::TrafficFrame::json(
            crate::gateway::traffic::basic_gateway_source(),
            crate::gateway::traffic::correlation(
                Some(request_id),
                Some("trace-traffic"),
                Some("session-traffic"),
            ),
            "inbound",
            "client_to_gateway",
            "mcp-http",
            json!({
                "jsonrpc": "2.0",
                "method": method,
                "id": request_id,
            }),
        )
        .with_session_id(Some("session-traffic"))
        .with_http(crate::gateway::traffic::http_post("/mcp", None, Some(200)))
        .with_mcp(crate::gateway::traffic::mcp_message(
            "request",
            method,
            Some(json!(request_id)),
        ))
    }

    async fn spawn_search_backend(hits: Value) -> (u16, oneshot::Sender<()>) {
        let app = Router::new().route(
            "/v1/search",
            axum::routing::post(move || {
                let hits = hits.clone();
                async move { axum::Json(json!({ "hits": hits })) }
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
        (port, tx)
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

    async fn spawn_skill_detail_backend(hits: Value, detail: Value) -> (u16, oneshot::Sender<()>) {
        let app = Router::new()
            .route("/health", axum::routing::get(|| async { StatusCode::OK }))
            .route(
                "/v1/search",
                axum::routing::post(move || {
                    let hits = hits.clone();
                    async move { axum::Json(json!({ "hits": hits })) }
                }),
            )
            .route(
                "/mcp",
                axum::routing::post(move |axum::Json(req): axum::Json<Value>| {
                    let detail = detail.clone();
                    async move {
                        let id = req.get("id").cloned().unwrap_or(json!("test"));
                        let tool_name = req
                            .pointer("/params/name")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if tool_name == "get_skill_info" {
                            let text = serde_json::to_string_pretty(&detail).unwrap();
                            axum::Json(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{ "type": "text", "text": text }],
                                    "isError": false
                                }
                            }))
                        } else {
                            axum::Json(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": { "code": -32601, "message": "unknown tool" }
                            }))
                        }
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
        (port, tx)
    }

    async fn response_status(router: Router, uri: &str) -> StatusCode {
        router
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
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

    // ── HTML dashboard ────────────────────────────────────────────────────

    #[tokio::test]
    async fn gateway_router_without_admin_state_omits_debug_routes_from_openapi() {
        let router = build_gateway_router_with_admin(make_gateway_state(), None, "/admin");

        let (status, doc) = body_json(router.clone(), "/v1/openapi.json").await;
        assert_eq!(status, StatusCode::OK);
        assert!(doc["paths"].get("/v1/search").is_some());
        assert!(doc["paths"].get("/v1/debug/instances").is_none());
        assert_eq!(
            response_status(router, "/v1/debug/instances").await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn gateway_router_with_admin_state_lists_debug_routes_in_openapi() {
        let state = make_gateway_state();
        let router =
            build_gateway_router_with_admin(state.clone(), Some(AdminState::new(state)), "/admin");

        let (status, doc) = body_json(router, "/v1/openapi.json").await;
        assert_eq!(status, StatusCode::OK);
        assert!(doc["paths"].get("/v1/debug/instances").is_some());
        assert!(doc["paths"].get("/v1/debug/traffic").is_some());
        assert!(doc["paths"].get("/v1/debug/traffic/export").is_some());
        assert!(doc["paths"].get("/v1/debug/workflows").is_some());
        assert!(doc["paths"].get("/v1/debug/analytics/overview").is_some());
        assert!(doc["paths"].get("/v1/debug/analytics/timeseries").is_some());
        assert!(doc["paths"].get("/v1/debug/analytics/heatmap").is_some());
        assert!(doc["paths"].get("/v1/debug/analytics/export").is_some());
        assert!(doc["paths"].get("/v1/debug/deregistered").is_some());
        assert!(doc["paths"].get("/v1/debug/integrations").is_some());
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

    // ── /api/instances ────────────────────────────────────────────────────

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

    // ── /api/health ───────────────────────────────────────────────────────

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

    // ── /api/tools ────────────────────────────────────────────────────────

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
    async fn test_admin_skills_refreshes_live_backend_when_index_empty() {
        let gs = make_gateway_state();
        let (port, stop) = spawn_search_backend(json!([
            {
                "skill": "maya-modeling",
                "action": "maya-modeling__create_cube",
                "summary": "Create a cube",
                "loaded": true,
                "has_schema": false
            },
            {
                "skill": "maya-modeling",
                "action": "maya-modeling__delete_cube",
                "summary": "Delete a cube",
                "loaded": true,
                "has_schema": false
            }
        ]))
        .await;
        let entry = make_service_entry("maya", "127.0.0.1", port, None);
        let instance_id = entry.instance_id;
        {
            let registry = gs.registry.write().await;
            registry.register(entry).unwrap();
        }
        assert!(
            gs.capability_index.snapshot().records.is_empty(),
            "endpoint test must start with an empty capability index"
        );
        let router = build_admin_router(AdminState::new(gs));

        let (status, body) = body_json(router, "/api/skills").await;
        let _ = stop.send(());
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 1);
        assert_eq!(body["loaded"], 1);
        assert_eq!(body["action_count"], 2);
        assert_eq!(body["skills"][0]["name"], "maya-modeling");
        assert_eq!(body["skills"][0]["dcc_type"], "maya");
        assert_eq!(body["skills"][0]["action_count"], 2);
        assert_eq!(
            body["skills"][0]["instances"][0],
            instance_short(&instance_id)
        );
    }

    #[tokio::test]
    async fn test_admin_skills_runs_skill_paths_reload_hook() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_hook = calls.clone();
        let state = make_admin_state().with_skill_paths_reload(Some(Arc::new(move || {
            calls_for_hook.fetch_add(1, Ordering::SeqCst);
        })));
        let router = build_admin_router(state);

        let (status, _body) = body_json(router, "/api/skills").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_admin_skills_exposes_health_and_adoption_metrics() {
        use crate::gateway::capability::tool_slug as make_tool_slug;
        use crate::gateway::search_telemetry::{
            RANKER_VERSION, SearchFollowupInput, SearchTelemetryHit, SearchTelemetryInput,
            SearchTelemetryStore,
        };

        let gs = make_gateway_state();
        let (port, stop) = spawn_search_backend(json!([
            {
                "skill": "maya-modeling",
                "action": "maya-modeling__create_sphere",
                "summary": "Create a polygon sphere",
                "loaded": true,
                "has_schema": true
            },
            {
                "skill": "maya-render",
                "action": "maya-render__render_preview",
                "summary": "Render a preview",
                "loaded": true,
                "has_schema": true
            }
        ]))
        .await;
        let entry = make_service_entry("maya", "127.0.0.1", port, None);
        let instance_id = entry.instance_id;
        {
            let registry = gs.registry.write().await;
            registry.register(entry).unwrap();
        }
        let modeling_slug = make_tool_slug("maya", &instance_id, "maya-modeling__create_sphere");
        let render_slug = make_tool_slug("maya", &instance_id, "maya-render__render_preview");

        let search_id = SearchTelemetryStore::new_search_id();
        gs.search_telemetry.record_search(SearchTelemetryInput {
            search_id: search_id.clone(),
            transport: "rest".to_string(),
            kind: "tool".to_string(),
            query: "create sphere or render preview".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            tags_any: vec![],
            instance_id: None,
            limit: Some(5),
            total: 2,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "idx-admin-skills".to_string(),
            hits: vec![
                SearchTelemetryHit {
                    tool_slug: render_slug,
                    skill_name: Some("maya-render".to_string()),
                    dcc_type: "maya".to_string(),
                    rank: 1,
                    score: 97,
                    match_reasons: vec!["tool_lexical".to_string()],
                    loaded: true,
                },
                SearchTelemetryHit {
                    tool_slug: modeling_slug.clone(),
                    skill_name: Some("maya-modeling".to_string()),
                    dcc_type: "maya".to_string(),
                    rank: 2,
                    score: 93,
                    match_reasons: vec!["skill_match".to_string()],
                    loaded: true,
                },
            ],
            trace_context: None,
            session_id: None,
            agent_context: None,
        });
        assert!(gs.search_telemetry.record_followup(SearchFollowupInput {
            search_id,
            kind: "call".to_string(),
            tool_slug: Some(modeling_slug),
            skill_name: None,
            success: false,
            trace_context: None,
        }));

        let router = build_admin_router(AdminState::new(gs));
        let (status, body) = body_json(router, "/api/skills").await;
        let _ = stop.send(());

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["health"]["searched_skills"], 2, "{body}");
        assert_eq!(body["health"]["used_skills"], 1);
        assert_eq!(body["health"]["low_adoption_skills"], 1);
        let skills = body["skills"].as_array().unwrap();
        let maya = skills
            .iter()
            .find(|s| s["name"] == "maya-modeling")
            .unwrap();
        assert_eq!(maya["adoption"]["search_hits"], 1);
        assert_eq!(maya["adoption"]["best_rank"], 2);
        assert_eq!(maya["adoption"]["call_count"], 1);
        assert_eq!(maya["adoption"]["failure_count"], 1);
        let render = skills.iter().find(|s| s["name"] == "maya-render").unwrap();
        assert_eq!(render["adoption"]["search_hits"], 1);
        assert_eq!(render["adoption"]["low_adoption"], true);
    }

    #[tokio::test]
    async fn test_admin_skill_detail_returns_backend_markdown() {
        let gs = make_gateway_state();
        let (port, stop) = spawn_skill_detail_backend(
            json!([
                {
                    "skill": "maya-modeling",
                    "action": "maya-modeling__create_cube",
                    "summary": "Create a cube",
                    "loaded": true,
                    "has_schema": false
                }
            ]),
            json!({
                "name": "maya-modeling",
                "description": "Modeling tools currently loaded by Maya.",
                "dcc": "maya",
                "skill_path": "G:/studio/skills/maya-modeling",
                "skill_md_path": "G:/studio/skills/maya-modeling/SKILL.md",
                "markdown": "---\nname: maya-modeling\n---\n# Maya Modeling\n\n- Create a cube\n",
                "tools": [{ "name": "create_cube" }],
                "state": "loaded"
            }),
        )
        .await;
        let entry = make_service_entry("maya", "127.0.0.1", port, None);
        let instance_id = entry.instance_id;
        {
            let registry = gs.registry.write().await;
            registry.register(entry).unwrap();
        }
        let router = build_admin_router(AdminState::new(gs));

        let uri = format!(
            "/api/skill-detail?name=maya-modeling&dcc_type=maya&instance_id={}",
            instance_short(&instance_id)
        );
        let (status, body) = body_json(router, &uri).await;
        let _ = stop.send(());

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["skill"]["name"], "maya-modeling");
        assert_eq!(body["skill"]["dcc_type"], "maya");
        assert_eq!(
            body["skill"]["instance_short"],
            instance_short(&instance_id)
        );
        assert!(
            body["skill"]["markdown"]
                .as_str()
                .unwrap()
                .contains("# Maya Modeling")
        );
        assert_eq!(
            body["skill"]["skill_md_path"],
            "G:/studio/skills/maya-modeling/SKILL.md"
        );
        assert_eq!(body["instances"].as_array().unwrap().len(), 1);
    }

    // ── /api/calls ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_admin_calls_empty_without_audit_log() {
        let (status, body) = body_json(admin_router(), "/api/calls").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["calls"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_admin_calls_returns_two_audit_records() {
        let audit_log: Arc<AuditLog> = Arc::new(parking_lot::Mutex::new(vec![
            AdminAuditRecord {
                timestamp: std::time::SystemTime::now(),
                request_id: "req-ok".to_string(),
                trace_id: Some("trace-calls".to_string()),
                span_id: None,
                parent_span_id: None,
                method: Some("tools/call".to_string()),
                instance_id: Some("maya-instance".to_string()),
                session_id: Some("session-1".to_string()),
                transport: Some("mcp".to_string()),
                agent_id: Some("agent-ok".to_string()),
                agent_name: Some("Operator Agent".to_string()),
                agent_model: Some("gpt-test".to_string()),
                actor_id: Some("artist-1".to_string()),
                actor_name: Some("Layout Artist".to_string()),
                actor_email_hash: Some("sha256:artist-1".to_string()),
                client_platform: Some("cursor".to_string()),
                client_os: Some("windows".to_string()),
                client_host: Some("workstation-7".to_string()),
                auth_subject: Some("user:artist-1".to_string()),
                source_ip: Some("192.0.2.44".to_string()),
                attribution_trust: Some(AgentContextTrust {
                    actor_id: Some("self_reported".to_string()),
                    actor_name: Some("self_reported".to_string()),
                    client_platform: Some("header".to_string()),
                    auth_subject: Some("auth".to_string()),
                    source_ip: Some("server_derived".to_string()),
                    ..AgentContextTrust::default()
                }),
                parent_request_id: None,
                action: "tools/call:maya__open_scene".to_string(),
                dcc_type: Some("maya".to_string()),
                success: true,
                error: None,
                duration_ms: Some(42),
                token_accounting: Some(token_telemetry("toon", 100, 40)),
                llm_usage: None,
            },
            AdminAuditRecord {
                timestamp: std::time::SystemTime::now(),
                request_id: "req-fail".to_string(),
                trace_id: Some("trace-calls".to_string()),
                span_id: None,
                parent_span_id: None,
                method: Some("tools/call".to_string()),
                instance_id: Some("blender-instance".to_string()),
                session_id: None,
                transport: None,
                agent_id: None,
                agent_name: None,
                agent_model: None,
                actor_id: None,
                actor_name: None,
                actor_email_hash: None,
                client_platform: None,
                client_os: None,
                client_host: None,
                auth_subject: None,
                source_ip: None,
                attribution_trust: None,
                parent_request_id: None,
                action: "tools/call:blender__render".to_string(),
                dcc_type: Some("blender".to_string()),
                success: false,
                error: Some("timeout".to_string()),
                duration_ms: None,
                token_accounting: None,
                llm_usage: None,
            },
        ]));
        let state = AdminState::new(make_gateway_state()).with_audit_log(audit_log);
        let router = build_admin_router(state);
        let (status, body) = body_json(router.clone(), "/api/calls").await;
        assert_eq!(status, StatusCode::OK);
        let calls = body["calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        // API may return in insertion order or reverse; verify both records present
        let successes: Vec<_> = calls
            .iter()
            .filter(|c| c["success"].as_bool() == Some(true))
            .collect();
        let failures: Vec<_> = calls
            .iter()
            .filter(|c| c["success"].as_bool() == Some(false))
            .collect();
        assert_eq!(successes.len(), 1, "expected 1 successful call");
        assert_eq!(failures.len(), 1, "expected 1 failed call");
        assert!(failures[0]["error"].is_string());
        // Verify new fields are populated
        assert_eq!(successes[0]["dcc_type"], "maya");
        assert_eq!(successes[0]["duration_ms"], 42);
        assert_eq!(successes[0]["request_id"], "req-ok");
        assert_eq!(successes[0]["response_format"], "toon");
        assert_eq!(successes[0]["saved_tokens"], 60);
        assert_eq!(
            successes[0]["token_accounting"]["token_estimator"],
            "dcc-mcp-byte4-v1"
        );
        assert_eq!(successes[0]["method"], "tools/call");
        assert_eq!(successes[0]["instance_id"], "maya-instance");
        assert_eq!(successes[0]["session_id"], "session-1");
        assert_eq!(successes[0]["transport"], "mcp");
        assert_eq!(successes[0]["agent_id"], "agent-ok");
        assert_eq!(successes[0]["agent_name"], "Operator Agent");
        assert_eq!(successes[0]["agent_model"], "gpt-test");
        assert_eq!(successes[0]["actor"], "Layout Artist");
        assert_eq!(successes[0]["actor_id"], "artist-1");
        assert_eq!(successes[0]["client_platform"], "cursor");
        assert_eq!(successes[0]["client_os"], "windows");
        assert_eq!(successes[0]["client_host"], "workstation-7");
        assert_eq!(successes[0]["auth_subject"], "user:artist-1");
        assert_eq!(successes[0]["source_ip"], "192.0.2.44");
        assert_eq!(
            successes[0]["attribution_trust"]["actor_id"],
            "self_reported"
        );
        assert_eq!(successes[0]["attribution_trust"]["auth_subject"], "auth");
        assert_eq!(
            successes[0]["attribution_trust"]["source_ip"],
            "server_derived"
        );
        assert_eq!(failures[0]["request_id"], "req-fail");
        assert_eq!(failures[0]["instance_id"], "blender-instance");

        let (limited_status, limited_body) = body_json(router, "/api/calls?limit=1").await;
        assert_eq!(limited_status, StatusCode::OK);
        assert_eq!(limited_body["calls"].as_array().unwrap().len(), 1);
        assert_eq!(limited_body["total"], 1);
    }

    #[tokio::test]
    async fn test_admin_calls_single_success_has_action_field() {
        let audit_log: Arc<AuditLog> = Arc::new(parking_lot::Mutex::new(vec![AdminAuditRecord {
            timestamp: std::time::SystemTime::now(),
            request_id: "req-photoshop".to_string(),
            trace_id: Some("trace-photoshop".to_string()),
            span_id: None,
            parent_span_id: None,
            method: Some("tools/call".to_string()),
            instance_id: None,
            session_id: None,
            transport: None,
            agent_id: None,
            agent_name: None,
            agent_model: None,
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            attribution_trust: None,
            parent_request_id: None,
            action: "tools/call:photoshop__save".to_string(),
            dcc_type: None,
            success: true,
            error: None,
            duration_ms: Some(100),
            token_accounting: None,
            llm_usage: None,
        }]));
        let state = AdminState::new(make_gateway_state()).with_audit_log(audit_log);
        let (_, body) = body_json(build_admin_router(state), "/api/calls").await;
        let calls = body["calls"].as_array().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0].get("tool").is_some(),
            "expected 'tool' field in call record"
        );
    }

    #[tokio::test]
    async fn test_admin_governance_exposes_policy_capture_redaction_and_pressure() {
        let mut gs = make_gateway_state();
        gs.policy = Arc::new(crate::gateway::GatewayPolicy {
            read_only: true,
            allowed_dcc_types: vec!["maya".to_string(), "customhost".to_string()],
            allowed_skill_families: vec!["safe-".to_string()],
            allowed_tool_slug_prefixes: vec!["maya.abcdef01.safe_read".to_string()],
            ..Default::default()
        });
        gs.middleware_chain = Arc::new(
            crate::gateway::middleware::MiddlewareChain::new()
                .with_before(Arc::new(crate::gateway::middleware::QuotaMiddleware::new(
                    1,
                )))
                .with_before(Arc::new(
                    crate::gateway::middleware::RedactionMiddleware::new(["api_key", "token"]),
                )),
        );
        gs.traffic_capture = Arc::new(governance_capture());
        gs.traffic_capture.emit_json_frame(
            crate::gateway::traffic::TrafficFrame::json(
                crate::gateway::traffic::basic_gateway_source(),
                crate::gateway::traffic::correlation(
                    Some("req-policy"),
                    Some("trace-governance"),
                    Some("session-governance"),
                ),
                "inbound",
                "client_to_gateway",
                "http",
                json!({
                    "jsonrpc": "2.0",
                    "method": "tools/call",
                    "params": {
                        "arguments": {
                            "api_key": "secret",
                            "keep": "visible"
                        }
                    }
                }),
            )
            .with_session_id(Some("session-governance"))
            .with_http(crate::gateway::traffic::http_post("/mcp", None, Some(200)))
            .with_mcp(crate::gateway::traffic::mcp_message(
                "request",
                "tools/call",
                Some(json!("req-policy")),
            )),
        );

        let audit_log: Arc<AuditLog> = Arc::new(Mutex::new(vec![
            audit_record(
                "req-policy",
                "maya.abcdef01.unsafe_write",
                false,
                Some(
                    "policy-denied: Gateway policy denied call for maya.abcdef01.unsafe_write: read-only",
                ),
            ),
            audit_record(
                "req-quota",
                "maya.abcdef01.safe_read_scene",
                false,
                Some(
                    "quota exceeded: session 'session-governance' exceeded 1 calls per 60s window",
                ),
            ),
            audit_record("req-ok", "maya.abcdef01.safe_read_scene", true, None),
        ]));
        let state = AdminState::new(gs).with_audit_log(audit_log);
        let router = build_admin_router(state.clone());

        let (status, body) = body_json(router, "/api/governance").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["schema_version"], "dcc-mcp.admin.governance.v1");
        assert_eq!(body["policy"]["read_only"], true);
        assert_eq!(body["traffic_capture"]["enabled"], true);
        assert_eq!(
            body["traffic_capture"]["redaction"]["paths"][0],
            "body.data.params.arguments.api_key"
        );
        let controls = body["middleware"]["controls"].as_array().unwrap();
        assert!(controls.iter().any(|row| row["kind"] == "quota"));
        assert!(controls.iter().any(|row| row["kind"] == "redaction"));
        assert!(
            body["recent_decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["outcome"] == "denied" && row["policy"]["reason"] == "read-only")
        );
        assert!(
            body["recent_decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["outcome"] == "throttled" && row["pressure"]["throttled"] == true)
        );
        assert!(
            body["recent_decisions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["privacy"]["redacted_paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|path| path == "body.data.params.arguments.api_key"))
        );

        let v1_router = build_v1_debug_router(state);
        let (debug_status, debug_body) = body_json(v1_router, "/v1/debug/governance").await;
        assert_eq!(debug_status, StatusCode::OK);
        assert_eq!(debug_body["stats"]["recent_policy_denied"], 1);
        assert_eq!(debug_body["stats"]["recent_throttled"], 1);
    }

    #[tokio::test]
    async fn test_admin_traffic_returns_live_frames_and_export() {
        let capture = admin_live_capture();
        capture.emit_json_frame(traffic_frame("tools/list", "req-live-1"));
        capture.emit_json_frame(traffic_frame("tools/call", "req-live-2"));
        capture.emit_json_frame(traffic_frame("resources/read", "req-live-3"));

        let mut gs = make_gateway_state();
        gs.traffic_capture = Arc::new(capture);
        let state = AdminState::new(gs);
        let router = build_admin_router(state.clone());

        let (status, body) = body_json(router.clone(), "/api/traffic?limit=10").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["schema_version"], "dcc-mcp.admin.traffic.v1");
        assert_eq!(body["total"], 2);
        assert_eq!(body["capture_status"]["state"], "captured");
        assert_eq!(body["capture_status"]["safe_to_share"], true);
        assert_eq!(body["capture_status"]["payload_policy"], "metadata-only");
        let frames = body["frames"].as_array().unwrap();
        assert_eq!(frames[0]["attributes"]["mcp"]["method"], "resources/read");
        assert_eq!(frames[0]["correlation"]["request_id"], "req-live-3");
        assert_eq!(frames[0]["attributes"]["body"]["payload_omitted"], true);
        assert!(frames[0]["attributes"]["body"].get("data").is_none());
        assert_eq!(frames[1]["attributes"]["mcp"]["method"], "tools/call");
        assert!(
            body["links"]["admin_traffic_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/admin?panel=traffic"))
        );
        assert!(
            body["links"]["traffic_export_jsonl_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/admin/api/traffic/export"))
        );

        let (export_status, export_body) =
            body_text(router.clone(), "/api/traffic/export?limit=10").await;
        assert_eq!(export_status, StatusCode::OK);
        let lines: Vec<&str> = export_body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"traffic.frame\""));
        assert!(lines[0].contains("\"resources/read\""));
        assert!(lines[0].contains("\"payload_omitted\":true"));
        assert!(!lines[0].contains("\"jsonrpc\""));
        assert!(lines[1].contains("\"tools/call\""));

        let v1_router = build_v1_debug_router(state);
        let (debug_status, debug_body) = body_json(v1_router, "/v1/debug/traffic?limit=1").await;
        assert_eq!(debug_status, StatusCode::OK);
        assert_eq!(debug_body["total"], 1);
        assert_eq!(
            debug_body["frames"][0]["attributes"]["mcp"]["method"],
            "resources/read"
        );
    }

    #[tokio::test]
    async fn test_admin_traffic_explains_disabled_capture() {
        let gs = make_gateway_state();
        let state = AdminState::new(gs);
        let router = build_admin_router(state);

        let (status, body) = body_json(router, "/api/traffic?limit=10").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 0);
        assert_eq!(body["capture_status"]["state"], "capture_disabled");
        assert_eq!(body["capture_status"]["capture_enabled"], false);
        assert_eq!(body["capture_status"]["live_sink_enabled"], false);
    }

    #[tokio::test]
    async fn test_admin_traffic_explains_missing_admin_live_sink() {
        let capture = governance_capture();
        capture.emit_json_frame(traffic_frame("tools/call", "req-jsonl-only"));

        let mut gs = make_gateway_state();
        gs.traffic_capture = Arc::new(capture);
        let state = AdminState::new(gs);
        let router = build_admin_router(state);

        let (status, body) = body_json(router, "/api/traffic?limit=10").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 0);
        assert_eq!(body["capture_status"]["state"], "capture_unavailable");
        assert_eq!(body["capture_status"]["capture_enabled"], true);
        assert_eq!(body["capture_status"]["live_sink_enabled"], false);
        assert_eq!(body["capture_status"]["captured_decision_count"], 1);
    }

    #[tokio::test]
    async fn test_admin_traffic_explains_filtered_capture() {
        let capture = filtered_admin_live_capture();
        capture.emit_json_frame(traffic_frame("tools/call", "req-filtered"));

        let mut gs = make_gateway_state();
        gs.traffic_capture = Arc::new(capture);
        let state = AdminState::new(gs);
        let router = build_admin_router(state);

        let (status, body) = body_json(router, "/api/traffic?limit=10").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 0);
        assert_eq!(body["capture_status"]["state"], "capture_filtered");
        assert_eq!(body["capture_status"]["capture_enabled"], true);
        assert_eq!(body["capture_status"]["live_sink_enabled"], true);
        assert_eq!(body["capture_status"]["skipped_decision_count"], 1);
        assert_eq!(body["capture_status"]["skip_reasons"][0], "filter");
    }

    #[tokio::test]
    async fn test_admin_traffic_reports_genuine_no_traffic() {
        let capture = admin_live_capture();
        let mut gs = make_gateway_state();
        gs.traffic_capture = Arc::new(capture);
        let state = AdminState::new(gs);
        let router = build_admin_router(state);

        let (status, body) = body_json(router, "/api/traffic?limit=10").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 0);
        assert_eq!(body["capture_status"]["state"], "no_traffic");
        assert_eq!(body["capture_status"]["capture_enabled"], true);
        assert_eq!(body["capture_status"]["live_sink_enabled"], true);
        assert_eq!(body["capture_status"]["recent_decision_count"], 0);
    }

    #[tokio::test]
    async fn test_admin_activity_merges_audit_and_trace_rows() {
        use crate::gateway::admin::trace::{DispatchTrace, TraceLog};
        use std::time::SystemTime;

        let audit_log: Arc<AuditLog> = Arc::new(parking_lot::Mutex::new(vec![AdminAuditRecord {
            timestamp: SystemTime::now(),
            request_id: "req-activity".to_string(),
            trace_id: Some("trace-activity".to_string()),
            span_id: Some("span-activity".to_string()),
            parent_span_id: None,
            method: Some("tools/call".to_string()),
            instance_id: Some("inst-1".to_string()),
            session_id: Some("session-1".to_string()),
            transport: Some("rest".to_string()),
            agent_id: Some("agent-activity".to_string()),
            agent_name: None,
            agent_model: None,
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            attribution_trust: None,
            parent_request_id: Some("parent-1".to_string()),
            action: "maya.inst.tool".to_string(),
            dcc_type: Some("maya".to_string()),
            success: true,
            error: None,
            duration_ms: Some(11),
            token_accounting: None,
            llm_usage: None,
        }]));
        let traces = Arc::new(TraceLog::new(10));
        traces.push(DispatchTrace {
            request_id: "req-activity".into(),
            trace_id: "trace-activity".into(),
            span_id: Some("span-activity".into()),
            parent_span_id: None,
            parent_request_id: Some("parent-1".into()),
            trace_flags: Some("01".into()),
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.inst.tool".into()),
            instance_id: Some("inst-1".into()),
            session_id: Some("session-1".into()),
            dcc_type: Some("maya".into()),
            transport: Some("rest".into()),
            agent_context: Some(crate::gateway::admin::trace::AgentContext {
                agent_id: Some("agent-activity".into()),
                parent_request_id: Some("parent-1".into()),
                ..Default::default()
            }),
            started_at: SystemTime::now(),
            total_ms: 11,
            ok: true,
            spans: vec![],
            input: None,
            output: None,
            token_accounting: Some(token_telemetry("toon", 100, 40)),
            llm_usage: None,
        });
        let state = AdminState::new(make_gateway_state())
            .with_audit_log(audit_log)
            .with_trace_log(traces, None);

        let (status, body) = body_json(build_admin_router(state), "/api/activity").await;

        assert_eq!(status, StatusCode::OK);
        let events = body["events"].as_array().unwrap();
        assert!(
            events.iter().any(|e| e["kind"] == "tool_call"),
            "expected audit event in activity payload"
        );
        assert!(
            events.iter().any(|e| e["kind"] == "dispatch_trace"),
            "expected trace event in activity payload"
        );
        assert_eq!(body["total"].as_u64(), Some(events.len() as u64));
    }

    #[tokio::test]
    async fn test_admin_search_telemetry_exposes_prompt_safe_stats() {
        use crate::gateway::search_telemetry::{
            RANKER_VERSION, SearchFollowupInput, SearchTelemetryHit, SearchTelemetryInput,
            SearchTelemetryStore,
        };

        let gs = make_gateway_state();
        let search_id = SearchTelemetryStore::new_search_id();
        gs.search_telemetry.record_search(SearchTelemetryInput {
            search_id: search_id.clone(),
            transport: "rest".to_string(),
            kind: "tool".to_string(),
            query: "token=abc123 render".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            tags_any: vec![],
            instance_id: None,
            limit: Some(5),
            total: 1,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "idx-admin".to_string(),
            hits: vec![SearchTelemetryHit {
                tool_slug: "maya.abcdef01.render_frame".to_string(),
                skill_name: Some("maya-render".to_string()),
                dcc_type: "maya".to_string(),
                rank: 1,
                score: 100,
                match_reasons: vec!["tool_lexical".to_string()],
                loaded: true,
            }],
            trace_context: None,
            session_id: None,
            agent_context: None,
        });
        assert!(gs.search_telemetry.record_followup(SearchFollowupInput {
            search_id,
            kind: "call".to_string(),
            tool_slug: Some("maya.abcdef01.render_frame".to_string()),
            skill_name: None,
            success: true,
            trace_context: None,
        }));

        let state = AdminState::new(gs);
        let (admin_status, admin_body) = body_json(
            build_admin_router(state.clone()),
            "/api/search-telemetry?limit=5",
        )
        .await;
        assert_eq!(admin_status, StatusCode::OK);
        assert_eq!(admin_body["stats"]["total_searches"], 1);
        assert_eq!(admin_body["stats"]["success_after_search_rate"], 1.0);
        assert_eq!(
            admin_body["recent"][0]["query_preview"],
            "[redacted] render"
        );

        let (debug_status, debug_body) = body_json(
            build_v1_debug_router(state),
            "/v1/debug/search-telemetry?limit=5",
        )
        .await;
        assert_eq!(debug_status, StatusCode::OK);
        assert_eq!(debug_body["stats"]["top1_hit_rate"], 1.0);
    }

    #[tokio::test]
    async fn test_admin_workflows_group_steps_and_quality_signals() {
        use crate::gateway::admin::trace::{AgentContext, DispatchTrace, TraceContext, TraceLog};
        use crate::gateway::search_telemetry::{
            RANKER_VERSION, SearchFollowupInput, SearchTelemetryHit, SearchTelemetryInput,
            SearchTelemetryStore,
        };
        use std::time::SystemTime;

        let gs = make_gateway_state();
        let traces = Arc::new(TraceLog::new(20));
        let trace_id = "4bf92f3577b34da6a3ce929d0e0e4736".to_string();
        let session_id = "session-agent-1".to_string();
        let search_id = SearchTelemetryStore::new_search_id();
        let search_ctx = TraceContext {
            trace_id: trace_id.clone(),
            request_id: "req-search".to_string(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
        };
        gs.search_telemetry.record_search(SearchTelemetryInput {
            search_id: search_id.clone(),
            transport: "rest".to_string(),
            kind: "tool".to_string(),
            query: "create sphere".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            tags_any: vec![],
            instance_id: Some("abcdef01-2345-6789-abcd-ef0123456789".to_string()),
            limit: Some(5),
            total: 2,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "idx-workflow".to_string(),
            hits: vec![SearchTelemetryHit {
                tool_slug: "maya.abcdef01.create_sphere".to_string(),
                skill_name: Some("maya-modeling".to_string()),
                dcc_type: "maya".to_string(),
                rank: 2,
                score: 88,
                match_reasons: vec!["skill_match".to_string(), "tool_lexical".to_string()],
                loaded: true,
            }],
            trace_context: Some(search_ctx),
            session_id: Some(session_id.clone()),
            agent_context: Some(AgentContext {
                agent_id: Some("agent-workflow".into()),
                agent_name: Some("Scene Builder".into()),
                model_provider: Some("openai".into()),
                model_version: Some("gpt-test".into()),
                reasoning_effort: Some("medium".into()),
                session_id: Some(session_id.clone()),
                turn_id: Some("turn-workflow".into()),
                user_intent_summary: Some("Create a simple sphere through MCP search.".into()),
                agent_reply_summary: Some("Selected the ranked sphere tool and called it.".into()),
                user_input_hash: Some("sha256:user".into()),
                agent_reply_hash: Some("sha256:reply".into()),
                user_input_chars: Some(96),
                agent_reply_chars: Some(128),
                tags: vec!["smoke".into()],
                metadata: json!({"workflow_id": "workflow-scene-build"}),
                ..Default::default()
            }),
        });
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert!(gs.search_telemetry.record_followup(SearchFollowupInput {
            search_id: search_id.clone(),
            kind: "describe".to_string(),
            tool_slug: Some("maya.abcdef01.create_sphere".to_string()),
            skill_name: Some("maya-modeling".to_string()),
            success: true,
            trace_context: Some(TraceContext {
                trace_id: trace_id.clone(),
                request_id: "req-describe".to_string(),
                span_id: None,
                parent_span_id: None,
                parent_request_id: Some("req-search".to_string()),
                trace_flags: None,
                trace_state: None,
            }),
        }));
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert!(gs.search_telemetry.record_followup(SearchFollowupInput {
            search_id: search_id.clone(),
            kind: "load_skill".to_string(),
            tool_slug: None,
            skill_name: Some("maya-modeling".to_string()),
            success: true,
            trace_context: Some(TraceContext {
                trace_id: trace_id.clone(),
                request_id: "req-load".to_string(),
                span_id: None,
                parent_span_id: None,
                parent_request_id: Some("req-describe".to_string()),
                trace_flags: None,
                trace_state: None,
            }),
        }));
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert!(gs.search_telemetry.record_followup(SearchFollowupInput {
            search_id: search_id.clone(),
            kind: "call".to_string(),
            tool_slug: Some("maya.abcdef01.create_sphere".to_string()),
            skill_name: Some("maya-modeling".to_string()),
            success: true,
            trace_context: Some(TraceContext {
                trace_id: trace_id.clone(),
                request_id: "req-call".to_string(),
                span_id: None,
                parent_span_id: None,
                parent_request_id: Some("req-load".to_string()),
                trace_flags: None,
                trace_state: None,
            }),
        }));
        traces.push(DispatchTrace {
            request_id: "req-call".into(),
            trace_id: trace_id.clone(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: Some("req-load".into()),
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.abcdef01.create_sphere".into()),
            instance_id: Some("abcdef01-2345-6789-abcd-ef0123456789".into()),
            session_id: Some(session_id.clone()),
            dcc_type: Some("maya".into()),
            transport: Some("rest".into()),
            agent_context: Some(AgentContext {
                agent_id: Some("agent-workflow".into()),
                agent_name: Some("Scene Builder".into()),
                model: Some("gpt-test".into()),
                task: Some("Create a simple sphere".into()),
                tags: vec!["smoke".into()],
                metadata: json!({"workflow_id": "workflow-scene-build"}),
                ..Default::default()
            }),
            started_at: SystemTime::now(),
            total_ms: 31,
            ok: true,
            spans: vec![],
            input: None,
            output: None,
            token_accounting: Some(token_telemetry("toon", 100, 40)),
            llm_usage: None,
        });

        let zero_id = SearchTelemetryStore::new_search_id();
        gs.search_telemetry.record_search(SearchTelemetryInput {
            search_id: zero_id.clone(),
            transport: "mcp".to_string(),
            kind: "tool".to_string(),
            query: "missing api".to_string(),
            dcc_type: Some("blender".to_string()),
            dcc_types: vec![],
            tags_any: vec![],
            instance_id: None,
            limit: Some(5),
            total: 0,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "idx-workflow".to_string(),
            hits: vec![],
            trace_context: None,
            session_id: None,
            agent_context: None,
        });

        let audit_log: Arc<AuditLog> = Arc::new(Mutex::new(vec![AdminAuditRecord {
            timestamp: SystemTime::now(),
            request_id: "req-audit-only".into(),
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            method: Some("tools/call".into()),
            instance_id: None,
            session_id: None,
            transport: Some("mcp".into()),
            agent_id: Some("agent-audit".into()),
            agent_name: None,
            agent_model: Some("gpt-audit".into()),
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            attribution_trust: None,
            parent_request_id: Some("req-missing-parent".into()),
            action: "photoshop.12345678.save_document".into(),
            dcc_type: Some("photoshop".into()),
            success: false,
            error: Some("document closed".into()),
            duration_ms: Some(9),
            token_accounting: None,
            llm_usage: None,
        }]));

        let state = AdminState::new(gs)
            .with_audit_log(audit_log)
            .with_trace_log(traces, None);
        let router = build_admin_router(state.clone());
        let (status, body) = body_json(router, "/api/workflows?limit=10").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"].as_u64(), Some(3));
        assert_eq!(body["summary"]["zero_result_workflows"], 1);

        let workflows = body["workflows"].as_array().unwrap();
        let session_workflow = workflows
            .iter()
            .find(|workflow| workflow["workflow_id"] == session_id)
            .expect("session workflow");
        assert_eq!(session_workflow["group_kind"], "session");
        assert_eq!(session_workflow["status"], "completed");
        assert_eq!(session_workflow["agent"]["agent_name"], "Scene Builder");
        assert_eq!(session_workflow["agent"]["model_provider"], "openai");
        assert_eq!(session_workflow["agent"]["model_version"], "gpt-test");
        assert_eq!(session_workflow["agent"]["reasoning_effort"], "medium");
        assert_eq!(session_workflow["agent"]["turn_id"], "turn-workflow");
        assert_eq!(
            session_workflow["agent"]["user_intent_summary"],
            "Create a simple sphere through MCP search."
        );
        assert_eq!(
            session_workflow["agent"]["agent_reply_summary"],
            "Selected the ranked sphere tool and called it."
        );
        assert_eq!(session_workflow["agent"]["user_input_hash"], "sha256:user");
        assert_eq!(
            session_workflow["agent"]["agent_reply_hash"],
            "sha256:reply"
        );
        assert_eq!(session_workflow["agent"]["user_input_chars"], 96);
        assert_eq!(session_workflow["agent"]["agent_reply_chars"], 128);
        assert_eq!(session_workflow["correlation"]["turn_id"], "turn-workflow");
        assert_eq!(session_workflow["discovery"]["best_selected_rank"], 2);
        assert_eq!(session_workflow["discovery"]["selected_count"], 3);
        assert!(
            session_workflow["discovery"]["time_to_first_success_ms"]
                .as_u64()
                .is_some()
        );
        let step_kinds: Vec<_> = session_workflow["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|step| step["kind"].as_str().unwrap())
            .collect();
        assert_eq!(step_kinds, vec!["search", "describe", "load_skill", "call"]);
        let call_step = session_workflow["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|step| step["kind"] == "call")
            .unwrap();
        assert_eq!(call_step["search"]["selected_rank"], 2);
        assert_eq!(call_step["search"]["selected_score"], 88);
        assert!(
            call_step["links"]["debug_bundle_url"]
                .as_str()
                .unwrap()
                .ends_with("/admin/api/debug-bundle/req-call")
        );

        let audit_workflow = workflows
            .iter()
            .find(|workflow| workflow["workflow_id"] == "req-audit-only")
            .expect("partial audit workflow");
        assert_eq!(audit_workflow["status"], "failed");
        assert_eq!(audit_workflow["agent"]["agent_id"], "agent-audit");

        let (debug_status, debug_body) =
            body_json(build_v1_debug_router(state), "/v1/debug/workflows?limit=10").await;
        assert_eq!(debug_status, StatusCode::OK);
        assert_eq!(debug_body["total"].as_u64(), Some(3));
    }

    #[tokio::test]
    async fn test_admin_tasks_and_debug_bundle_from_trace() {
        use crate::gateway::admin::trace::{AgentContext, DispatchTrace, TraceLog, TracePayload};
        use crate::gateway::event_log::{ContendEvent, EventKind};
        use std::time::SystemTime;

        let traces = Arc::new(TraceLog::new(10));
        let instance_id = "abcdef01-2345-6789-abcd-ef0123456789";
        traces.push(DispatchTrace {
            request_id: "req-prev".into(),
            trace_id: "trace-task".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.abcdef01.save_scene".into()),
            instance_id: Some(instance_id.into()),
            session_id: Some("session-1".into()),
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: Some(AgentContext {
                actor_id: Some("artist-1".into()),
                actor_name: Some("Layout Artist".into()),
                agent_id: Some("agent-1".into()),
                client_platform: Some("cursor".into()),
                client_host: Some("workstation-7".into()),
                auth_subject: Some("user:artist-1".into()),
                source_ip: Some("192.0.2.44".into()),
                ..AgentContext::default()
            }),
            started_at: SystemTime::UNIX_EPOCH + Duration::from_millis(1_000),
            total_ms: 12,
            ok: true,
            spans: vec![],
            input: Some(TracePayload::from_value(
                &json!({"file": "scene.ma", "token": "[REDACTED]"}),
                1024,
            )),
            output: None,
            token_accounting: None,
            llm_usage: None,
        });
        traces.push(DispatchTrace {
            request_id: "req-task".into(),
            trace_id: "trace-task".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: Some("req-prev".into()),
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.inst.long_task".into()),
            instance_id: Some(instance_id.into()),
            session_id: Some("session-1".into()),
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: None,
            started_at: SystemTime::UNIX_EPOCH + Duration::from_millis(2_000),
            total_ms: 25,
            ok: false,
            spans: vec![],
            input: None,
            output: None,
            token_accounting: None,
            llm_usage: None,
        });
        let gateway = make_gateway_state();
        gateway.event_log.push(ContendEvent::new(
            EventKind::HostDied,
            "maya",
            "abcdef01",
            Some("call=long_task display_id=maya@2026-abcdef01".into()),
        ));
        let audit_log: Arc<AuditLog> = Arc::new(Mutex::new(vec![AdminAuditRecord {
            timestamp: SystemTime::UNIX_EPOCH + Duration::from_millis(2_500),
            request_id: "req-task".into(),
            trace_id: Some("trace-task".into()),
            span_id: None,
            parent_span_id: None,
            method: Some("tools/call".into()),
            instance_id: Some(instance_id.into()),
            session_id: Some("session-1".into()),
            transport: Some("mcp".into()),
            agent_id: Some("agent-task".into()),
            agent_name: Some("Task Agent".into()),
            agent_model: Some("gpt-test".into()),
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            attribution_trust: None,
            parent_request_id: Some("req-prev".into()),
            action: "maya.inst.long_task".into(),
            dcc_type: Some("maya".into()),
            success: false,
            error: Some(
                "host died while opening C:\\studio\\secret\\shot.ma via http://127.0.0.1:8765/callback"
                    .into(),
            ),
            duration_ms: Some(25),
            token_accounting: Some(token_telemetry("toon", 100, 40)),
            llm_usage: None,
        }]));
        let state = AdminState::new(gateway)
            .with_audit_log(audit_log)
            .with_trace_log(traces, None);
        let router = build_admin_router(state.clone());

        let (tasks_status, tasks_body) = body_json(router.clone(), "/api/tasks").await;
        assert_eq!(tasks_status, StatusCode::OK);
        assert_eq!(tasks_body["total"].as_u64(), Some(1));
        let task = &tasks_body["tasks"][0];
        assert_eq!(task["task_id"], "session-1");
        assert_eq!(task["task_type"], "session_task");
        assert_eq!(task["status"], "failed");
        assert_eq!(task["correlation"]["request_id"], "req-task");
        assert_eq!(task["related"]["request_ids"].as_array().unwrap().len(), 2);
        assert_eq!(task["related"]["workflow_ids"][0], "session-1");
        assert_eq!(task["app_types"][0], "maya");
        assert_eq!(task["artifacts"][0]["kind"], "save");
        assert!(task["failure_reason"].as_str().is_some_and(|reason| {
            reason.contains("[path-redacted]") && reason.contains("[url-redacted]")
        }));
        assert!(
            task["links"]["primary_request"]["debug_bundle_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/admin/api/debug-bundle/req-task"))
        );
        let failure_reason = task["failure_reason"].as_str().unwrap();
        assert!(!failure_reason.contains("C:\\studio"));
        assert!(!failure_reason.contains("127.0.0.1"));

        let (bundle_status, bundle_body) =
            body_json(router.clone(), "/api/debug-bundle/req-task").await;
        assert_eq!(bundle_status, StatusCode::OK);
        assert_eq!(bundle_body["request_id"], "req-task");
        assert_eq!(bundle_body["trace_id"], "trace-task");
        assert_eq!(bundle_body["request_ids"].as_array().unwrap().len(), 2);
        assert_eq!(bundle_body["traces"].as_array().unwrap().len(), 2);
        assert!(bundle_body["trace"].is_object());
        assert!(bundle_body["related_activity"].is_array());
        assert_eq!(
            bundle_body["postmortem"]["previous_calls"][0]["request_id"],
            "req-prev"
        );
        assert!(
            bundle_body["postmortem"]["previous_calls"][0]["input"]["content"]
                .as_str()
                .is_some_and(|content| content.contains("[REDACTED]"))
        );
        assert_eq!(
            bundle_body["postmortem"]["gateway_events"][0]["status"],
            "host_died"
        );
        assert!(bundle_body.get("related_logs").is_none());
        assert!(bundle_body["hints"].is_array());
        assert!(
            bundle_body["links"]["issue_report_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/admin/api/issue-report/req-task"))
        );
        assert!(
            bundle_body["links"]["openapi_inspector_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/admin?panel=openapi"))
        );
        assert!(
            bundle_body["links"]["openapi_spec_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/v1/openapi.json"))
        );

        let v1_router = crate::gateway::admin::router::build_v1_debug_router(state);
        let (instances_status, instances_body) =
            body_json(v1_router.clone(), "/v1/debug/instances").await;
        assert_eq!(instances_status, StatusCode::OK);
        assert_eq!(instances_body["view"], "live");

        let (activity_status, activity_body) =
            body_json(v1_router.clone(), "/v1/debug/activity?limit=20").await;
        assert_eq!(activity_status, StatusCode::OK);
        assert!(activity_body["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["correlation"]["request_id"] == "req-task")
        }));

        let (traces_status, traces_body) =
            body_json(v1_router.clone(), "/v1/debug/traces?limit=20").await;
        assert_eq!(traces_status, StatusCode::OK);
        assert!(
            traces_body["traces"]
                .as_array()
                .is_some_and(|traces| traces.iter().any(|trace| trace["request_id"] == "req-task"))
        );

        let (trace_detail_status, trace_detail_body) =
            body_json(v1_router.clone(), "/v1/debug/traces/req-task").await;
        assert_eq!(trace_detail_status, StatusCode::OK);
        assert_eq!(trace_detail_body["request_id"], "req-task");
        assert_eq!(trace_detail_body["trace_id"], "trace-task");

        let (context_status, context_body) =
            body_json(v1_router.clone(), "/v1/debug/trace-context/trace-task").await;
        assert_eq!(context_status, StatusCode::OK);
        assert_eq!(context_body["request_id"], "req-task");
        assert_eq!(context_body["trace_id"], "trace-task");

        let (agent_packet_status, agent_packet_body) =
            body_json(v1_router.clone(), "/v1/debug/agent-traces/req-task").await;
        assert_eq!(agent_packet_status, StatusCode::OK);
        assert_eq!(
            agent_packet_body["schema_version"],
            "dcc-mcp.admin.agent-trace-packet.v1"
        );
        assert_eq!(agent_packet_body["lookup_id"], "req-task");
        assert_eq!(agent_packet_body["request_id"], "req-task");
        assert_eq!(agent_packet_body["trace_id"], "trace-task");
        assert_eq!(agent_packet_body["status"], "err");
        assert_eq!(agent_packet_body["postmortem"]["previous_call_count"], 1);
        assert_eq!(agent_packet_body["postmortem"]["gateway_event_count"], 1);
        assert!(
            agent_packet_body["links"]["agent_trace_packet_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/v1/debug/agent-traces/req-task"))
        );
        assert!(agent_packet_body.get("trace").is_none());
        assert!(agent_packet_body.get("traces").is_none());
        assert!(agent_packet_body.get("debug_bundle").is_none());
        let agent_packet_json = serde_json::to_string(&agent_packet_body).unwrap();
        assert!(!agent_packet_json.contains("scene.ma"));
        assert!(!agent_packet_json.contains("[REDACTED]"));

        let (agent_packet_trace_status, agent_packet_trace_body) =
            body_json(v1_router.clone(), "/v1/debug/agent-traces/trace-task").await;
        assert_eq!(agent_packet_trace_status, StatusCode::OK);
        assert_eq!(agent_packet_trace_body["lookup_id"], "trace-task");
        assert_eq!(agent_packet_trace_body["request_id"], "req-task");
        assert_eq!(agent_packet_trace_body["trace_id"], "trace-task");

        let (v1_tasks_status, v1_tasks_body) =
            body_json(v1_router.clone(), "/v1/debug/tasks?limit=20").await;
        assert_eq!(v1_tasks_status, StatusCode::OK);
        assert!(
            v1_tasks_body["tasks"]
                .as_array()
                .is_some_and(|tasks| tasks.iter().any(|task| task["task_id"] == "session-1"))
        );

        let (calls_status, calls_body) = body_json(v1_router.clone(), "/v1/debug/calls").await;
        assert_eq!(calls_status, StatusCode::OK);
        assert!(
            calls_body["calls"]
                .as_array()
                .is_some_and(|calls| calls.iter().any(|call| call["request_id"] == "req-task"))
        );

        {
            let _env = API_LOGS_ENV_LOCK.lock();
            let _no_disk = ScopedNoDiskLogsDir::new();
            let (logs_status, logs_body) = body_json(v1_router.clone(), "/v1/debug/logs").await;
            assert_eq!(logs_status, StatusCode::OK);
            assert!(
                logs_body["logs"]
                    .as_array()
                    .is_some_and(|logs| logs.iter().any(|log| log["request_id"] == "req-task"))
            );
        }

        let (stats_status, stats_body) =
            body_json(v1_router.clone(), "/v1/debug/stats?range=all").await;
        assert_eq!(stats_status, StatusCode::OK);
        assert_eq!(stats_body["range"], "all");
        assert_eq!(stats_body["total_calls"], 2);

        let (health_status, health_body) = body_json(v1_router.clone(), "/v1/debug/health").await;
        assert_eq!(health_status, StatusCode::OK);
        assert_eq!(health_body["version"], "0.0.0-test");

        let (integrations_status, integrations_body) =
            body_json(v1_router.clone(), "/v1/debug/integrations").await;
        assert_eq!(integrations_status, StatusCode::OK);
        assert_eq!(
            integrations_body["integrations"].as_array().unwrap().len(),
            4
        );

        let (v1_status, v1_body) =
            body_json(v1_router.clone(), "/v1/debug/bundles/trace-task").await;
        assert_eq!(v1_status, StatusCode::OK);
        assert_eq!(v1_body["request_id"], "req-task");
        assert_eq!(v1_body["trace_id"], "trace-task");
        assert_eq!(v1_body["request_ids"].as_array().unwrap().len(), 2);
        assert!(
            v1_body["links"]["trace_api_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/admin/api/traces/req-task"))
        );
        assert!(
            v1_body["links"]["debug_bundle_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/admin/api/debug-bundle/req-task"))
        );

        let (compact_bundle_status, compact_bundle_headers, compact_bundle_text) =
            body_text_with_accept(
                v1_router.clone(),
                "/v1/debug/bundles/trace-task",
                crate::gateway::response_codec::TOON_MIME,
            )
            .await;
        assert_eq!(compact_bundle_status, StatusCode::OK);
        assert!(
            compact_bundle_headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with(crate::gateway::response_codec::TOON_MIME))
        );
        assert_eq!(
            compact_bundle_headers
                .get(crate::gateway::response_codec::HEADER_RESPONSE_FORMAT)
                .and_then(|value| value.to_str().ok()),
            Some("toon")
        );
        assert!(
            compact_bundle_headers
                .get(crate::gateway::response_codec::HEADER_SAVED_TOKENS)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok())
                .is_some_and(|value| value > 0)
        );
        assert!(compact_bundle_text.len() < serde_json::to_string(&v1_body).unwrap().len());
        let compact_bundle: Value = toon_format::decode_default(&compact_bundle_text).unwrap();
        assert_eq!(
            compact_bundle["schema_version"],
            "dcc-mcp.admin.debug-summary.v1"
        );
        assert_eq!(compact_bundle["request_id"], "req-task");
        assert_eq!(compact_bundle["root_cause"], "host died");
        assert_eq!(
            compact_bundle["redaction"]["payload_previews_omitted"],
            true
        );
        assert!(compact_bundle.get("trace").is_none());
        assert!(!compact_bundle_text.contains("scene.ma"));
        assert!(!compact_bundle_text.contains("[REDACTED]"));

        let (compact_trace_status, compact_trace_headers, compact_trace_text) =
            body_text_with_accept(
                v1_router.clone(),
                "/v1/debug/traces/req-task?response_format=toon",
                "application/json",
            )
            .await;
        assert_eq!(compact_trace_status, StatusCode::OK);
        assert_eq!(
            compact_trace_headers
                .get(crate::gateway::response_codec::HEADER_RESPONSE_FORMAT)
                .and_then(|value| value.to_str().ok()),
            Some("toon")
        );
        let compact_trace: Value = toon_format::decode_default(&compact_trace_text).unwrap();
        assert_eq!(
            compact_trace["schema_version"],
            "dcc-mcp.admin.trace-summary.v1"
        );
        assert_eq!(compact_trace["request_id"], "req-task");

        let (v1_report_status, v1_report_body) =
            body_json(v1_router.clone(), "/v1/debug/issue-reports/req-task").await;
        assert_eq!(v1_report_status, StatusCode::OK);
        assert_eq!(v1_report_body["request_id"], "req-task");
        assert_eq!(v1_report_body["privacy_mode"], "public-safe");
        assert_eq!(
            v1_report_body["summary"]["error"]["kind"],
            "backend-unavailable"
        );
        assert!(v1_report_body.get("debug_bundle").is_none());
        let (v1_raw_report_status, v1_raw_report_body) = body_json(
            v1_router,
            "/v1/debug/issue-reports/req-task?include_raw=true",
        )
        .await;
        assert_eq!(v1_raw_report_status, StatusCode::OK);
        assert_eq!(v1_raw_report_body["privacy_mode"], "raw-local-evidence");
        assert_eq!(v1_raw_report_body["debug_bundle"]["trace_id"], "trace-task");

        let (report_status, report_body) =
            body_json(router.clone(), "/api/issue-report/req-task").await;
        assert_eq!(report_status, StatusCode::OK);
        assert_eq!(
            report_body["schema_version"],
            "dcc-mcp.admin.issue-report.v1"
        );
        assert_eq!(report_body["report_type"], "github_issue_public_safe");
        assert_eq!(report_body["privacy_mode"], "public-safe");
        assert_eq!(report_body["request_id"], "req-task");
        assert_eq!(report_body["summary"]["status"], "failed");
        assert_eq!(report_body["summary"]["dcc_type"], "maya");
        assert_eq!(report_body["summary"]["tool_family"], "long_task");
        assert_eq!(
            report_body["summary"]["error"]["kind"],
            "backend-unavailable"
        );
        assert_eq!(
            report_body["summary"]["postmortem"]["previous_call_count"],
            1
        );
        assert_eq!(
            report_body["summary"]["postmortem"]["gateway_event_count"],
            1
        );
        assert_eq!(
            report_body["summary"]["token_accounting"]["response_format"],
            "toon"
        );
        assert_eq!(
            report_body["summary"]["response_token_accounting"]["response_format"],
            "toon"
        );
        assert_eq!(
            report_body["summary"]["token_accounting"]["returned_tokens"],
            40
        );
        assert_eq!(
            report_body["summary"]["token_accounting"]["saved_tokens"],
            60
        );
        assert_eq!(
            report_body["summary"]["redaction_status"]["raw_payloads_excluded"],
            true
        );
        assert_eq!(
            report_body["summary"]["redaction_status"]["redaction_markers_detected"],
            true
        );
        assert!(report_body.get("debug_bundle").is_none());
        assert_eq!(
            report_body["summary"]["payload_tokens"]["missing_payload_tokens"],
            true
        );
        assert!(
            report_body["summary"]["token_accounting_contract"]["missing_payload_tokens"]
                .as_str()
                .is_some()
        );
        assert!(
            report_body["github_issue"]["body_template"]
                .as_str()
                .is_some_and(|body| body.contains("Public-safe diagnostics"))
        );
        assert!(
            report_body["links"]["safe_issue_report_path"]
                .as_str()
                .is_some_and(|url| url == "/admin/api/issue-report/req-task")
        );
        assert!(
            report_body["links"]["docs_path"]
                .as_str()
                .is_some_and(|url| url == "/docs")
        );
        assert!(
            report_body["raw_debug_bundle"]["admin_path"]
                .as_str()
                .is_some_and(|url| url == "/admin/api/issue-report/req-task?mode=raw")
        );
        let report_text = serde_json::to_string(&report_body).unwrap();
        for forbidden in [
            "http://",
            "127.0.0.1",
            "C:\\studio",
            "secret",
            "shot.ma",
            "callback",
            "scene.ma",
            "[REDACTED]",
            "host died while opening",
        ] {
            assert!(
                !report_text.contains(forbidden),
                "safe issue report leaked {forbidden}: {report_text}"
            );
        }
        let issue_body = report_body["github_issue"]["body_template"]
            .as_str()
            .unwrap();
        for forbidden in ["http://", "127.0.0.1", "C:\\studio", "shot.ma", "scene.ma"] {
            assert!(
                !issue_body.contains(forbidden),
                "safe issue body leaked {forbidden}: {issue_body}"
            );
        }

        let (raw_report_status, raw_report_body) =
            body_json(router, "/api/issue-report/req-task?mode=raw").await;
        assert_eq!(raw_report_status, StatusCode::OK);
        assert_eq!(raw_report_body["privacy_mode"], "raw-local-evidence");
        assert_eq!(raw_report_body["debug_bundle"]["request_id"], "req-task");
        assert_eq!(raw_report_body["debug_bundle"]["trace_id"], "trace-task");
        assert!(
            serde_json::to_string(&raw_report_body)
                .unwrap()
                .contains("scene.ma")
        );
    }

    // ── /api/logs ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_admin_logs_returns_json_array() {
        let _env = API_LOGS_ENV_LOCK.lock();
        let _no_disk = ScopedNoDiskLogsDir::new();
        let (status, body) = body_json(admin_router(), "/api/logs").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["logs"].is_array(), "expected 'logs' array, got {body}");
    }

    #[tokio::test]
    async fn test_admin_logs_empty_by_default() {
        let _env = API_LOGS_ENV_LOCK.lock();
        let _no_disk = ScopedNoDiskLogsDir::new();
        let (_, body) = body_json(admin_router(), "/api/logs").await;
        assert!(body["logs"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_admin_logs_returns_injected_event_entries() {
        let _env = API_LOGS_ENV_LOCK.lock();
        let _no_disk = ScopedNoDiskLogsDir::new();
        use crate::gateway::event_log::{ContendEvent, EventKind};

        let gs = make_gateway_state();
        gs.event_log.push(ContendEvent::new(
            EventKind::ElectionWon,
            "maya",
            "abc",
            None,
        ));
        gs.event_log.push(ContendEvent::new(
            EventKind::GhostReaped,
            "blender",
            "def",
            None,
        ));
        let state = AdminState::new(gs);
        let (status, body) = body_json(build_admin_router(state), "/api/logs").await;
        assert_eq!(status, StatusCode::OK);
        let logs = body["logs"].as_array().unwrap();
        assert_eq!(logs.len(), 2);
        // Both events present (order may vary)
        let events: Vec<_> = logs.iter().filter_map(|l| l["event"].as_str()).collect();
        assert!(
            events.contains(&"election_won"),
            "missing election_won event"
        );
        assert!(
            events.contains(&"ghost_reaped"),
            "missing ghost_reaped event"
        );
        let dcc_types: Vec<_> = logs.iter().filter_map(|l| l["dcc_type"].as_str()).collect();
        assert!(dcc_types.contains(&"maya"), "missing maya dcc_type");
        assert!(dcc_types.contains(&"blender"), "missing blender dcc_type");
        for row in logs {
            assert_eq!(
                row["source"].as_str(),
                Some("contention"),
                "contention rows must carry source=contention"
            );
        }
    }

    #[tokio::test]
    async fn test_admin_logs_limit_1000_reads_event_log_tail() {
        let _env = API_LOGS_ENV_LOCK.lock();
        let _no_disk = ScopedNoDiskLogsDir::new();
        use crate::gateway::event_log::{ContendEvent, EventKind, EventLog};

        let gs = make_gateway_state();
        for i in 0..EventLog::CAPACITY {
            gs.event_log.push(ContendEvent {
                timestamp: format!("2026-05-16T12:{:02}:{:02}.000Z", i / 60, i % 60),
                event: EventKind::GhostReaped,
                dcc_type: "maya".into(),
                instance_id: format!("event-{i:04}"),
                reason: None,
            });
        }

        let state = AdminState::new(gs);
        let (status, body) = body_json(build_admin_router(state), "/api/logs?limit=1000").await;
        assert_eq!(status, StatusCode::OK);
        let logs = body["logs"].as_array().unwrap();
        assert_eq!(logs.len(), EventLog::CAPACITY);
        assert!(
            logs.iter()
                .all(|log| log["source"].as_str() == Some("contention"))
        );
        assert!(
            logs.iter()
                .any(|log| log["instance_id"].as_str() == Some("event-0000"))
        );
        assert!(
            logs.iter()
                .any(|log| log["instance_id"].as_str() == Some("event-0999"))
        );
    }

    #[tokio::test]
    async fn test_admin_logs_limit_1000_reads_file_log_tail() {
        let _env = API_LOGS_ENV_LOCK.lock();
        let disk_logs = ScopedDiskLogsDir::new();

        let mut contents = String::new();
        for i in 0..1_000 {
            contents.push_str(&format!(
                "2026-05-16T12:{:02}:{:02}.000000Z INFO dcc_mcp_gateway: file-row-{i}\n",
                i / 60,
                i % 60
            ));
        }
        std::fs::write(disk_logs.path().join("gateway.log"), contents).unwrap();

        let (status, body) = body_json(admin_router(), "/api/logs?limit=1000").await;
        assert_eq!(status, StatusCode::OK);
        let logs = body["logs"].as_array().unwrap();
        assert_eq!(logs.len(), 1_000);
        assert!(
            logs.iter()
                .all(|log| log["source"].as_str() == Some("file"))
        );
        assert!(
            logs.iter()
                .any(|log| log["message"].as_str() == Some("file-row-0"))
        );
        assert!(
            logs.iter()
                .any(|log| log["message"].as_str() == Some("file-row-999"))
        );
    }

    #[tokio::test]
    async fn test_admin_logs_merges_audit_tail_when_audit_attached() {
        let _env = API_LOGS_ENV_LOCK.lock();
        let _no_disk = ScopedNoDiskLogsDir::new();
        use std::time::UNIX_EPOCH;

        let gs = make_gateway_state();
        let audit: AuditLog = Mutex::new(vec![
            AdminAuditRecord {
                timestamp: UNIX_EPOCH,
                request_id: "req-audit-1".into(),
                trace_id: Some("trace-audit-1".into()),
                span_id: None,
                parent_span_id: None,
                method: Some("tools/call".into()),
                instance_id: Some("deadbeef".into()),
                session_id: None,
                transport: None,
                agent_id: None,
                agent_name: None,
                agent_model: None,
                actor_id: None,
                actor_name: None,
                actor_email_hash: None,
                client_platform: None,
                client_os: None,
                client_host: None,
                auth_subject: None,
                source_ip: None,
                attribution_trust: None,
                parent_request_id: None,
                action: "maya.deadbeef.scene__info".into(),
                dcc_type: Some("maya".into()),
                success: true,
                error: None,
                duration_ms: Some(12),
                token_accounting: Some(token_telemetry("json", 50, 50)),
                llm_usage: None,
            },
            AdminAuditRecord {
                timestamp: UNIX_EPOCH + Duration::from_millis(1),
                request_id: "req-audit-2".into(),
                trace_id: Some("trace-audit-2".into()),
                span_id: None,
                parent_span_id: None,
                method: Some("tools/call".into()),
                instance_id: Some("cafebabe".into()),
                session_id: None,
                transport: None,
                agent_id: None,
                agent_name: None,
                agent_model: None,
                actor_id: None,
                actor_name: None,
                actor_email_hash: None,
                client_platform: None,
                client_os: None,
                client_host: None,
                auth_subject: None,
                source_ip: None,
                attribution_trust: None,
                parent_request_id: None,
                action: "blender.cafebabe.scene__info".into(),
                dcc_type: Some("blender".into()),
                success: false,
                error: Some("boom".into()),
                duration_ms: Some(24),
                token_accounting: None,
                llm_usage: None,
            },
        ]);
        let state = AdminState::new(gs).with_audit_log(Arc::new(audit));
        let router = build_admin_router(state);
        let (status, body) = body_json(router.clone(), "/api/logs").await;
        assert_eq!(status, StatusCode::OK);
        let logs = body["logs"].as_array().unwrap();
        assert_eq!(logs.len(), 2);
        assert!(
            logs.iter()
                .all(|log| log["source"].as_str() == Some("audit"))
        );
        assert!(
            logs.iter()
                .any(|log| log["tool"].as_str() == Some("maya.deadbeef.scene__info"))
        );
        assert!(
            logs.iter()
                .any(|log| log["token_accounting"]["response_format"].as_str() == Some("json"))
        );

        let (limited_status, limited_body) = body_json(router, "/api/logs?limit=1").await;
        assert_eq!(limited_status, StatusCode::OK);
        assert_eq!(limited_body["logs"].as_array().unwrap().len(), 1);
        assert_eq!(limited_body["total"], 1);
    }

    fn analytics_audit_record(
        request_id: &str,
        action: &str,
        instance_id: Option<&str>,
        agent_id: Option<&str>,
        success: bool,
    ) -> AdminAuditRecord {
        AdminAuditRecord {
            timestamp: std::time::SystemTime::now()
                .checked_sub(Duration::from_secs(60))
                .unwrap(),
            request_id: request_id.to_string(),
            trace_id: Some(format!("trace-{request_id}")),
            span_id: None,
            parent_span_id: None,
            method: Some("tools/call".to_string()),
            instance_id: instance_id.map(str::to_string),
            session_id: None,
            transport: Some("rest".to_string()),
            agent_id: agent_id.map(str::to_string),
            agent_name: agent_id.map(|id| format!("Agent {id}")),
            agent_model: None,
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            attribution_trust: None,
            parent_request_id: None,
            action: action.to_string(),
            dcc_type: Some("maya".to_string()),
            success,
            error: (!success).then(|| "boom".to_string()),
            duration_ms: Some(42),
            token_accounting: Some(token_telemetry("json", 100, 40)),
            llm_usage: None,
        }
    }

    #[tokio::test]
    async fn test_admin_analytics_overview_returns_unique_counts() {
        let audit: AuditLog = Mutex::new(vec![
            analytics_audit_record(
                "analytics-1",
                "maya.inst-a.scene__info",
                Some("inst-a"),
                Some("agent-a"),
                true,
            ),
            analytics_audit_record(
                "analytics-2",
                "maya.inst-b.scene__info",
                Some("inst-b"),
                Some("agent-a"),
                false,
            ),
            analytics_audit_record(
                "analytics-3",
                "maya.inst-b.scene__info",
                Some("inst-b"),
                None,
                true,
            ),
        ]);
        let state = AdminState::new(make_gateway_state()).with_audit_log(Arc::new(audit));
        let router = build_admin_router(state);

        let (status, body) = body_json(router, "/api/analytics/overview?range=7d").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["kpi"]["calls_total"], 3);
        assert_eq!(body["kpi"]["calls_failed"], 1);
        assert_eq!(body["kpi"]["unique_instances"], 2);
        assert_eq!(body["kpi"]["unique_agents"], 1);
        assert_eq!(body["daily_series"][0]["max_duration_ms"], 42);
    }

    #[tokio::test]
    async fn test_admin_analytics_csv_export_escapes_cells() {
        let audit: AuditLog = Mutex::new(vec![analytics_audit_record(
            "analytics-csv-1",
            "=cmd,tool\nline",
            Some("inst-a"),
            Some("@agent-a"),
            true,
        )]);
        let state = AdminState::new(make_gateway_state()).with_audit_log(Arc::new(audit));
        let router = build_admin_router(state);

        let (status, _headers, body) = body_text_with_accept(
            router,
            "/api/analytics/export?range=7d&format=csv",
            "text/csv",
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.starts_with("request_id,timestamp,action,dcc_type"));
        assert!(
            body.contains("\"'=cmd,tool\nline\""),
            "expected formula-prefixed and quoted action cell, got:\n{body}"
        );
        assert!(
            body.contains("'@agent-a"),
            "expected formula-prefixed agent id, got:\n{body}"
        );
    }

    #[tokio::test]
    async fn test_v1_debug_analytics_export_mirrors_admin_export() {
        let audit: AuditLog = Mutex::new(vec![analytics_audit_record(
            "analytics-v1-csv-1",
            "maya.inst.scene__info",
            Some("inst-a"),
            Some("agent-a"),
            true,
        )]);
        let state = AdminState::new(make_gateway_state()).with_audit_log(Arc::new(audit));
        let router = build_v1_debug_router(state);

        let (status, headers, body) = body_text_with_accept(
            router,
            "/v1/debug/analytics/export?range=7d&format=csv",
            "text/csv",
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("text/csv")),
        );
        assert!(body.starts_with("request_id,timestamp,action,dcc_type"));
        assert!(body.contains("analytics-v1-csv-1"));
    }

    // ── unknown routes ────────────────────────────────────────────────────

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

    // ── content-type headers ──────────────────────────────────────────────

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

    // -- Phase 3: /api/stats -----------------------------------------------

    #[tokio::test]
    async fn test_admin_stats_empty_returns_zero_total() {
        let (status, body) = body_json(admin_router(), "/api/stats").await;
        assert_eq!(status, StatusCode::OK);
        // Without a trace log attached, should return 0 or an error object.
        assert!(body.is_object());
    }

    #[tokio::test]
    async fn test_admin_stats_with_trace_log_returns_fields() {
        use crate::gateway::admin::trace::{DispatchTrace, TraceLog};
        use std::sync::Arc;
        use std::time::SystemTime;

        let log = Arc::new(TraceLog::new(100));
        log.push(DispatchTrace {
            request_id: "r1".into(),
            trace_id: "trace-stats-1".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.create_sphere".into()),
            instance_id: Some("inst-abc".into()),
            session_id: None,
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms: 150,
            ok: true,
            spans: vec![],
            input: None,
            output: None,
            token_accounting: None,
            llm_usage: None,
        });
        log.push(DispatchTrace {
            request_id: "r2".into(),
            trace_id: "trace-stats-2".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.open_file".into()),
            instance_id: Some("inst-abc".into()),
            session_id: None,
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms: 50,
            ok: false,
            spans: vec![],
            input: None,
            output: None,
            token_accounting: None,
            llm_usage: None,
        });

        let state = make_admin_state().with_trace_log(log, None);
        let router = build_admin_router(state);
        let (status, body) = body_json(router, "/api/stats?range=1h").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["range"], "1h");
        assert_eq!(body["total_calls"], 2);
        assert_eq!(body["successful_calls"], 1);
        assert_eq!(body["failed_calls"], 1);
        assert_eq!(body["success_rate"], 50.0);
        assert_eq!(body["payload_token_estimator"], "dcc-mcp-byte4-v1");
        assert!(body["total_tokens"].as_u64().is_some());
        assert!(body["avg_tokens_per_call"].as_f64().is_some());
        assert_eq!(
            body["payload_token_usage"]["token_estimator"],
            "dcc-mcp-byte4-v1"
        );
        assert!(
            body["payload_token_usage"]["calls_missing_payload_tokens"]
                .as_u64()
                .is_some()
        );
        assert!(body["latency_ms"]["p50_ms"].as_u64().unwrap() > 0);
        assert!(body["top_app_types"].is_array());
        assert_eq!(body["top_app_types"][0]["name"], "maya");
        assert!(body["top_tools"].is_array());
        assert!(body["hourly_distribution"].is_array());
        assert_eq!(body["hourly_distribution"].as_array().unwrap().len(), 24);
    }

    #[tokio::test]
    async fn test_admin_stats_all_range_is_default() {
        let (status, body) = body_json(admin_router(), "/api/stats?range=invalid").await;
        assert_eq!(status, StatusCode::OK);
        // Unknown range should fall back to "all".
        assert!(body["range"] == "all" || body.get("error").is_some());
    }

    #[tokio::test]
    async fn test_admin_traces_returns_rows_with_token_fields() {
        use crate::gateway::admin::trace::{AgentContext, DispatchTrace, TraceLog, TracePayload};
        use std::sync::Arc;
        use std::time::SystemTime;
        let log = Arc::new(TraceLog::new(100));

        let mut with_input = DispatchTrace {
            request_id: "trace-row-input".into(),
            trace_id: "trace-row-input".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.create_sphere".into()),
            instance_id: None,
            session_id: None,
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms: 123,
            ok: true,
            spans: vec![],
            input: Some(TracePayload::from_value(
                &serde_json::json!({"prompt": "a short prompt for tracing"}),
                1024,
            )),
            output: None,
            token_accounting: None,
            llm_usage: None,
        };
        with_input.trace_id = "trace-row-input-trace-id".into();
        with_input.agent_context = Some(AgentContext {
            actor_id: Some("artist-1".into()),
            actor_name: Some("Layout Artist".into()),
            client_platform: Some("cursor".into()),
            client_host: Some("workstation-7".into()),
            auth_subject: Some("user:artist-1".into()),
            source_ip: Some("192.0.2.44".into()),
            trust: AgentContextTrust {
                actor_id: Some("self_reported".into()),
                client_platform: Some("header".into()),
                auth_subject: Some("auth".into()),
                source_ip: Some("server_derived".into()),
                ..AgentContextTrust::default()
            },
            ..AgentContext::default()
        });
        log.push(with_input);

        let with_output = DispatchTrace {
            request_id: "trace-row-output".into(),
            trace_id: "trace-row-output-trace-id".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.close_file".into()),
            instance_id: None,
            session_id: None,
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms: 91,
            ok: false,
            spans: vec![],
            input: None,
            output: Some(TracePayload::from_value(
                &serde_json::json!({"result": "ok"}),
                1024,
            )),
            token_accounting: None,
            llm_usage: None,
        };
        log.push(with_output);

        let state = make_admin_state().with_trace_log(log, None);
        let router = build_admin_router(state);
        let (status, body) = body_json(router, "/api/traces?limit=20").await;
        assert_eq!(status, StatusCode::OK);
        let traces = body["traces"].as_array().unwrap();
        assert_eq!(traces.len(), 2);

        let rows_with_inputs: Vec<_> = traces
            .iter()
            .filter(|t| t["request_id"] == "trace-row-input")
            .collect();
        let rows_with_outputs: Vec<_> = traces
            .iter()
            .filter(|t| t["request_id"] == "trace-row-output")
            .collect();
        assert_eq!(rows_with_inputs.len(), 1);
        assert_eq!(rows_with_outputs.len(), 1);
        assert!(rows_with_inputs[0]["input_tokens"].as_u64().is_some());
        assert!(rows_with_outputs[0]["output_tokens"].as_u64().is_some());
        assert!(rows_with_inputs[0]["total_tokens"].as_u64().is_some());
        assert!(rows_with_outputs[0]["total_tokens"].as_u64().is_some());
        assert_eq!(
            rows_with_inputs[0]["payload_token_accounting"]["kind"],
            "payload"
        );
        assert_eq!(
            rows_with_outputs[0]["payload_token_accounting"]["missing_payload_tokens"],
            false
        );
        assert_eq!(rows_with_inputs[0]["actor"], "Layout Artist");
        assert_eq!(rows_with_inputs[0]["actor_id"], "artist-1");
        assert_eq!(rows_with_inputs[0]["client_platform"], "cursor");
        assert_eq!(rows_with_inputs[0]["client_host"], "workstation-7");
        assert_eq!(rows_with_inputs[0]["auth_subject"], "user:artist-1");
        assert_eq!(rows_with_inputs[0]["source_ip"], "192.0.2.44");
        assert_eq!(
            rows_with_inputs[0]["attribution_trust"]["auth_subject"],
            "auth"
        );
    }

    #[tokio::test]
    async fn test_admin_trace_detail_returns_token_totals() {
        use crate::gateway::admin::trace::{DispatchTrace, TraceLog, TracePayload};
        use std::sync::Arc;
        use std::time::SystemTime;
        let log = Arc::new(TraceLog::new(10));
        let trace = DispatchTrace {
            request_id: "trace-detail-input".into(),
            trace_id: "trace-detail-trace-id".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.delete_node".into()),
            instance_id: None,
            session_id: None,
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms: 42,
            ok: true,
            spans: vec![],
            input: Some(TracePayload::from_str("response body", 1024)),
            output: Some(TracePayload::from_value(
                &serde_json::json!({"ok": true}),
                1024,
            )),
            token_accounting: None,
            llm_usage: None,
        };
        log.push(trace);
        let state = make_admin_state().with_trace_log(log, None);
        let router = build_admin_router(state);
        let (status, body) = body_json(router, "/api/traces/trace-detail-input").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["request_id"], "trace-detail-input");
        assert!(body["input_tokens"].as_u64().is_some());
        assert!(body["output_tokens"].as_u64().is_some());
        assert!(body["total_tokens"].as_u64().is_some());
        assert_eq!(body["estimated_tokens"], body["total_tokens"]);
        assert_eq!(body["estimated_total_tokens"], body["total_tokens"]);
        assert_eq!(body["payload_token_estimator"], "dcc-mcp-byte4-v1");
        assert_eq!(body["payload_token_accounting"]["kind"], "payload");
        assert_eq!(
            body["payload_token_accounting"]["missing_payload_tokens"],
            false
        );
    }

    // ── /api/workers (Phase 4) ────────────────────────────────────────────

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
    async fn test_admin_workers_returns_json_shape() {
        let (status, body) = body_json(admin_router(), "/api/workers").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["workers"].is_array(), "expected workers array");
        assert!(body["summary"].is_object(), "expected summary object");
        assert_eq!(body["total"].as_u64(), Some(0));
        assert_eq!(body["summary"]["live"].as_u64(), Some(0));
        assert_eq!(body["summary"]["stale"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn test_admin_instances_defaults_to_live_rows() {
        use dcc_mcp_transport::discovery::types::ServiceStatus;

        let gs = make_gateway_state();
        {
            let reg = gs.registry.write().await;
            reg.register(make_service_entry("maya", "127.0.0.1", 18813, Some(4242)))
                .unwrap();

            let mut stale = make_service_entry("maya", "127.0.0.1", 18814, Some(4243));
            stale.last_heartbeat = std::time::SystemTime::now() - Duration::from_secs(120);
            reg.register(stale).unwrap();

            let mut unreachable = make_service_entry("3dsmax", "127.0.0.1", 18815, Some(4244));
            unreachable.status = ServiceStatus::Unreachable;
            reg.register(unreachable).unwrap();
        }

        let state = AdminState::new(gs);
        let router = build_admin_router(state);
        let (status, body) = body_json(router, "/api/instances").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["view"], "live");
        assert_eq!(body["total"].as_u64(), Some(1));
        assert_eq!(body["summary"]["live"].as_u64(), Some(1));
        let rows = body["instances"].as_array().unwrap();
        assert_eq!(rows[0]["dcc_type"], "maya");
        assert_eq!(rows[0]["port"], 18813);
    }

    #[tokio::test]
    async fn test_admin_instances_all_view_keeps_diagnostic_rows() {
        use dcc_mcp_transport::discovery::types::ServiceStatus;

        let gs = make_gateway_state();
        {
            let reg = gs.registry.write().await;
            reg.register(make_service_entry("maya", "127.0.0.1", 18813, Some(4242)))
                .unwrap();

            let mut unreachable = make_service_entry("3dsmax", "127.0.0.1", 18815, Some(4244));
            unreachable.status = ServiceStatus::Unreachable;
            reg.register(unreachable).unwrap();
        }

        let state = AdminState::new(gs);
        let router = build_admin_router(state);
        let (status, body) = body_json(router, "/api/instances?view=all").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["view"], "all");
        assert_eq!(body["total"].as_u64(), Some(2));
        assert_eq!(body["summary"]["live"].as_u64(), Some(1));
        assert_eq!(body["summary"]["unhealthy"].as_u64(), Some(1));
    }

    #[tokio::test]
    async fn test_admin_workers_with_registered_instance() {
        let gs = make_gateway_state();
        // Inject one ServiceEntry into the registry.
        {
            let reg = gs.registry.write().await;
            let mut entry = make_service_entry("maya", "127.0.0.1", 18813, Some(4242));
            entry
                .metadata
                .insert("host_rpc_uri".into(), "commandport://127.0.0.1:6000".into());
            entry
                .metadata
                .insert("host_rpc_scheme".into(), "commandport".into());
            entry
                .metadata
                .insert("dispatch_status".into(), "ready".into());
            entry
                .metadata
                .insert("dispatch_ready_at_unix".into(), "1780367000".into());
            entry
                .metadata
                .insert("mcp_url".into(), "http://127.0.0.1:18813/mcp".into());
            entry
                .metadata
                .insert("gateway_runtime_mode".into(), "daemon-backed".into());
            entry
                .metadata
                .insert("gateway_guardian_enabled".into(), "true".into());
            reg.register(entry).unwrap();
        }
        let state = AdminState::new(gs);
        let router = build_admin_router(state);
        let (status, body) = body_json(router, "/api/workers").await;
        assert_eq!(status, StatusCode::OK);
        let workers = body["workers"].as_array().unwrap();
        assert_eq!(workers.len(), 1, "expected 1 worker, got {workers:?}");
        let w = &workers[0];
        assert_eq!(w["dcc_type"], "maya");
        assert_eq!(w["pid"], 4242);
        assert_eq!(w["host"], "127.0.0.1");
        assert_eq!(w["port"], 18813);
        assert_eq!(w["mcp_url"], "http://127.0.0.1:18813/mcp");
        assert_eq!(w["adapter_version"], "0.3.0");
        assert_eq!(w["host_rpc_uri"], "commandport://127.0.0.1:6000");
        assert_eq!(w["host_rpc_scheme"], "commandport");
        assert_eq!(w["dispatch_status"], "ready");
        assert_eq!(w["dispatch_ready"], true);
        assert_eq!(w["dispatch_ready_at_unix"], "1780367000");
        assert_eq!(w["gateway_runtime_mode"], "daemon-backed");
        assert_eq!(w["gateway_guardian_enabled"], true);
        assert_eq!(w["gateway_recovery_driver"], "daemon_guardian");
        assert_eq!(w["registration_refresh_mode"], "file_registry_heartbeat");
        // CPU/memory not yet wired — see workers.rs module docs.
        assert!(w["cpu_percent"].is_null());
        assert!(w["memory_bytes"].is_null());
        assert!(w["uptime_secs"].as_u64().is_some());
        // summary should reflect 1 live, 0 stale.
        assert_eq!(body["total"].as_u64(), Some(1));
        assert_eq!(body["summary"]["live"].as_u64(), Some(1));
        assert_eq!(body["summary"]["stale"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn test_admin_workers_keeps_booting_failure_rows_visible() {
        use dcc_mcp_transport::discovery::types::ServiceStatus;

        let gs = make_gateway_state();
        {
            let reg = gs.registry.write().await;
            let mut booting = make_service_entry("3dsmax", "127.0.0.1", 0, Some(4244));
            booting.status = ServiceStatus::Booting;
            booting
                .metadata
                .insert("failure_reason".into(), "host-rpc connect failed".into());
            booting
                .metadata
                .insert("failure_stage".into(), "host-rpc-connect".into());
            booting
                .metadata
                .insert("host_rpc_uri".into(), "commandport://127.0.0.1:6000".into());
            booting
                .metadata
                .insert("host_rpc_scheme".into(), "commandport".into());
            booting
                .metadata
                .insert("dispatch_status".into(), "unavailable".into());
            reg.register(booting).unwrap();
        }
        let state = AdminState::new(gs);
        let router = build_admin_router(state);
        let (status, body) = body_json(router, "/api/workers").await;
        assert_eq!(status, StatusCode::OK);
        let workers = body["workers"].as_array().unwrap();
        assert_eq!(workers.len(), 1, "expected booting worker row");
        assert_eq!(workers[0]["status"], "booting");
        assert_eq!(workers[0]["port"], 0);
        assert_eq!(workers[0]["failure_reason"], "host-rpc connect failed");
        assert_eq!(workers[0]["failure_stage"], "host-rpc-connect");
        assert_eq!(workers[0]["host_rpc_scheme"], "commandport");
        assert_eq!(workers[0]["dispatch_status"], "unavailable");
        assert_eq!(workers[0]["dispatch_ready"], false);
        assert_eq!(body["summary"]["unhealthy"].as_u64(), Some(1));
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

    #[tokio::test]
    async fn test_admin_workers_hides_stale_registry_rows() {
        let gs = make_gateway_state();
        {
            let reg = gs.registry.write().await;
            reg.register(make_service_entry("maya", "127.0.0.1", 18813, Some(4242)))
                .unwrap();

            let mut stale = make_service_entry("maya", "127.0.0.1", 18814, Some(4243));
            stale.last_heartbeat = std::time::SystemTime::now() - Duration::from_secs(120);
            reg.register(stale).unwrap();
        }

        let state = AdminState::new(gs);
        let router = build_admin_router(state);
        let (status, body) = body_json(router, "/api/workers").await;

        assert_eq!(status, StatusCode::OK);
        let workers = body["workers"].as_array().unwrap();
        assert_eq!(
            workers.len(),
            1,
            "expected only live workers, got {workers:?}"
        );
        assert_eq!(workers[0]["port"], 18813);
        assert_eq!(body["total"].as_u64(), Some(1));
        assert_eq!(body["summary"]["live"].as_u64(), Some(1));
        assert_eq!(body["summary"]["stale"].as_u64(), Some(0));
    }

    // ── /api/skill-paths (CRUD) ─────────────────────────────────────────

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
        // The display string pairs a friendly source label with a safe folder
        // tail (never the absolute local path) so same-source rows stay
        // distinguishable.
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

        // POST a new skill path
        let resp = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/skill-paths")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({"path": "/tmp/new-skills"}))
                            .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(reload_calls.load(Ordering::SeqCst), 1);

        // GET should now include the new path
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

        // DELETE the custom path
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

        // GET should no longer include the deleted path
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
        let state = make_admin_state();
        let router = build_admin_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/skill-paths")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({"path": ""})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_admin_skill_path_post_without_lane_returns_503() {
        // AdminState without sqlite lane attached
        let state = make_admin_state();
        let router = build_admin_router(state);
        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/skill-paths")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&serde_json::json!({"path": "/valid/path"})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
