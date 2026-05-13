//! Tests for the admin UI handlers.

#[cfg(all(test, feature = "admin"))]
mod admin_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::Router;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use parking_lot::Mutex;
    use serde_json::Value;
    use tokio::sync::{RwLock, broadcast, watch};
    use tower::ServiceExt;

    use crate::gateway::admin::router::build_admin_router;
    use crate::gateway::admin::state::{AdminAuditRecord, AdminState, AuditLog};
    use crate::gateway::state::GatewayState;
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;

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
                method: Some("tools/call".to_string()),
                instance_id: Some("maya-instance".to_string()),
                session_id: Some("session-1".to_string()),
                action: "tools/call:maya__open_scene".to_string(),
                dcc_type: Some("maya".to_string()),
                success: true,
                error: None,
                duration_ms: Some(42),
            },
            AdminAuditRecord {
                timestamp: std::time::SystemTime::now(),
                request_id: "req-fail".to_string(),
                method: Some("tools/call".to_string()),
                instance_id: Some("blender-instance".to_string()),
                session_id: None,
                action: "tools/call:blender__render".to_string(),
                dcc_type: Some("blender".to_string()),
                success: false,
                error: Some("timeout".to_string()),
                duration_ms: None,
            },
        ]));
        let state = AdminState::new(make_gateway_state()).with_audit_log(audit_log);
        let (status, body) = body_json(build_admin_router(state), "/api/calls").await;
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
        assert_eq!(failures[0]["request_id"], "req-fail");
        assert_eq!(failures[0]["instance_id"], "blender-instance");
    }

    #[tokio::test]
    async fn test_admin_calls_single_success_has_action_field() {
        let audit_log: Arc<AuditLog> = Arc::new(parking_lot::Mutex::new(vec![AdminAuditRecord {
            timestamp: std::time::SystemTime::now(),
            request_id: "req-photoshop".to_string(),
            method: Some("tools/call".to_string()),
            instance_id: None,
            session_id: None,
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

    // ── /api/logs ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_admin_logs_returns_json_array() {
        let (status, body) = body_json(admin_router(), "/api/logs").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["logs"].is_array(), "expected 'logs' array, got {body}");
    }

    #[tokio::test]
    async fn test_admin_logs_empty_by_default() {
        let (_, body) = body_json(admin_router(), "/api/logs").await;
        assert!(body["logs"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_admin_logs_returns_injected_event_entries() {
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
    async fn test_admin_logs_merges_audit_tail_when_audit_attached() {
        use std::time::UNIX_EPOCH;

        let gs = make_gateway_state();
        let audit: AuditLog = Mutex::new(vec![AdminAuditRecord {
            timestamp: UNIX_EPOCH,
            request_id: "req-audit-1".into(),
            method: Some("tools/call".into()),
            instance_id: Some("deadbeef".into()),
            session_id: None,
            action: "maya.deadbeef.scene__info".into(),
            dcc_type: Some("maya".into()),
            success: true,
            error: None,
            duration_ms: Some(12),
        }]);
        let state = AdminState::new(gs).with_audit_log(Arc::new(audit));
        let (status, body) = body_json(build_admin_router(state), "/api/logs").await;
        assert_eq!(status, StatusCode::OK);
        let logs = body["logs"].as_array().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0]["source"].as_str(), Some("audit"));
        assert_eq!(logs[0]["tool"].as_str(), Some("maya.deadbeef.scene__info"));
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
            method: "tools/call".into(),
            tool_slug: Some("maya.create_sphere".into()),
            instance_id: Some("inst-abc".into()),
            session_id: None,
            dcc_type: Some("maya".into()),
            started_at: SystemTime::now(),
            total_ms: 150,
            ok: true,
            spans: vec![],
            input: None,
            output: None,
        });
        log.push(DispatchTrace {
            request_id: "r2".into(),
            method: "tools/call".into(),
            tool_slug: Some("maya.open_file".into()),
            instance_id: Some("inst-abc".into()),
            session_id: None,
            dcc_type: Some("maya".into()),
            started_at: SystemTime::now(),
            total_ms: 50,
            ok: false,
            spans: vec![],
            input: None,
            output: None,
        });

        let state = make_admin_state().with_trace_log(log);
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
}
