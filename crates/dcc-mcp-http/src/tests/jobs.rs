use super::*;

// ── jobs.get_status (#319) ────────────────────────────────────────────

#[tokio::test]
pub async fn test_jobs_get_status_unknown_id_returns_is_error_envelope() {
    let server = TestServer::new(make_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "jobs.get_status",
                "arguments": {"job_id": "nonexistent-uuid"}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(
        body.get("error").is_none(),
        "unknown job id must not produce a transport-level JSON-RPC error"
    );
    let result = &body["result"];
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("nonexistent-uuid"),
        "error message must name the missing id, got: {text}"
    );
}

#[tokio::test]
pub async fn test_jobs_get_status_missing_job_id_param_is_error() {
    let server = TestServer::new(make_router());
    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "jobs.get_status",
                "arguments": {}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.to_lowercase().contains("job_id"),
        "error text must name the missing parameter, got: {text}"
    );
}

#[tokio::test]
pub async fn test_jobs_get_status_returns_full_envelope_for_terminal_job() {
    use crate::job::JobProgress;

    let state = make_app_state();
    let parent = state.jobs.create("workflow.run");
    let parent_id = parent.read().id.clone();
    let child = state
        .jobs
        .create_with_parent("workflow.step", Some(parent_id.clone()));
    let child_id = child.read().id.clone();
    state.jobs.start(&child_id).unwrap();
    state
        .jobs
        .update_progress(
            &child_id,
            JobProgress {
                current: 3,
                total: 10,
                message: Some("half-way".into()),
            },
        )
        .unwrap();
    state
        .jobs
        .complete(&child_id, json!({"ok": true, "value": 42}))
        .unwrap();

    let app = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::post(crate::handler::handle_post)
                .get(crate::handler::handle_get)
                .delete(crate::handler::handle_delete),
        )
        .with_state(state);
    let server = TestServer::new(app);

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "jobs.get_status",
                "arguments": {"job_id": child_id, "include_result": true}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let result = &body["result"];
    assert_eq!(result["isError"], false);
    let sc = &result["structuredContent"];
    assert_eq!(sc["job_id"], child_id);
    assert_eq!(sc["parent_job_id"], parent_id);
    assert_eq!(sc["tool"], "workflow.step");
    assert_eq!(sc["status"], "completed");
    assert!(sc["created_at"].is_string());
    assert!(sc["started_at"].is_string());
    assert!(sc["completed_at"].is_string());
    assert_eq!(sc["progress"]["current"], 3);
    assert_eq!(sc["progress"]["total"], 10);
    assert_eq!(sc["result"]["ok"], true);
    assert_eq!(sc["result"]["value"], 42);
}

#[tokio::test]
pub async fn test_jobs_get_status_include_result_false_omits_result() {
    let state = make_app_state();
    let job = state.jobs.create("t.x");
    let id = job.read().id.clone();
    state.jobs.start(&id).unwrap();
    state.jobs.complete(&id, json!({"v": 1})).unwrap();

    let app = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::post(crate::handler::handle_post)
                .get(crate::handler::handle_get)
                .delete(crate::handler::handle_delete),
        )
        .with_state(state);
    let server = TestServer::new(app);

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "jobs.get_status",
                "arguments": {"job_id": id, "include_result": false}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let sc = &body["result"]["structuredContent"];
    assert_eq!(sc["status"], "completed");
    assert!(
        sc.get("result").is_none(),
        "include_result=false must omit `result` key, got {sc}"
    );
}

#[tokio::test]
pub async fn test_jobs_get_status_running_job_has_no_result_yet() {
    let state = make_app_state();
    let job = state.jobs.create("t.slow");
    let id = job.read().id.clone();
    state.jobs.start(&id).unwrap();

    let app = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::post(crate::handler::handle_post)
                .get(crate::handler::handle_get)
                .delete(crate::handler::handle_delete),
        )
        .with_state(state);
    let server = TestServer::new(app);

    let resp = server
        .post("/mcp")
        .add_header(
            axum::http::header::ACCEPT,
            "application/json".parse::<HeaderValue>().unwrap(),
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "jobs.get_status",
                "arguments": {"job_id": id, "include_result": true}
            }
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let sc = &body["result"]["structuredContent"];
    assert_eq!(sc["status"], "running");
    assert!(
        sc.get("result").is_none(),
        "running job must not have a `result` key even with include_result=true"
    );
    assert!(sc["started_at"].is_string());
    assert_eq!(sc["completed_at"], Value::Null);
}
