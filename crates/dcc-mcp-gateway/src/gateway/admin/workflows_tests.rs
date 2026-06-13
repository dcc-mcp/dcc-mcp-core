//! Tests for admin workflow, task, and debug-bundle endpoints.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::to_bytes;
use axum::http::{HeaderMap, Request, StatusCode, header};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};
use tower::ServiceExt;

use crate::gateway::admin::router::{build_admin_router, build_v1_debug_router};
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

    let (v1_status, v1_body) = body_json(v1_router.clone(), "/v1/debug/bundles/trace-task").await;
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

    let (compact_trace_status, compact_trace_headers, compact_trace_text) = body_text_with_accept(
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
