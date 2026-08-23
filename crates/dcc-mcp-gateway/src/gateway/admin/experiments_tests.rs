//! Focused tests for experiment API contracts.

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::json;

use crate::gateway::admin::application::router::build_v1_debug_router;
use crate::gateway::admin::sqlite_lane::AdminSqliteLane;
use crate::gateway::admin::tests::admin_tests::{
    body_json, make_admin_state, post_json_as_session,
};

#[tokio::test]
async fn experiment_api_projects_runs_session_dag_metrics_and_judges() {
    let dir = tempfile::tempdir().unwrap();
    let lane = AdminSqliteLane::spawn(dir.path().join("experiments.sqlite"), 30).unwrap();
    let state = make_admin_state().with_admin_sqlite_lane(Some(lane));
    let router = build_v1_debug_router(state);

    let (status, created) = post_json_as_session(
        router.clone(),
        "/v1/experiments",
        "maya-root-session",
        json!({
            "name": "Cross-DCC scene validation",
            "scenario_id": "scene-validation-v1",
            "workflow_id": "workflow-scene-validation"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let experiment_id = created["experiment_id"].as_str().unwrap();

    let run_uri = format!("/v1/experiments/{experiment_id}/runs");
    let (status, _) = post_json_as_session(
        router.clone(),
        &run_uri,
        "maya-root-session",
        json!({
            "run_id": "run-maya-1",
            "status": "running",
            "seed": 7,
            "parameters": {"dcc_type": "maya"}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, _) = post_json_as_session(
        router.clone(),
        &run_uri,
        "maya-root-session",
        json!({
            "run_id": "run-maya-1",
            "status": "passed",
            "metrics": {"tool_calls": 4, "duration_ms": 1200}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let judge_uri = format!("/v1/experiments/{experiment_id}/judge-results");
    let (status, _) = post_json_as_session(
        router.clone(),
        &judge_uri,
        "maya-root-session",
        json!({
            "run_id": "run-maya-1",
            "evaluator_id": "scene-quality",
            "evaluator_version": "1",
            "rubric_hash": "sha256:abc",
            "status": "passed",
            "scores": {"quality": 0.93},
            "summary": "Scene evidence satisfies the rubric."
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    tokio::time::sleep(Duration::from_millis(100)).await;
    let detail_uri = format!("/v1/experiments/{experiment_id}");
    let (status, detail) = body_json(router, &detail_uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["experiment"]["scenario_id"], "scene-validation-v1");
    assert_eq!(detail["runs"][0]["status"], "passed");
    assert_eq!(
        detail["session_dag"]["nodes"][0]["session_id"],
        "maya-root-session"
    );
    assert_eq!(detail["metrics"]["runs"]["passed"], 1);
    assert_eq!(detail["metrics"]["judges"]["passed"], 1);
    assert_eq!(detail["judge_results"][0]["evaluator_id"], "scene-quality");
    assert_eq!(detail["judge_results"][0]["authority"], "evidence_only");
}
