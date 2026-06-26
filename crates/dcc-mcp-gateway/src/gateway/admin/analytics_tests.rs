//! Tests for admin analytics endpoints.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::to_bytes;
use axum::http::{HeaderMap, Request, StatusCode, header};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{RwLock, broadcast, watch};
use tower::ServiceExt;

use crate::gateway::admin::router::{build_admin_router, build_v1_debug_router};
use crate::gateway::admin::state::{AdminAuditRecord, AdminState, AuditLog};
use crate::gateway::admin::trace::TokenTelemetry;
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
        search_telemetry: Arc::new(crate::gateway::search_telemetry::SearchTelemetryStore::new()),
        debug_routes_enabled: false,
        auth: Arc::new(crate::gateway::security::GatewayAuth::disabled()),
        update_manifest_url: None,
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
    }
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
