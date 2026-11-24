//! Focused tests for durable record/replay lifecycle contracts.

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
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

fn tool_trace(session_id: &str, tool_slug: String, arguments: Value) -> DispatchTrace {
    DispatchTrace {
        request_id: format!("{session_id}-request"),
        trace_id: format!("{session_id}-trace"),
        span_id: None,
        parent_span_id: None,
        parent_request_id: None,
        trace_flags: None,
        trace_state: None,
        method: "tools/call".into(),
        tool_slug: Some(tool_slug),
        instance_id: None,
        session_id: Some(session_id.to_owned()),
        dcc_type: Some("maya".into()),
        transport: Some("mcp".into()),
        agent_context: None,
        started_at: SystemTime::now(),
        total_ms: 12,
        ok: true,
        spans: vec![],
        input: Some(TracePayload::from_value(&arguments, 1024)),
        output: None,
        script_execution: None,
        token_accounting: None,
        llm_usage: None,
    }
}

fn register_backend(state: &crate::gateway::admin::state::AdminState, port: u16) -> uuid::Uuid {
    let mut entry = make_service_entry("maya", "127.0.0.1", port, None);
    entry.metadata.insert(
        crate::gateway::http_registration::DISCOVERY_MCP_URL_METADATA_KEY.to_string(),
        format!("http://127.0.0.1:{port}/mcp"),
    );
    let instance_id = entry.instance_id;
    state.gateway.registry.register(entry).unwrap();
    instance_id
}

fn register_tool(
    state: &crate::gateway::admin::state::AdminState,
    instance_id: uuid::Uuid,
    tool: &str,
) -> String {
    let tool_slug = crate::gateway::capability::tool_slug("maya", &instance_id, tool);
    state.gateway.capability_index.upsert_instance(
        instance_id,
        vec![crate::gateway::capability::CapabilityRecord::new(
            tool_slug.clone(),
            tool.to_string(),
            tool.to_string(),
            Some("maya-primitives".to_string()),
            "Maya tool",
            vec![],
            "maya".to_string(),
            instance_id,
            true,
            true,
            None,
        )],
        crate::gateway::capability::InstanceFingerprint(1),
    );
    tool_slug
}
async fn post_compile_session(router: Router, session_id: &str) -> (StatusCode, Value) {
    post_json_as_session(
        router,
        "/v1/recordings/compile-session",
        session_id,
        json!({
            "session_id": session_id,
            "name": "session-workflow",
            "reviewed": true
        }),
    )
    .await
}

fn houdini_tool_trace(session_id: &str, tool_slug: String, arguments: Value) -> DispatchTrace {
    let mut trace = tool_trace(session_id, tool_slug, arguments);
    trace.dcc_type = Some("houdini".into());
    trace
}
#[cfg(feature = "admin-persist-sqlite")]
#[tokio::test]
async fn session_compile_uses_persisted_trace_outside_recent_window() {
    let backend = Router::new().route(
        "/v1/describe",
        axum::routing::post(|| async {
            axum::Json(json!({
                "entry": {"action": "maya_primitives__create_sphere", "skill": "maya-primitives"},
                "description": "Create a sphere",
                "input_schema": {"type": "object", "properties": {"radius": {"type": "number"}}}
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

    let dir = tempfile::tempdir().unwrap();
    let lane = AdminSqliteLane::spawn(dir.path().join("compile.sqlite"), 30).unwrap();
    let state = make_admin_state().with_admin_sqlite_lane(Some(lane));
    let instance_id = register_backend(&state, port);
    let tool_slug = register_tool(&state, instance_id, "maya_primitives__create_sphere");
    let mut target = tool_trace("early-session", tool_slug.clone(), json!({"radius": 2.0}));
    target.request_id = "early-request".into();
    // The slug still identifies the exact backend when the trace was
    // captured before instance_id enrichment was added.
    target.instance_id = None;
    // Keep the trace outside the in-memory recent window while retaining it
    // inside the lane's 30-day SQLite retention window.
    target.started_at = SystemTime::now() - Duration::from_secs(60 * 60);
    let lane = state.admin_sqlite_lane.as_ref().unwrap();
    lane.try_persist_trace(&target);
    let mut noise = target.clone();
    noise.session_id = Some("other-session".into());
    for index in 0..2_001 {
        noise.request_id = format!("noise-{index}");
        lane.try_persist_trace(&noise);
    }
    let router = build_v1_debug_router(state);

    let (status, body) = post_compile_session(router, "early-session").await;
    let _ = stop_tx.send(());

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["compiled"]["workflow"]["steps"][0]["tool"],
        "maya_primitives__create_sphere"
    );
}

#[tokio::test]
async fn session_compile_rejects_mixed_dcc_history() {
    let state = make_admin_state();
    let maya_instance = register_backend(&state, 0);
    let maya_slug = register_tool(&state, maya_instance, "maya_primitives__create_sphere");
    let houdini_instance = make_service_entry("houdini", "127.0.0.1", 0, None);
    let houdini_id = houdini_instance.instance_id;
    state.gateway.registry.register(houdini_instance).unwrap();
    let houdini_slug = crate::gateway::capability::tool_slug(
        "houdini",
        &houdini_id,
        "houdini_geometry__create_box",
    );
    state.gateway.capability_index.upsert_instance(
        houdini_id,
        vec![crate::gateway::capability::CapabilityRecord::new(
            houdini_slug.clone(),
            "houdini_geometry__create_box".into(),
            "houdini_geometry__create_box".into(),
            Some("houdini-geometry".into()),
            "Houdini tool",
            vec![],
            "houdini".into(),
            houdini_id,
            true,
            true,
            None,
        )],
        crate::gateway::capability::InstanceFingerprint(1),
    );

    let mut maya_trace = tool_trace("mixed-session", maya_slug, json!({"radius": 2.0}));
    maya_trace.instance_id = Some(maya_instance.to_string());
    maya_trace.started_at = SystemTime::UNIX_EPOCH;
    let mut houdini_trace = houdini_tool_trace("mixed-session", houdini_slug, json!({"size": 2.0}));
    houdini_trace.instance_id = Some(houdini_id.to_string());
    houdini_trace.request_id = "houdini-request".into();
    houdini_trace.started_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    let traces = Arc::new(TraceLog::new(10));
    traces.push(maya_trace);
    traces.push(houdini_trace);
    let router = build_v1_debug_router(state.with_trace_log(traces, None));

    let (status, body) = post_compile_session(router, "mixed-session").await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "mixed_dcc_session");
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
            "name": "reviewed-session-workflow"
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
                }
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
    let instance_id = register_backend(&state, port);
    let tool_slug = register_tool(&state, instance_id, "maya_primitives__create_sphere");
    let mut trace = tool_trace(
        "completed-session",
        tool_slug,
        json!({"radius": 2.0, "note": "public scene review"}),
    );
    trace.instance_id = Some(instance_id.to_string());
    let traces = Arc::new(TraceLog::new(10));
    traces.push(trace);
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
async fn session_compile_binds_same_named_tools_to_the_slug_instance() {
    let backend = Router::new().route(
        "/v1/describe",
        axum::routing::post(|| async {
            axum::Json(json!({
                "entry": {"action": "maya_primitives__create_sphere", "skill": "maya-primitives"},
                "description": "Create a sphere",
                "input_schema": {"type": "object", "properties": {"radius": {"type": "number"}}}
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
    let selected_instance = register_backend(&state, port);
    let other_instance = register_backend(&state, 0);
    let selected_slug = register_tool(&state, selected_instance, "maya_primitives__create_sphere");
    register_tool(&state, other_instance, "maya_primitives__create_sphere");

    let traces = Arc::new(TraceLog::new(10));
    let mut first_trace = tool_trace("same-tool-session", selected_slug, json!({"radius": 1.0}));
    first_trace.request_id = "same-tool-first".into();
    first_trace.started_at = SystemTime::UNIX_EPOCH;
    traces.push(first_trace);
    let mut second_trace = tool_trace(
        "same-tool-session",
        crate::gateway::capability::tool_slug(
            "maya",
            &selected_instance,
            "maya_primitives__create_sphere",
        ),
        json!({"radius": 2.0}),
    );
    second_trace.request_id = "same-tool-second".into();
    second_trace.started_at = SystemTime::UNIX_EPOCH;
    traces.push(second_trace);
    let router = build_v1_debug_router(state.with_trace_log(traces, None));

    let (status, body) = post_compile_session(router, "same-tool-session").await;
    let _ = stop_tx.send(());

    assert_eq!(status, StatusCode::OK, "{body}");
    let steps = body["compiled"]["workflow"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["tool"], "maya_primitives__create_sphere");
    assert_eq!(steps[0]["args"]["radius"], 1.0);
    assert_eq!(steps[1]["args"]["radius"], 2.0);
}

#[tokio::test]
async fn session_compile_rejects_mixed_slug_instances_without_trace_instance_ids() {
    let state = make_admin_state();
    let first_instance = register_backend(&state, 0);
    let second_instance = register_backend(&state, 0);
    let first_slug = register_tool(&state, first_instance, "maya_primitives__create_sphere");
    let second_slug = register_tool(&state, second_instance, "maya_primitives__create_sphere");
    let traces = Arc::new(TraceLog::new(10));
    let mut first_trace = tool_trace("mixed-instance-session", first_slug, json!({"radius": 1.0}));
    first_trace.request_id = "mixed-instance-first".into();
    traces.push(first_trace);
    let mut second_trace = tool_trace(
        "mixed-instance-session",
        second_slug,
        json!({"radius": 2.0}),
    );
    second_trace.request_id = "mixed-instance-second".into();
    traces.push(second_trace);
    let router = build_v1_debug_router(state.with_trace_log(traces, None));

    let (status, body) = post_compile_session(router, "mixed-instance-session").await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "mixed_target_instance");
}

#[tokio::test]
async fn session_compile_rejects_overlapping_short_instance_prefixes() {
    let state = make_admin_state();
    let traces = Arc::new(TraceLog::new(10));
    let mut first_trace = tool_trace(
        "overlapping-instance-session",
        "maya.abcd.maya_primitives__create_sphere".into(),
        json!({"radius": 1.0}),
    );
    first_trace.request_id = "overlapping-first".into();
    let mut second_trace = tool_trace(
        "overlapping-instance-session",
        "maya.abcdef12.maya_primitives__create_sphere".into(),
        json!({"radius": 2.0}),
    );
    second_trace.request_id = "overlapping-second".into();
    traces.push(first_trace);
    traces.push(second_trace);
    let router = build_v1_debug_router(state.with_trace_log(traces, None));

    let (status, body) = post_compile_session(router, "overlapping-instance-session").await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "mixed_target_instance");
}

#[tokio::test]
async fn session_compile_rejects_history_over_the_bounded_limit() {
    let state = make_admin_state();
    let instance_id = register_backend(&state, 0);
    let tool_slug = register_tool(&state, instance_id, "maya_primitives__create_sphere");
    let traces = Arc::new(TraceLog::new(2_100));
    for index in 0..2_001 {
        let mut trace = tool_trace(
            "oversized-session",
            tool_slug.clone(),
            json!({"radius": index}),
        );
        trace.request_id = format!("oversized-request-{index}");
        traces.push(trace);
    }
    let router = build_v1_debug_router(state.with_trace_log(traces, None));

    let (status, body) = post_compile_session(router, "oversized-session").await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "session_history_too_large");
}

#[tokio::test]
async fn retroactive_session_compile_rejects_ui_act_without_semantic_find() {
    let state = make_admin_state();
    let instance_id = register_backend(&state, 0);
    let tool_slug = register_tool(&state, instance_id, "ui_control__act");
    let traces = Arc::new(TraceLog::new(10));
    traces.push(tool_trace(
        "ui-session",
        tool_slug,
        json!({"action": "click", "session": "default"}),
    ));
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
