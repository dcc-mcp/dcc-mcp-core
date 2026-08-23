//! Focused tests for durable record/replay lifecycle contracts.

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::gateway::admin::application::router::build_v1_debug_router;
use crate::gateway::admin::sqlite_lane::AdminSqliteLane;
use crate::gateway::admin::tests::admin_tests::{make_admin_state, post_json_as_session};
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
