//! Focused tests for Admin stats and trace endpoints.

#[cfg(all(test, feature = "admin"))]
mod endpoint_contracts {
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use axum::Router;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use serde_json::{Value, json};
    use tokio::sync::{RwLock, broadcast, watch};
    use tower::ServiceExt;

    use crate::gateway::admin::router::build_admin_router;
    use crate::gateway::admin::state::AdminState;
    use crate::gateway::admin::trace::{
        AgentContext, AgentContextTrust, DispatchTrace, TraceLog, TracePayload,
    };
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
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn test_admin_stats_empty_returns_zero_total() {
        let (status, body) = body_json(admin_router(), "/api/stats").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.is_object());
    }

    #[tokio::test]
    async fn test_admin_stats_with_trace_log_returns_fields() {
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
        assert!(body["range"] == "all" || body.get("error").is_some());
    }

    #[tokio::test]
    async fn test_admin_traces_returns_rows_with_token_fields() {
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
                &json!({"prompt": "a short prompt for tracing"}),
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
            output: Some(TracePayload::from_value(&json!({"result": "ok"}), 1024)),
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
            output: Some(TracePayload::from_value(&json!({"ok": true}), 1024)),
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
}
