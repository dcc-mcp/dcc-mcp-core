//! Tests for the admin UI handlers.

#[cfg(all(test, feature = "admin"))]
#[allow(clippy::await_holding_lock)] // Intentional: parking_lot Mutex for env-var test serialization
mod admin_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use axum::Router;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use parking_lot::Mutex;
    use serde_json::{Value, json};
    use tokio::sync::{RwLock, broadcast, oneshot, watch};
    use tower::ServiceExt;

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
}
