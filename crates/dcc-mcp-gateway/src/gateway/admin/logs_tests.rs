//! Focused tests for Admin logs APIs.

#![allow(clippy::await_holding_lock)] // Intentional: parking_lot Mutex serializes env-var tests.

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
use crate::gateway::admin::trace::TokenTelemetry;
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
        // SAFETY: same as `new` - guarded by the test mutex.
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
        // SAFETY: same as `new` - guarded by the test mutex.
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
        search_telemetry: Arc::new(crate::gateway::search_telemetry::SearchTelemetryStore::new()),
        debug_routes_enabled: false,
        auth: Arc::new(crate::gateway::security::GatewayAuth::disabled()),
        update_manifest_url: None,
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
    }
}

fn admin_router() -> Router {
    build_admin_router(AdminState::new(make_gateway_state()))
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
