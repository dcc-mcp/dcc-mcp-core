use super::*;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tempfile::tempdir;

async fn echo_request_id(request: Request, next: Next) -> Response {
    let request_id = request.headers().get("x-request-id").cloned();
    let mut response = next.run(request).await;
    if let Some(request_id) = request_id {
        response.headers_mut().insert("x-request-id", request_id);
    }
    response
}

#[tokio::test]
async fn local_load_skill_routes_through_gateway_to_keep_index_coherent() {
    async fn load_skill(Json(body): Json<Value>) -> Json<Value> {
        Json(json!({
            "loaded": true,
            "skill_name": body["skill_name"],
            "registered_tools": ["blender_scene__list_objects"],
            "source": "gateway"
        }))
    }

    let app = Router::new().route("/v1/load_skill", post(load_skill));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let registry = tempdir().unwrap();
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        registry.path().to_path_buf(),
        false,
    );

    let result = control
        .load_skill(LoadSkillRequest {
            body: json!({
                "skill_name": "blender-scene",
                "dcc_type": "blender",
                "instance_id": "abc12345"
            }),
        })
        .await
        .unwrap();

    assert_eq!(result["loaded"], true);
    assert_eq!(result["source"], "gateway");
    assert_eq!(result["registered_tools"][0], "blender_scene__list_objects");
    server.abort();
}

#[tokio::test]
async fn required_gateway_routes_a_local_call_and_reports_stats_coverage() {
    async fn call(Json(body): Json<Value>) -> Json<Value> {
        Json(json!({"success": true, "request": body}))
    }

    async fn stats(Query(query): Query<HashMap<String, String>>) -> Json<Value> {
        Json(json!({"total_calls": 1, "query": query}))
    }

    let app = Router::new()
        .route("/v1/call", post(call))
        .route("/v1/debug/stats", get(stats))
        .layer(middleware::from_fn(echo_request_id));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let registry = tempdir().unwrap();
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        registry.path().to_path_buf(),
        true,
    );

    let result = control
        .call(
            "maya.abc12345.inspect".to_string(),
            None,
            None,
            json!({"detail": true}),
            Some(json!({"agent_context": {"session_id": "task-42"}})),
            Duration::from_secs(2),
        )
        .await
        .unwrap();

    assert_eq!(result["control_route"], "gateway");
    assert_eq!(result["gateway_stats_recorded"], true);
    assert_eq!(
        result["request"]["meta"]["agent_context"]["session_id"],
        "task-42"
    );

    let stats = control
        .stats(StatsRequest {
            range: "24h".to_string(),
            session_id: Some("task-42".to_string()),
            ..StatsRequest::default()
        })
        .await
        .unwrap();
    assert_eq!(stats["stats_coverage"]["configured_call_route"], "gateway");
    assert_eq!(stats["stats_coverage"]["configured_route_recorded"], true);
    assert_eq!(stats["query"]["session_id"], "task-42");

    server.abort();
}

#[tokio::test]
async fn wait_for_async_call_returns_terminal_result_without_requeueing_polls() {
    async fn call(
        State(requests): State<Arc<Mutex<Vec<Value>>>>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let slug = body["tool_slug"].as_str().unwrap_or_default().to_string();
        let poll = {
            let mut requests = requests.lock().unwrap();
            let poll = requests
                .iter()
                .filter(|request| {
                    request["tool_slug"]
                        .as_str()
                        .is_some_and(|tool| tool.ends_with(".jobs_get_status"))
                })
                .count();
            requests.push(body);
            poll
        };
        if slug.ends_with(".jobs_get_status") {
            let status = if poll == 0 { "running" } else { "completed" };
            let current = if poll == 0 { 45 } else { 90 };
            return Json(json!({
                "structuredContent": {
                    "job_id": "job-42",
                    "status": status,
                    "progress": {
                        "current": current,
                        "total": 90,
                        "message": format!("frame {current}")
                    },
                    "result": (status == "completed").then(|| json!({
                        "success": true,
                        "message": "Flipbook job launched",
                        "context": {
                            "job_id": "flipbook-f0631aa83e07",
                            "progress": {"completed": 96, "total": 96}
                        }
                    }))
                }
            }));
        }
        Json(json!({
            "slug": slug,
            "output": {"job_id": "job-42", "status": "pending"}
        }))
    }

    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/call", post(call))
        .with_state(requests.clone())
        .layer(middleware::from_fn(echo_request_id));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let registry = tempdir().unwrap();
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        registry.path().to_path_buf(),
        true,
    );

    let mut progress = Vec::new();
    let result = control
        .call_and_wait_with_progress(
            "unity.abc12345.run_tests".to_string(),
            None,
            None,
            json!({}),
            Some(json!({
                "agent_context": {"session_id": "task-42"},
                "lease_owner": "workflow-42",
                "dcc": {"async": true, "wait_for_terminal": true},
                "progressToken": "progress-9"
            })),
            Duration::from_secs(2),
            Duration::from_secs(5),
            |update| progress.push(update.clone()),
        )
        .await
        .unwrap();

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    let poll_meta = &requests[1]["meta"];
    assert_eq!(poll_meta["agent_context"]["session_id"], "task-42");
    assert_eq!(poll_meta["lease_owner"], "workflow-42");
    assert!(poll_meta["dcc"].get("async").is_none());
    assert!(poll_meta["dcc"].get("wait_for_terminal").is_none());
    assert!(poll_meta.get("progressToken").is_none());
    assert_eq!(result["structuredContent"]["status"], "completed");
    assert_eq!(
        result["structuredContent"]["result"]["message"],
        "Flipbook job launched"
    );
    assert_eq!(result["structuredContent"]["core_job_id"], "job-42");
    assert_eq!(result["structuredContent"]["job_id_owner"], "core");
    assert_eq!(
        result["structuredContent"]["adapter_job_id"],
        "flipbook-f0631aa83e07"
    );
    assert_eq!(
        result["structuredContent"]["adapter_job"]["owner"],
        "adapter"
    );
    assert_eq!(result["wait"]["terminal"], false);
    assert_eq!(result["wait"]["tracking_status"], "poll_contract_missing");
    assert_eq!(result["success"], false);
    assert_eq!(
        progress
            .iter()
            .map(|update| (update.status.as_str(), update.current, update.total))
            .collect::<Vec<_>>(),
        vec![
            ("pending", None, None),
            ("running", Some(45), Some(90)),
            ("completed", Some(90), Some(90)),
        ]
    );
    server.abort();
}

#[tokio::test]
async fn wait_for_direct_adapter_job_polls_declared_tool_to_terminal() {
    async fn call(
        State(requests): State<Arc<Mutex<Vec<Value>>>>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let slug = body["tool_slug"].as_str().unwrap_or_default().to_string();
        let poll_count = {
            let mut requests = requests.lock().unwrap();
            let poll_count = requests
                .iter()
                .filter(|request| {
                    request["tool_slug"]
                        .as_str()
                        .is_some_and(|tool| tool.ends_with(".blender_render__get_render_job"))
                })
                .count();
            requests.push(body);
            poll_count
        };
        if slug.ends_with(".blender_render__get_render_job") {
            let status = if poll_count == 0 {
                "running"
            } else {
                "completed"
            };
            let current = if poll_count == 0 { 2 } else { 3 };
            return Json(json!({
                "structuredContent": {
                    "success": true,
                    "message": "Render job status",
                    "context": {
                        "job_id": "render-adapter-42",
                        "status": status,
                        "progress": {"current": current, "total": 3},
                    }
                }
            }));
        }
        Json(json!({
            "structuredContent": {
                "success": true,
                "message": "Background render job submitted",
                "context": {
                    "job_id": "render-adapter-42",
                    "status": "running",
                    "progress": {"current": 1, "total": 3},
                },
                "adapter_job_id": "render-adapter-42",
                "adapter_job": {
                    "job_id": "render-adapter-42",
                    "owner": "adapter",
                    "status": "running",
                    "poll_contract": {"registered": true, "reason": null},
                    "poll": {
                        "owner": "adapter",
                        "tool": "blender_render__get_render_job",
                        "arguments": {"job_id": "render-adapter-42"},
                    }
                }
            }
        }))
    }

    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/call", post(call))
        .with_state(requests.clone())
        .layer(middleware::from_fn(echo_request_id));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let registry = tempdir().unwrap();
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        registry.path().to_path_buf(),
        true,
    );

    let mut progress = Vec::new();
    let result = control
        .call_and_wait_with_progress(
            "blender.abc12345.blender_render__start_render_job".to_string(),
            None,
            None,
            json!({}),
            None,
            Duration::from_secs(2),
            Duration::from_secs(5),
            |update| progress.push(update.clone()),
        )
        .await
        .unwrap();

    assert_eq!(result["wait"]["terminal"], true);
    assert_eq!(result["wait"]["owner"], "adapter");
    assert_eq!(result["wait"]["job_id"], "render-adapter-42");
    assert_eq!(result["wait"]["status"], "completed");
    assert_eq!(
        result["structuredContent"]["adapter_job"]["terminal_result"]["structuredContent"]["context"]
            ["status"],
        "completed"
    );
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request["tool_slug"].as_str().unwrap().to_string())
            .collect::<Vec<_>>(),
        vec![
            "blender.abc12345.blender_render__start_render_job",
            "blender.abc12345.blender_render__get_render_job",
            "blender.abc12345.blender_render__get_render_job",
        ]
    );
    assert_eq!(
        progress
            .iter()
            .map(|update| (update.job_id.as_str(), update.status.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("render-adapter-42", "running"),
            ("render-adapter-42", "running"),
            ("render-adapter-42", "completed"),
        ]
    );
    server.abort();
}

#[tokio::test]
async fn wait_rejects_adapter_poll_that_returns_a_different_job() {
    async fn call(
        State(requests): State<Arc<Mutex<Vec<String>>>>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let slug = body["tool_slug"].as_str().unwrap_or_default().to_string();
        requests.lock().unwrap().push(slug.clone());
        if slug.ends_with(".houdini_render__get_render_job") {
            return Json(json!({
                "structuredContent": {
                    "success": true,
                    "context": {"job_id": "fresh-job", "status": "pending"}
                }
            }));
        }
        Json(json!({
            "structuredContent": {
                "success": true,
                "context": {"job_id": "render-job-42", "status": "running"},
                "adapter_job": {
                    "job_id": "render-job-42",
                    "owner": "adapter",
                    "status": "running",
                    "poll_contract": {"registered": true, "reason": null},
                    "poll": {
                        "owner": "adapter",
                        "tool": "houdini_render__get_render_job",
                        "arguments": {"job_id": "render-job-42"}
                    }
                }
            }
        }))
    }

    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/call", post(call))
        .with_state(requests.clone())
        .layer(middleware::from_fn(echo_request_id));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let registry = tempdir().unwrap();
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        registry.path().to_path_buf(),
        true,
    );

    let result = control
        .call_and_wait(
            "houdini.abc12345.houdini_render__render_rop".to_string(),
            None,
            None,
            json!({}),
            None,
            Duration::from_secs(2),
            Duration::from_secs(3),
        )
        .await
        .unwrap();

    assert_eq!(result["success"], false);
    assert_eq!(result["tracking_status"], "job_id_mismatch");
    assert_eq!(result["job_id"], "render-job-42");
    assert_eq!(result["returned_job_id"], "fresh-job");
    assert_eq!(result["job_not_resubmitted"], true);
    assert_eq!(requests.lock().unwrap().len(), 2);
    server.abort();
}

#[tokio::test]
async fn wait_falls_back_to_bare_job_status_for_direct_adapter_base_url() {
    async fn call(
        State(requests): State<Arc<Mutex<Vec<String>>>>,
        Json(body): Json<Value>,
    ) -> Response {
        let slug = body["tool_slug"].as_str().unwrap_or_default().to_string();
        requests.lock().unwrap().push(slug.clone());
        if slug == "touchdesigner.touchdesigner-scripting.jobs_get_status" {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "kind": "unknown-slug",
                    "message": "no action registered for slug 'touchdesigner.touchdesigner-scripting.jobs_get_status'"
                })),
            )
                .into_response();
        }
        if slug == "jobs_get_status" {
            return Json(json!({
                "output": {
                    "job_id": "job-direct-42",
                    "status": "completed",
                    "result": {"success": true, "message": "done"}
                }
            }))
            .into_response();
        }
        Json(json!({
            "output": {"job_id": "job-direct-42", "status": "pending"}
        }))
        .into_response()
    }

    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/call", post(call))
        .with_state(requests.clone())
        .layer(middleware::from_fn(echo_request_id));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let registry = tempdir().unwrap();
    let endpoint = Endpoint::new(format!("http://{addr}"));
    let control = DccControlPlane::new(
        GatewayTarget::Remote {
            name: "adapter".to_string(),
            endpoint: endpoint.clone(),
        },
        endpoint,
        registry.path().to_path_buf(),
        false,
    );

    let result = control
        .call_and_wait(
            "touchdesigner.touchdesigner-scripting.get_project_info".to_string(),
            None,
            None,
            json!({}),
            None,
            Duration::from_secs(2),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    assert_eq!(result["output"]["status"], "completed");
    assert_eq!(
        *requests.lock().unwrap(),
        vec![
            "touchdesigner.touchdesigner-scripting.get_project_info",
            "touchdesigner.touchdesigner-scripting.jobs_get_status",
            "jobs_get_status",
        ]
    );
    server.abort();
}

#[tokio::test]
async fn wait_for_async_call_resumes_after_gateway_unavailable() {
    async fn call(State(polls): State<Arc<Mutex<u32>>>, Json(body): Json<Value>) -> Response {
        if body["tool_slug"]
            .as_str()
            .is_some_and(|slug| slug.ends_with(".jobs_get_status"))
        {
            let attempt = {
                let mut polls = polls.lock().unwrap();
                let attempt = *polls;
                *polls += 1;
                attempt
            };
            if attempt < 2 {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "error": {
                            "kind": "instance-offline",
                            "previous_status": "unreachable",
                            "retryable": true
                        }
                    })),
                )
                    .into_response();
            }
            return Json(json!({
                "structuredContent": {
                    "job_id": "job-houdini-42",
                    "status": "completed",
                    "result": {"success": true}
                }
            }))
            .into_response();
        }
        Json(json!({
            "output": {"job_id": "job-houdini-42", "status": "running"}
        }))
        .into_response()
    }

    let polls = Arc::new(Mutex::new(0));
    let app = Router::new()
        .route("/v1/call", post(call))
        .with_state(polls.clone())
        .layer(middleware::from_fn(echo_request_id));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let registry = tempdir().unwrap();
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        registry.path().to_path_buf(),
        true,
    );

    let mut progress = Vec::new();
    let result = control
        .call_and_wait_with_progress(
            "houdini.04fccb17.render".to_string(),
            None,
            None,
            json!({}),
            None,
            Duration::from_secs(2),
            Duration::from_secs(5),
            |update| progress.push(update.clone()),
        )
        .await
        .unwrap();

    assert_eq!(result["structuredContent"]["status"], "completed");
    assert_eq!(result["wait_recovery"]["control_plane_disruptions"], 2);
    assert_eq!(result["wait_recovery"]["resumed"], true);
    assert_eq!(result["wait_recovery"]["job_resubmitted"], false);
    assert_eq!(
        progress
            .iter()
            .filter(|update| update.status == "control_plane_reconnecting")
            .count(),
        1,
        "one outage should emit one reconnecting transition"
    );
    assert_eq!(*polls.lock().unwrap(), 3);
    server.abort();
}

#[tokio::test]
async fn wait_for_async_call_reports_exited_owner_without_resubmitting() {
    async fn call(Json(body): Json<Value>) -> Response {
        if body["tool_slug"]
            .as_str()
            .is_some_and(|slug| slug.ends_with(".jobs_get_status"))
        {
            return (
                StatusCode::GONE,
                Json(json!({
                    "error": {
                        "kind": "instance-offline",
                        "previous_status": "exited",
                        "retryable": false,
                        "recommended_next_action": "Refresh instances and search for a replacement."
                    }
                })),
            )
                .into_response();
        }
        Json(json!({
            "output": {"job_id": "job-maya-42", "status": "running"}
        }))
        .into_response()
    }

    let app = Router::new()
        .route("/v1/call", post(call))
        .layer(middleware::from_fn(echo_request_id));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let registry = tempdir().unwrap();
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        registry.path().to_path_buf(),
        true,
    );

    let result = control
        .call_and_wait(
            "maya.abcdef01.render".to_string(),
            None,
            None,
            json!({}),
            None,
            Duration::from_secs(2),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    assert_eq!(result["tracking_status"], "owner_exited");
    assert_eq!(result["job_id"], "job-maya-42");
    assert_eq!(result["job_not_resubmitted"], true);
    assert_eq!(
        result["control_plane_error"]["body"]["error"]["previous_status"],
        "exited"
    );
    server.abort();
}

#[tokio::test]
async fn wait_without_job_identity_says_it_did_not_wait() {
    async fn call(Json(body): Json<Value>) -> Json<Value> {
        // A render tool that returns success with no job id at all.
        Json(json!({
            "slug": body["tool_slug"],
            "output": {"success": true, "message": "render submitted"},
        }))
    }

    let app = Router::new()
        .route("/v1/call", post(call))
        .layer(middleware::from_fn(echo_request_id));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let registry = tempdir().unwrap();
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        registry.path().to_path_buf(),
        true,
    );

    let result = control
        .call_and_wait(
            "houdini.abcdef01.render_rop".to_string(),
            None,
            None,
            json!({}),
            Some(json!({"dcc": {"async": true, "wait_for_terminal": true}})),
            Duration::from_secs(2),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    assert_eq!(
        result["wait"]["tracking_status"], "no_job_identity",
        "--wait must not look like it waited"
    );
    assert_eq!(result["wait"]["terminal"], false);
    assert_eq!(result["wait"]["waited"], false);
    assert_eq!(result["success"], false);
    assert!(result["error"].as_str().is_some());
    server.abort();
}

#[test]
fn wait_on_a_synchronous_call_warns_without_failing() {
    let mut result = json!({"success": true});
    attach_no_job_identity_wait(&mut result, false);
    assert_eq!(result["wait"]["tracking_status"], "no_job_identity");
    assert_eq!(result["success"], true);
    assert!(result["warning"].as_str().is_some());

    let mut async_result = json!({"success": true});
    attach_no_job_identity_wait(&mut async_result, true);
    assert_eq!(async_result["success"], false);
    assert!(async_result["error"].as_str().is_some());
}

#[test]
fn no_job_identity_wait_keeps_its_reason_alongside_an_existing_warning() {
    // `warning` is a single-valued field: a result that already carries an
    // unrelated warning must not drop the explanation for the wait that
    // never happened, so the envelope always carries it.
    let mut result = json!({"success": true, "warning": "frame range was clamped"});
    attach_no_job_identity_wait(&mut result, false);

    assert_eq!(result["warning"], "frame range was clamped");
    assert_eq!(result["wait"]["tracking_status"], "no_job_identity");
    assert!(
        result["wait"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("no job identity")),
        "the --wait explanation survives an existing warning, got {:?}",
        result["wait"]["message"]
    );
}

#[test]
fn async_intent_is_read_from_call_meta() {
    assert!(async_wait_requested(Some(&json!({"dcc": {"async": true}}))));
    assert!(async_wait_requested(Some(&json!({"dcc": {
        "wait_for_terminal": true
    }}))));
    assert!(!async_wait_requested(Some(
        &json!({"dcc": {"async": false}})
    )));
    assert!(!async_wait_requested(None));
    assert!(!async_wait_requested(Some(&json!({}))));
    // A false alias must not mask a true one: every alias is evaluated.
    assert!(async_wait_requested(Some(&json!({"dcc": {
        "async": false,
        "wait_for_terminal": true,
    }}))));
    assert!(async_wait_requested(Some(&json!({"dcc": {
        "async": false,
        "waitForTerminal": true,
    }}))));
    assert!(
        !async_wait_requested(Some(&json!({"dcc": {
            "async": false,
            "wait_for_terminal": false,
        }}))),
        "no alias signalling true means the call is synchronous"
    );
}

#[test]
fn direct_local_results_disclose_that_gateway_stats_exclude_them() {
    let call = attach_call_route(json!({"success": true}), true);
    assert_eq!(call["control_route"], "local_mcp_direct");
    assert_eq!(call["gateway_stats_recorded"], false);
    assert!(
        call["gateway_stats_hint"]
            .as_str()
            .unwrap()
            .contains("--require-gateway")
    );

    let stats = attach_stats_coverage(json!({"total_calls": 0}), true);
    assert_eq!(
        stats["stats_coverage"]["configured_call_route"],
        "local_mcp_direct"
    );
    assert_eq!(stats["stats_coverage"]["configured_route_recorded"], false);
    assert_eq!(
        stats["stats_coverage"]["excluded_control_routes"][0],
        "local_mcp_direct"
    );
}

#[tokio::test]
async fn gateway_only_is_rejected_outside_the_direct_local_path() {
    // `--gateway-only` is a local-inventory filter: it diffs the backend's
    // raw `tools/list` against the gateway capability index. On the gateway
    // path the field is not part of the REST contract, so forwarding it
    // would silently return the unfiltered set — a plausible-looking wrong
    // answer for `--gateway-only=false`, which is supposed to be the
    // difference between the two inventories.
    let registry = tempdir().unwrap();
    let unreachable = Endpoint::new("http://127.0.0.1:1".to_string());
    for control in [
        // --require-gateway on a local target
        DccControlPlane::new(
            GatewayTarget::Local,
            unreachable.clone(),
            registry.path().to_path_buf(),
            true,
        ),
        // --transport rest
        DccControlPlane::new(
            GatewayTarget::Local,
            unreachable.clone(),
            registry.path().to_path_buf(),
            false,
        )
        .with_transport(TransportMode::Rest),
        // a remote gateway profile
        DccControlPlane::new(
            GatewayTarget::Remote {
                name: "remote".to_string(),
                endpoint: unreachable.clone(),
            },
            unreachable.clone(),
            registry.path().to_path_buf(),
            false,
        ),
    ] {
        assert!(
            !control.uses_direct_local(),
            "the fixture must exercise the gateway path"
        );
        for gateway_only in [Some(true), Some(false)] {
            let error = control
                .search(SearchRequest {
                    query: None,
                    dcc_type: None,
                    instance_id: None,
                    limit: None,
                    gateway_only,
                })
                .await
                .expect_err("--gateway-only must not be silently dropped");

            let message = format!("{error:#}");
            assert!(
                message.contains("--gateway-only"),
                "the refusal must name the flag; got {message:?}"
            );
        }
    }
}
