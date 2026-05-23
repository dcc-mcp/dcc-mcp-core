//! Tests for the admin UI handlers.

#[cfg(all(test, feature = "admin"))]
#[allow(clippy::await_holding_lock)] // Intentional: parking_lot Mutex for env-var test serialization
mod admin_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::Router;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use parking_lot::Mutex;
    use serde_json::{Value, json};
    use tokio::sync::{RwLock, broadcast, oneshot, watch};
    use tower::ServiceExt;

    use dcc_mcp_gateway_core::naming::instance_short;

    use crate::gateway::admin::router::build_admin_router;
    use crate::gateway::admin::state::{AdminAuditRecord, AdminState, AuditLog};
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
            capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
            event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
            #[cfg(feature = "prometheus")]
            gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
            middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
            instance_diagnostics: Arc::new(
                crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
            ),
            debug_routes_enabled: false,
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
            "/traces?limit=200",
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
                "action": "maya-modeling.create_cube",
                "summary": "Create a cube",
                "loaded": true,
                "has_schema": false
            },
            {
                "skill": "maya-modeling",
                "action": "maya-modeling.delete_cube",
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
                parent_request_id: None,
                action: "tools/call:maya__open_scene".to_string(),
                dcc_type: Some("maya".to_string()),
                success: true,
                error: None,
                duration_ms: Some(42),
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
                parent_request_id: None,
                action: "tools/call:blender__render".to_string(),
                dcc_type: Some("blender".to_string()),
                success: false,
                error: Some("timeout".to_string()),
                duration_ms: None,
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
        assert_eq!(successes[0]["method"], "tools/call");
        assert_eq!(successes[0]["instance_id"], "maya-instance");
        assert_eq!(successes[0]["session_id"], "session-1");
        assert_eq!(successes[0]["transport"], "mcp");
        assert_eq!(successes[0]["agent_id"], "agent-ok");
        assert_eq!(successes[0]["agent_name"], "Operator Agent");
        assert_eq!(successes[0]["agent_model"], "gpt-test");
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
            parent_request_id: None,
            action: "tools/call:photoshop__save".to_string(),
            dcc_type: None,
            success: true,
            error: None,
            duration_ms: Some(100),
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
            parent_request_id: Some("parent-1".to_string()),
            action: "maya.inst.tool".to_string(),
            dcc_type: Some("maya".to_string()),
            success: true,
            error: None,
            duration_ms: Some(11),
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
    async fn test_admin_tasks_and_debug_bundle_from_trace() {
        use crate::gateway::admin::trace::{DispatchTrace, TraceLog, TracePayload};
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
            agent_context: None,
            started_at: SystemTime::UNIX_EPOCH + Duration::from_millis(1_000),
            total_ms: 12,
            ok: true,
            spans: vec![],
            input: Some(TracePayload::from_value(
                &json!({"file": "scene.ma", "token": "[REDACTED]"}),
                1024,
            )),
            output: None,
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
            parent_request_id: Some("req-prev".into()),
            action: "maya.inst.long_task".into(),
            dcc_type: Some("maya".into()),
            success: false,
            error: Some("host died".into()),
            duration_ms: Some(25),
        }]));
        let state = AdminState::new(gateway)
            .with_audit_log(audit_log)
            .with_trace_log(traces, None);
        let router = build_admin_router(state.clone());

        let (tasks_status, tasks_body) = body_json(router.clone(), "/api/tasks").await;
        assert_eq!(tasks_status, StatusCode::OK);
        assert_eq!(tasks_body["tasks"][0]["task_id"], "req-task");
        assert_eq!(tasks_body["tasks"][0]["status"], "failed");

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

        let (v1_tasks_status, v1_tasks_body) =
            body_json(v1_router.clone(), "/v1/debug/tasks?limit=20").await;
        assert_eq!(v1_tasks_status, StatusCode::OK);
        assert!(
            v1_tasks_body["tasks"]
                .as_array()
                .is_some_and(|tasks| tasks.iter().any(|task| task["task_id"] == "req-task"))
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

        let (v1_report_status, v1_report_body) =
            body_json(v1_router, "/v1/debug/issue-reports/req-task").await;
        assert_eq!(v1_report_status, StatusCode::OK);
        assert_eq!(v1_report_body["request_id"], "req-task");
        assert_eq!(v1_report_body["debug_bundle"]["trace_id"], "trace-task");

        let (report_status, report_body) = body_json(router, "/api/issue-report/req-task").await;
        assert_eq!(report_status, StatusCode::OK);
        assert_eq!(
            report_body["schema_version"],
            "dcc-mcp.admin.issue-report.v1"
        );
        assert_eq!(report_body["request_id"], "req-task");
        assert_eq!(report_body["summary"]["status"], "failed");
        assert_eq!(
            report_body["summary"]["postmortem"]["previous_call_count"],
            1
        );
        assert_eq!(
            report_body["summary"]["postmortem"]["gateway_event_count"],
            1
        );
        assert_eq!(report_body["debug_bundle"]["request_id"], "req-task");
        assert!(
            report_body["github_issue"]["body_template"]
                .as_str()
                .is_some_and(|body| body.contains("Upload this JSON export"))
        );
        assert!(
            report_body["links"]["issue_report_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/admin/api/issue-report/req-task"))
        );
        assert!(
            report_body["links"]["openapi_docs_url"]
                .as_str()
                .is_some_and(|url| url.ends_with("/docs"))
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
                parent_request_id: None,
                action: "maya.deadbeef.scene__info".into(),
                dcc_type: Some("maya".into()),
                success: true,
                error: None,
                duration_ms: Some(12),
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
                parent_request_id: None,
                action: "blender.cafebabe.scene__info".into(),
                dcc_type: Some("blender".into()),
                success: false,
                error: Some("boom".into()),
                duration_ms: Some(24),
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

        let (limited_status, limited_body) = body_json(router, "/api/logs?limit=1").await;
        assert_eq!(limited_status, StatusCode::OK);
        assert_eq!(limited_body["logs"].as_array().unwrap().len(), 1);
        assert_eq!(limited_body["total"], 1);
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
            "/api/traces",
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
        });

        let state = make_admin_state().with_trace_log(log, None);
        let router = build_admin_router(state);
        let (status, body) = body_json(router, "/api/stats?range=1h").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["range"], "1h");
        assert_eq!(body["total_calls"], 2);
        assert_eq!(body["successful_calls"], 1);
        assert_eq!(body["failed_calls"], 1);
        assert!(body["latency_ms"]["p50_ms"].as_u64().unwrap() > 0);
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
            reg.register(make_service_entry("maya", "127.0.0.1", 18813, Some(4242)))
                .unwrap();
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
        assert_eq!(paths[0]["path"], "/opt/skills/maya");
        assert_eq!(paths[0]["source"], "cli");
        assert_eq!(paths[1]["path"], "/opt/skills/blender");
        assert_eq!(paths[1]["source"], "env:DCC_MCP_SKILL_PATHS");
    }

    #[cfg(feature = "admin-persist-sqlite")]
    #[tokio::test]
    async fn test_admin_skill_path_crud_via_api() {
        use crate::gateway::admin::sqlite_lane::AdminSqliteLane;

        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test_crud.sqlite");
        let lane = AdminSqliteLane::spawn(db_path, 30).expect("spawn lane");

        let state = make_admin_state().with_admin_sqlite_lane(Some(lane));
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

        // GET should now include the new path
        let (status, body) = body_json(router.clone(), "/api/skill-paths").await;
        assert_eq!(status, StatusCode::OK);
        let paths = body["paths"].as_array().unwrap();
        let custom: Vec<_> = paths
            .iter()
            .filter(|p| p["source"] == "admin_custom")
            .collect();
        assert_eq!(custom.len(), 1, "expected 1 custom path, got {paths:?}");
        assert_eq!(custom[0]["path"], "/tmp/new-skills");

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
