//! Focused tests for durable record/replay lifecycle contracts.

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::oneshot;
use tower::ServiceExt;

use crate::gateway::admin::application::router::build_v1_debug_router;
use crate::gateway::admin::sqlite_lane::AdminSqliteLane;
use crate::gateway::admin::tests::admin_tests::{
    make_admin_state, make_service_entry, post_json_as_session,
};
use crate::gateway::admin::trace::{DispatchTrace, TraceLog, TracePayload};
use crate::gateway::traffic::{TrafficFrame, basic_gateway_source, correlation, mcp_message};

async fn get_json_as_session(router: Router, uri: &str, session_id: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("x-dcc-mcp-agent-session-id", session_id)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn gateway_restart_recovers_incremental_recording_as_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("recordings.sqlite");
    let session_id = "maya-recording-session";

    let lane = AdminSqliteLane::spawn(db_path.clone(), 30).unwrap();
    let state = make_admin_state().with_admin_sqlite_lane(Some(lane.clone()));
    let traffic = state.gateway.traffic_capture.clone();
    let router = build_v1_debug_router(state);

    let (status, started) = post_json_as_session(
        router.clone(),
        "/v1/recordings/start",
        session_id,
        json!({"dcc_type": "maya"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let recording_id = started["recording_id"].as_str().unwrap();

    traffic.emit_json_frame(
        TrafficFrame::json(
            basic_gateway_source(),
            correlation(Some("req-1"), Some("trace-1"), Some(session_id)),
            "inbound",
            "agent_to_gateway",
            "mcp",
            json!({
                "tool_slug": "maya.abcd.scene__build",
                "arguments": {"output": "demo", "api_token": "secret"}
            }),
        )
        .with_session_id(Some(session_id))
        .with_mcp(mcp_message("request", "tools/call", Some(json!(1)))),
    );
    traffic.emit_json_frame(
        TrafficFrame::json(
            basic_gateway_source(),
            correlation(Some("req-1"), Some("trace-1"), Some(session_id)),
            "outbound",
            "gateway_to_agent",
            "mcp",
            json!({"result": {"isError": false}}),
        )
        .with_session_id(Some(session_id))
        .with_mcp(mcp_message("response", "tools/call", Some(json!(1)))),
    );

    drop(router);
    drop(traffic);
    drop(lane);

    let recovered_lane = AdminSqliteLane::spawn(db_path, 30).unwrap();
    let recovered_state = make_admin_state().with_admin_sqlite_lane(Some(recovered_lane.clone()));
    let recovered_router = build_v1_debug_router(recovered_state);

    let (status, recording) = get_json_as_session(
        recovered_router.clone(),
        &format!("/v1/recordings/{recording_id}"),
        session_id,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recording["status"], "interrupted");
    assert_eq!(recording["events"][0]["success"], true);
    assert_eq!(
        recording["events"][0]["arguments"]["api_token"],
        "[REDACTED_SENSITIVE_INPUT]"
    );

    let (status, _) = post_json_as_session(
        recovered_router,
        "/v1/recordings/start",
        session_id,
        json!({"dcc_type": "maya"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn gateway_restart_preserves_stopped_recording_status() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("stopped-recording.sqlite");
    let session_id = "houdini-recording-session";

    let lane = AdminSqliteLane::spawn(db_path.clone(), 30).unwrap();
    let state = make_admin_state().with_admin_sqlite_lane(Some(lane.clone()));
    let traffic = state.gateway.traffic_capture.clone();
    let router = build_v1_debug_router(state);

    let (status, started) = post_json_as_session(
        router.clone(),
        "/v1/recordings/start",
        session_id,
        json!({"dcc_type": "houdini"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let recording_id = started["recording_id"].as_str().unwrap();

    traffic.emit_json_frame(
        TrafficFrame::json(
            basic_gateway_source(),
            correlation(Some("req-2"), Some("trace-2"), Some(session_id)),
            "inbound",
            "agent_to_gateway",
            "mcp",
            json!({
                "tool_slug": "houdini.efgh.scene__build",
                "arguments": {"output": "demo"}
            }),
        )
        .with_session_id(Some(session_id))
        .with_mcp(mcp_message("request", "tools/call", Some(json!(2)))),
    );
    traffic.emit_json_frame(
        TrafficFrame::json(
            basic_gateway_source(),
            correlation(Some("req-2"), Some("trace-2"), Some(session_id)),
            "outbound",
            "gateway_to_agent",
            "mcp",
            json!({"result": {"isError": false}}),
        )
        .with_session_id(Some(session_id))
        .with_mcp(mcp_message("response", "tools/call", Some(json!(2)))),
    );
    let (status, stopped) = post_json_as_session(
        router.clone(),
        "/v1/recordings/stop",
        session_id,
        json!({"recording_id": recording_id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stopped["status"], "stopped");

    drop(router);
    drop(traffic);
    drop(lane);

    let recovered_lane = AdminSqliteLane::spawn(db_path, 30).unwrap();
    let recovered_state = make_admin_state().with_admin_sqlite_lane(Some(recovered_lane));
    let recovered_router = build_v1_debug_router(recovered_state);

    let (status, recording) = get_json_as_session(
        recovered_router,
        &format!("/v1/recordings/{recording_id}"),
        session_id,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recording["status"], "stopped");
    assert_eq!(recording["events"].as_array().unwrap().len(), 1);
    assert_eq!(recording["events"][0]["success"], true);
}

#[tokio::test]
async fn retroactive_session_compile_keeps_explicit_review_gate() {
    let router = build_v1_debug_router(make_admin_state());
    let (status, body) = post_json_as_session(
        router,
        "/v1/recordings/compile-session",
        "completed-session",
        json!({
            "session_id": "completed-session",
            "name": "reviewed-session-workflow",
            "reviewed": false
        }),
    )
    .await;

    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(body["error"], "review_required");
}

#[tokio::test]
async fn completed_session_history_compiles_without_prior_recording_start() {
    let backend = Router::new().route(
        "/v1/describe",
        axum::routing::post(|| async {
            axum::Json(json!({
                "entry": {"action": "maya_primitives__create_sphere", "skill": "maya-primitives"},
                "description": "Create a sphere",
                "input_schema": {
                    "type": "object",
                    "properties": {"radius": {"type": "number"}}
                },
                "annotations": {"readOnlyHint": false, "idempotentHint": false}
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, backend)
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await;
    });

    let state = make_admin_state();
    let mut entry = make_service_entry("maya", "127.0.0.1", port, None);
    entry.metadata.insert(
        crate::gateway::http_registration::DISCOVERY_MCP_URL_METADATA_KEY.to_string(),
        format!("http://127.0.0.1:{port}/mcp"),
    );
    let instance_id = entry.instance_id;
    state.gateway.registry.register(entry).unwrap();
    let tool_slug = crate::gateway::capability::tool_slug(
        "maya",
        &instance_id,
        "maya_primitives__create_sphere",
    );
    state.gateway.capability_index.upsert_instance(
        instance_id,
        vec![crate::gateway::capability::CapabilityRecord::new(
            tool_slug.clone(),
            "maya_primitives__create_sphere".to_string(),
            "maya_primitives__create_sphere".to_string(),
            Some("maya-primitives".to_string()),
            "Create a sphere",
            vec![],
            "maya".to_string(),
            instance_id,
            true,
            true,
            None,
        )],
        crate::gateway::capability::InstanceFingerprint(1),
    );
    let traces = Arc::new(TraceLog::new(10));
    traces.push(DispatchTrace {
        request_id: "completed-request".into(),
        trace_id: "completed-trace".into(),
        span_id: None,
        parent_span_id: None,
        parent_request_id: None,
        trace_flags: None,
        trace_state: None,
        method: "tools/call".into(),
        tool_slug: Some(tool_slug),
        instance_id: Some(instance_id.to_string()),
        session_id: Some("completed-session".into()),
        dcc_type: Some("maya".into()),
        transport: Some("mcp".into()),
        agent_context: None,
        started_at: SystemTime::now(),
        total_ms: 12,
        ok: true,
        spans: vec![],
        input: Some(TracePayload::from_input_value(
            &json!({"radius": 2.0}),
            1024,
        )),
        output: None,
        script_execution: None,
        token_accounting: None,
        llm_usage: None,
    });
    let router = build_v1_debug_router(state.with_trace_log(traces, None));

    let (status, body) = post_json_as_session(
        router,
        "/v1/recordings/compile-session",
        "completed-session",
        json!({
            "session_id": "completed-session",
            "name": "reviewed-session-workflow",
            "reviewed": true
        }),
    )
    .await;
    let _ = stop_tx.send(());

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["source"], "session_history");
    assert_eq!(body["replay_authorized"], false);
    assert_eq!(
        body["compiled"]["workflow"]["steps"][0]["tool"],
        "maya_primitives__create_sphere"
    );
}

#[tokio::test]
async fn retroactive_session_compile_rejects_ui_act_without_semantic_find() {
    let state = make_admin_state();
    let instance_id = uuid::Uuid::new_v4();
    let tool_slug = crate::gateway::capability::tool_slug("maya", &instance_id, "ui_control__act");
    state.gateway.capability_index.upsert_instance(
        instance_id,
        vec![crate::gateway::capability::CapabilityRecord::new(
            tool_slug.clone(),
            "ui_control__act".to_string(),
            "ui_control__act".to_string(),
            Some("ui-control".to_string()),
            "Act on a semantic control",
            vec![],
            "maya".to_string(),
            instance_id,
            true,
            true,
            None,
        )],
        crate::gateway::capability::InstanceFingerprint(1),
    );
    let traces = Arc::new(TraceLog::new(10));
    traces.push(DispatchTrace {
        request_id: "ui-request".into(),
        trace_id: "ui-trace".into(),
        span_id: None,
        parent_span_id: None,
        parent_request_id: None,
        trace_flags: None,
        trace_state: None,
        method: "tools/call".into(),
        tool_slug: Some(tool_slug),
        instance_id: Some(instance_id.to_string()),
        session_id: Some("ui-session".into()),
        dcc_type: Some("maya".into()),
        transport: Some("mcp".into()),
        agent_context: None,
        started_at: SystemTime::now(),
        total_ms: 4,
        ok: true,
        spans: vec![],
        input: Some(TracePayload::from_input_value(
            &json!({"action": "click", "session": "default"}),
            1024,
        )),
        output: None,
        script_execution: None,
        token_accounting: None,
        llm_usage: None,
    });
    let router = build_v1_debug_router(state.with_trace_log(traces, None));

    let (status, body) = post_json_as_session(
        router,
        "/v1/recordings/compile-session",
        "ui-session",
        json!({
            "session_id": "ui-session",
            "name": "unsafe-ui-history",
            "reviewed": true
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "semantic_query_missing");
}
