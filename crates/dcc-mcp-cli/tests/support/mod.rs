#![allow(dead_code)]

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::{Json, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use tokio::sync::oneshot;

pub(crate) struct GatewayFixture {
    pub(crate) base_url: String,
    pub(crate) shutdown: Option<oneshot::Sender<()>>,
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for GatewayFixture {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(crate) struct LocalMcpFixture {
    pub(crate) base_url: String,
    pub(crate) shutdown: Option<oneshot::Sender<()>>,
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
    pub(crate) tools_list_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(crate) activate_group_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl LocalMcpFixture {
    pub(crate) fn mcp_url(&self) -> String {
        format!("{}/mcp", self.base_url)
    }

    pub(crate) fn safe_stop_url(&self) -> String {
        format!("{}/safe-stop", self.base_url)
    }

    pub(crate) fn tools_list_calls(&self) -> usize {
        self.tools_list_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn activate_group_calls(&self) -> usize {
        self.activate_group_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Drop for LocalMcpFixture {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(crate) fn json_or_compact_fixture_response(
    headers: &HeaderMap,
    payload: Value,
    compact_body: &'static str,
) -> Response {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if accept.contains("application/json") {
        Json(payload).into_response()
    } else {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/toon")],
            compact_body,
        )
            .into_response()
    }
}

pub(crate) fn spawn_gateway_fixture() -> GatewayFixture {
    let async_polls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"ok": true, "service": "dcc-mcp-gateway"})) }),
        )
        .route(
            "/update-manifest.json",
            get(|| async {
                Json(json!({
                    "dcc-mcp-server": {
                        "version": "9.9.9",
                        "url": "https://example.invalid/dcc-mcp-server.zip",
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "release_notes": "Fixture update"
                    }
                }))
            }),
        )
        .route(
            "/mcp",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                let accept = headers
                    .get(header::ACCEPT)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                if !(accept.contains("application/json") && accept.contains("text/event-stream"))
                {
                    return (
                        StatusCode::NOT_ACCEPTABLE,
                        Json(json!({
                            "error": "not_acceptable",
                            "message": "Client must accept both application/json and text/event-stream"
                        })),
                    );
                }

                let method = body.get("method").and_then(Value::as_str).unwrap_or("");
                match method {
                    "initialize" => (
                        StatusCode::OK,
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                            "result": {
                                "protocolVersion": "2025-03-26",
                                "capabilities": {
                                    "tools": {"listChanged": true}
                                },
                                "serverInfo": {
                                    "name": "fixture-gateway",
                                    "version": "0.0.0-test"
                                }
                            }
                        })),
                    ),
                    "tools/list" => (
                        StatusCode::OK,
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                            "result": {
                                "tools": [
                                    {
                                        "name": "search_tools",
                                        "description": "Search tools",
                                        "inputSchema": {"type": "object"}
                                    },
                                    {
                                        "name": "jobs_get_status",
                                        "description": "Get async job status",
                                        "inputSchema": {"type": "object"}
                                    }
                                ]
                            }
                        })),
                    ),
                    "tools/call" => {
                        let name = body
                            .pointer("/params/name")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let job_id = body
                            .pointer("/params/arguments/job_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let status = json!({
                            "job_id": job_id,
                            "tool": "render_write_node",
                            "status": "completed"
                        });
                        (
                            StatusCode::OK,
                            Json(json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or(json!(null)),
                                "result": {
                                    "isError": name != "jobs_get_status" || job_id == "job-404",
                                    "content": [{"type": "text", "text": status.to_string()}],
                                    "structuredContent": status
                                }
                            })),
                        )
                    }
                    _ => (
                        StatusCode::OK,
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                            "error": {
                                "code": -32601,
                                "message": "method not found"
                            }
                        })),
                    ),
                }
            }),
        )
        .route("/v1/healthz", get(|| async { Json(json!({"ok": true})) }))
        .route(
            "/v1/readyz",
            get(|| async {
                Json(json!({
                    "ok": true,
                    "live_instance_count": 1,
                    "ready_instance_count": 1,
                    "instances": [{
                        "instance_id": "abc12345-0000-0000-0000-000000000000",
                        "instance_short": "abc12345",
                        "dcc_type": "maya",
                        "mcp_url": "http://127.0.0.1:9/mcp",
                        "readiness": {
                            "process": true,
                            "dcc": true,
                            "skill_catalog": true,
                            "dispatcher": true,
                            "host_execution_bridge": true,
                            "main_thread_executor": true
                        },
                        "dispatch": {
                            "reported": true,
                            "ready": true
                        },
                        "gateway": {
                            "recovery_driver": "daemon_guardian"
                        },
                        "lifecycle": {
                            "supports_safe_stop": true
                        }
                    }]
                }))
            }),
        )
        .route(
            "/admin/api/health",
            get(|| async {
                Json(json!({
                    "status": "ok",
                    "gateway": {
                        "current": {
                            "name": "Maya-main-15084",
                            "role": "active",
                            "pid": 15084,
                            "host": "127.0.0.1",
                            "port": 9765,
                            "instance_id": "11111111-0000-0000-0000-000000000000",
                            "version": "0.17.9",
                            "adapter_version": "0.3.4",
                            "adapter_dcc": "maya"
                        },
                        "candidates": [{
                            "name": "Maya-layout-120920",
                            "role": "challenger",
                            "pid": 120920,
                            "host": "127.0.0.1",
                            "port": 9765,
                            "instance_id": "22222222-0000-0000-0000-000000000000",
                            "version": "0.17.9",
                            "adapter_version": "0.3.4",
                            "adapter_dcc": "maya"
                        }]
                    }
                }))
            }),
        )
        .route(
            "/v1/instances",
            get(|| async {
                Json(json!({
                    "total": 1,
                    "instances": [{
                        "instance_id": "abc12345-0000-0000-0000-000000000000",
                        "instance_short": "abc12345",
                        "dcc_type": "maya",
                        "mcp_url": "http://127.0.0.1:18080/mcp",
                        "metadata": {
                            "owner": "release-smoke-test",
                            "session": "test"
                        },
                        "lifecycle": {
                            "owner": "release-smoke-test",
                            "session": "test",
                            "supports_safe_stop": true,
                            "safe_stop_url": "http://127.0.0.1:18080/safe-stop"
                        },
                        "diagnostics": {
                            "readiness": {
                                "process": true,
                                "dcc": true,
                                "skill_catalog": true,
                                "dispatcher": true,
                                "host_execution_bridge": true,
                                "main_thread_executor": true
                            }
                        }
                    }]
                }))
            }),
        )
        .route(
            "/v1/search",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                json_or_compact_fixture_response(
                    &headers,
                    json!({
                    "total": 1,
                    "hits": [{
                        "slug": "maya.abc12345.create_sphere",
                        "instance_id": body.get("instance_id").cloned().unwrap_or(Value::Null),
                        "skill": "modeling",
                        "action": "create_sphere",
                        "dcc": body.get("dcc_type").and_then(Value::as_str).unwrap_or("maya"),
                        "summary": body.get("query").and_then(Value::as_str).unwrap_or("sphere"),
                        "loaded": true,
                        "scope": "gateway"
                    }]
                    }),
                    "hits[slug:\"maya.abc12345.create_sphere\"]",
                )
            }),
        )
        .route(
            "/v1/describe",
            post(|Json(body): Json<Value>| async move {
                Json(json!({
                    "record": {"tool_slug": body["tool_slug"]},
                    "tool": {"inputSchema": {"type": "object"}}
                }))
            }),
        )
        .route(
            "/v1/load_skill",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                json_or_compact_fixture_response(
                    &headers,
                    json!({
                    "loaded": true,
                    "skill_name": body["skill_name"],
                    "dcc_type": body.get("dcc_type").cloned().unwrap_or(Value::Null),
                    "instance_id": body.get("instance_id").cloned().unwrap_or(Value::Null),
                    "activate_groups": body.get("activate_groups").cloned().unwrap_or(Value::Null),
                    "registered_tools": ["workflow__run"],
                    "tool_count": 1,
                    "tools": [{
                        "name": "workflow__run",
                        "inputSchema": {"type": "object"}
                    }]
                    }),
                    "loaded:true\nskill_name:\"workflow\"",
                )
            }),
        )
        .route(
            "/v1/call",
            post(move |Json(body): Json<Value>| {
                let async_polls = Arc::clone(&async_polls);
                async move {
                    if body["tool_slug"] == "jobs_get_status" {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "kind": "bad-request",
                                "message": "invalid tool slug 'jobs_get_status' — expected '<dcc>.<skill>.<action>' or bare action name"
                            })),
                        );
                    }
                    if body["tool_slug"] == "maya.abc12345.domain_failure" {
                        return (
                            StatusCode::OK,
                            Json(json!({
                                "slug": body["tool_slug"],
                                "output": {
                                    "success": false,
                                    "message": "tool domain failure"
                                },
                                "validation_skipped": false,
                                "request_id": "fixture-domain-failure"
                            })),
                        );
                    }
                    if body["tool_slug"] == "maya.abc12345.long_render" {
                        return (
                            StatusCode::OK,
                            Json(json!({
                                "slug": body["tool_slug"],
                                "output": {
                                    "job_id": "job-progress-42",
                                    "status": "pending"
                                }
                            })),
                        );
                    }
                    if body["tool_slug"] == "maya.abc12345.jobs_get_status" {
                        let poll = async_polls.fetch_add(1, Ordering::SeqCst);
                        let (status, current, message) = if poll == 0 {
                            ("running", 45, "rendering frame 45")
                        } else {
                            ("completed", 100, "render complete")
                        };
                        return (
                            StatusCode::OK,
                            Json(json!({
                                "slug": body["tool_slug"],
                                "output": {
                                    "job_id": "job-progress-42",
                                    "status": status,
                                    "progress": {
                                        "current": current,
                                        "total": 100,
                                        "message": message
                                    }
                                }
                            })),
                        );
                    }
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "tool_slug": body["tool_slug"],
                            "arguments": body["arguments"]
                        })),
                    )
                }
            }),
        )
        .route(
            "/v1/call_batch",
            post(|Json(body): Json<Value>| async move {
                let results: Vec<Value> = body
                    .get("calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(index, call)| {
                        let tool_slug = call
                            .get("tool_slug")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if tool_slug == "maya.abc12345.domain_failure" {
                            json!({
                                "index": index,
                                "tool_slug": tool_slug,
                                "ok": false,
                                "result": {
                                    "slug": tool_slug,
                                    "output": {
                                        "success": false,
                                        "message": "tool domain failure"
                                    }
                                },
                                "error": {
                                    "kind": "tool-error",
                                    "message": "backend transport succeeded but the tool reported failure"
                                }
                            })
                        } else {
                            json!({
                                "index": index,
                                "tool_slug": tool_slug,
                                "ok": true,
                                "result": {"success": true}
                            })
                        }
                    })
                    .collect();
                let success = results
                    .iter()
                    .all(|item| item.get("ok").and_then(Value::as_bool) == Some(true));
                Json(json!({
                    "success": success,
                    "stop_on_error": body
                        .get("stop_on_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    "results": results
                }))
            }),
        )
        .route(
            "/v1/dcc/{dcc_type}/instances/{instance_id}/call",
            post(
                |Path((dcc_type, instance_id)): Path<(String, String)>,
                 Json(body): Json<Value>| async move {
                    Json(json!({
                        "success": true,
                        "dcc_type": dcc_type,
                        "instance_id": instance_id,
                        "backend_tool": body["backend_tool"],
                        "arguments": body["arguments"]
                    }))
                },
            ),
        )
        .route(
            "/v1/dcc/{dcc_type}/instances/{instance_id}/stop",
            post(
                |Path((dcc_type, instance_id)): Path<(String, String)>,
                 Json(body): Json<Value>| async move {
                    Json(json!({
                        "ok": true,
                        "stopping": true,
                        "dcc_type": dcc_type,
                        "instance_id": instance_id,
                        "expected_owner": body.get("expected_owner").cloned().unwrap_or(Value::Null),
                        "expected_session": body.get("expected_session").cloned().unwrap_or(Value::Null)
                    }))
                },
            ),
        )
        .route(
            "/v1/update/check",
            get(
                |Query(query): Query<std::collections::HashMap<String, String>>| async move {
                    let binary = query.get("binary").map(String::as_str).unwrap_or_default();
                    let current = query
                        .get("current_version")
                        .map(String::as_str)
                        .unwrap_or("0.0.0");
                    if binary != "dcc-mcp-server" {
                        return (
                            StatusCode::NOT_FOUND,
                            Json(json!({
                                "error": format!("binary '{binary}' not found in update manifest")
                            })),
                        );
                    }

                    (
                        StatusCode::OK,
                        Json(json!({
                            "update_available": current != "0.19.0",
                            "latest_version": "0.19.0",
                            "download_url": "https://example.invalid/dcc-mcp-server.zip",
                            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                            "release_notes": "Server update"
                        })),
                    )
                },
            ),
        )
        .route(
            "/v1/update/download/{binary_name}",
            get(|Path(binary_name): Path<String>| async move {
                if binary_name != "dcc-mcp-server" {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({
                            "error": format!("binary '{binary_name}' not found in update manifest")
                        })),
                    );
                }
                (
                    StatusCode::OK,
                    Json(json!({
                        "download_url": "https://example.invalid/dcc-mcp-server.zip",
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    })),
                )
            }),
        );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
    });

    GatewayFixture {
        base_url: format!("http://{addr}"),
        shutdown: Some(shutdown_tx),
        thread: Some(thread),
    }
}

pub(crate) fn spawn_local_mcp_fixture() -> LocalMcpFixture {
    let tools_list_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let activate_group_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/mcp",
            post(
                |State((tools_list_calls, activate_group_calls)): State<(
                    std::sync::Arc<std::sync::atomic::AtomicUsize>,
                    std::sync::Arc<std::sync::atomic::AtomicUsize>,
                )>,
                 headers: HeaderMap,
                 Json(body): Json<Value>| async move {
                let accept = headers
                    .get(header::ACCEPT)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                if !(accept.contains("application/json") && accept.contains("text/event-stream"))
                {
                    return (
                        StatusCode::NOT_ACCEPTABLE,
                        Json(json!({
                            "error": "not_acceptable",
                            "message": "Client must accept both application/json and text/event-stream"
                        })),
                    );
                }

                let method = body.get("method").and_then(Value::as_str).unwrap_or("");
                match method {
                    "tools/list" => {
                        tools_list_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        (
                            StatusCode::OK,
                            Json(json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or(json!(null)),
                                "result": {
                                    "tools": [
                                        {
                                            "name": "search_tools",
                                            "description": "Search local tools",
                                            "inputSchema": {"type": "object"}
                                        },
                                        {
                                            "name": "load_skill",
                                            "description": "Load local skill",
                                            "inputSchema": {"type": "object"}
                                        },
                                        {
                                            "name": "maya_scene__get_session_info",
                                            "description": "Read scene session info",
                                            "inputSchema": {"type": "object", "properties": {}}
                                        },
                                        {
                                            "name": "workflow__run",
                                            "description": "Run workflow",
                                            "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}}
                                        }
                                    ]
                                }
                            })),
                        )
                    }
                    "tools/call" => {
                        let params = body.get("params").cloned().unwrap_or_else(|| json!({}));
                        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                        let arguments = params
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| json!({}));
                        let payload = match name {
                            "search_tools" => {
                                let query = arguments
                                    .get("query")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                if query.is_empty() {
                                    return (
                                        StatusCode::OK,
                                        Json(json!({
                                            "jsonrpc": "2.0",
                                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                                            "error": {
                                                "code": -32602,
                                                "message": "Missing required parameter: query"
                                            }
                                        })),
                                    );
                                }
                                if arguments.get("include_disabled") != Some(&json!(true)) {
                                    return (
                                        StatusCode::OK,
                                        Json(json!({
                                            "jsonrpc": "2.0",
                                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                                            "error": {
                                                "code": -32602,
                                                "message": "local search must include inactive-group tools"
                                            }
                                        })),
                                    );
                                }
                                if query == "skill-only" {
                                    json!({
                                        "total": 1,
                                        "query": query,
                                        "tools": [],
                                        "skill_candidates": [{
                                            "kind": "skill_candidate",
                                            "skill_name": "workflow",
                                            "description": "Workflow tools",
                                            "tags": ["workflow"],
                                            "dcc": "maya",
                                            "scope": "repo",
                                            "tool_count": 1,
                                            "matching_tools": [],
                                            "requires_load_skill": true,
                                            "load_hint": {
                                                "tool": "load_skill",
                                                "arguments": {"skill_name": "workflow"}
                                            }
                                        }]
                                    })
                                } else if query == "inactive" {
                                    json!({
                                        "total": 1,
                                        "query": query,
                                        "tools": [{
                                            "kind": "tool",
                                            "name": "workflow__run",
                                            "description": "Run workflow",
                                            "category": "workflow",
                                            "group": "execution",
                                            "enabled": false,
                                            "has_schema": true,
                                            "dcc": "maya",
                                            "skill_name": "workflow",
                                            "annotations": {
                                                "read_only_hint": false,
                                                "destructive_hint": false,
                                                "idempotent_hint": false,
                                                "open_world_hint": false
                                            }
                                        }],
                                        "skill_candidates": []
                                    })
                                } else {
                                    json!({
                                    "total": 2,
                                    "query": query,
                                    "tools": [{
                                        "kind": "tool",
                                        "name": "maya_scene__get_session_info",
                                        "description": "Read scene session info",
                                        "category": "scene",
                                        "group": "",
                                        "enabled": true,
                                        "has_schema": false,
                                        "dcc": "maya",
                                        "skill_name": "maya-scene",
                                        "annotations": {
                                            "read_only_hint": true,
                                            "destructive_hint": false,
                                            "idempotent_hint": true,
                                            "open_world_hint": false
                                        },
                                        "metadata": {
                                            "dcc": {
                                                "affinity": "any",
                                                "execution": "sync",
                                                "jobStrategy": "monolithic"
                                            }
                                        }
                                    }],
                                    "skill_candidates": [{
                                        "kind": "skill_candidate",
                                        "skill_name": "workflow",
                                        "description": "Workflow tools",
                                        "tags": ["workflow"],
                                        "dcc": "maya",
                                        "scope": "repo",
                                        "tool_count": 1,
                                        "matching_tools": ["run"],
                                        "tool_group": "execution",
                                        "requires_load_skill": true,
                                        "load_hint": {
                                            "tool": "load_skill",
                                            "arguments": {
                                                "skill_name": "workflow",
                                                "tool_group": "execution"
                                            }
                                        }
                                    }]
                                    })
                                }
                            }
                            "load_skill" => {
                                if arguments.get("dcc_type").is_some()
                                    || arguments.get("dcc").is_some()
                                    || arguments.get("instance_id").is_some()
                                    || arguments.get("target_tool_slug").is_some()
                                    || arguments.get("meta").is_some()
                                    || arguments.get("_meta").is_some()
                                {
                                    return (
                                        StatusCode::OK,
                                        Json(json!({
                                            "jsonrpc": "2.0",
                                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                                            "error": {
                                                "code": -32602,
                                                "message": "load_skill received local or gateway routing fields"
                                            }
                                        })),
                                    );
                                }
                                if arguments.get("tool_group").is_some()
                                    && arguments.get("activate_groups") != Some(&json!(false))
                                {
                                    return (
                                        StatusCode::OK,
                                        Json(json!({
                                            "jsonrpc": "2.0",
                                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                                            "error": {
                                                "code": -32602,
                                                "message": "targeted load_skill must default activate_groups=false"
                                            }
                                        })),
                                    );
                                }
                                if arguments.get("skill_name") == Some(&json!("broken")) {
                                    return (
                                        StatusCode::OK,
                                        Json(json!({
                                            "jsonrpc": "2.0",
                                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                                            "result": {
                                                "isError": true,
                                                "content": [{"type": "text", "text": "skill load failed"}]
                                            }
                                        })),
                                    );
                                }
                                match arguments.get("skill_name").and_then(Value::as_str) {
                                    Some("broken-domain") => json!({
                                        "success": false,
                                        "message": "skill domain failure"
                                    }),
                                    Some("not-loaded") => json!({
                                        "loaded": false,
                                        "message": "skill not loaded"
                                    }),
                                    _ => json!({
                                        "loaded": true,
                                        "skill_name": arguments.get("skill_name").cloned().unwrap_or(Value::Null),
                                        "activate_groups": arguments.get("activate_groups").cloned().unwrap_or(Value::Null),
                                        "strict_tool_group": arguments.get("strict_tool_group").cloned().unwrap_or(Value::Null),
                                        "registered_tools": ["workflow__run"],
                                        "tool_count": 1,
                                        "tools": [{
                                            "name": "workflow__run",
                                            "annotations": {
                                                "read_only_hint": false,
                                                "destructive_hint": false,
                                                "idempotent_hint": false,
                                                "open_world_hint": false
                                            },
                                            "metadata": {
                                                "dcc": {
                                                    "affinity": "any",
                                                    "execution": "sync",
                                                    "jobStrategy": "monolithic"
                                                }
                                            },
                                            "inputSchema": {
                                                "type": "object",
                                                "properties": {"name": {"type": "string"}},
                                                "required": ["name"]
                                            }
                                        }]
                                    }),
                                }
                            }
                            "activate_tool_group" => {
                                activate_group_calls
                                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                json!({
                                    "success": true,
                                    "group": arguments.get("group_name").cloned().unwrap_or(Value::Null),
                                })
                            }
                            "dcc_admin__reload_skills" => json!({
                                "reloaded": true,
                                "count": 1,
                                "skipped": []
                            }),
                            "maya_scene__get_session_info" => json!({
                                "success": true,
                                "scene": "fixture.ma",
                                "arguments": arguments
                            }),
                            "workflow__run" => json!({
                                "success": true,
                                "workflow": arguments.get("name").cloned().unwrap_or(Value::Null)
                            }),
                            _ => {
                                return (
                                    StatusCode::OK,
                                    Json(json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or(json!(null)),
                                        "result": {
                                            "isError": true,
                                            "content": [{"type": "text", "text": format!("unknown tool {name}")}]
                                        }
                                    })),
                                );
                            }
                        };

                        (
                            StatusCode::OK,
                            Json(json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or(json!(null)),
                                "result": {
                                    "isError": false,
                                    "content": [{"type": "text", "text": payload.to_string()}]
                                }
                            })),
                        )
                    }
                    _ => (
                        StatusCode::OK,
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                            "error": {
                                "code": -32601,
                                "message": "method not found"
                            }
                        })),
                    ),
                }
            }),
        )
        .route(
            "/v1/readyz",
            get(|| async {
                Json(json!({
                    "ready": true,
                    "readiness": {
                        "process": true,
                        "dcc": true,
                        "dispatcher": true,
                        "host_execution_bridge": true,
                        "main_thread_executor": true
                    }
                }))
            }),
        )
        .route(
            "/v1/describe",
            post(|Json(body): Json<Value>| async move {
                Json(json!({
                    "entry": {"slug": body["tool_slug"], "loaded": true},
                    "description": "Dynamically loaded workflow tool",
                    "input_schema": {
                        "type": "object",
                        "required": ["name"],
                        "properties": {"name": {"type": "string"}}
                    },
                    "annotations": {"loaded": true},
                    "metadata": {"dcc": {"affinity": "main"}}
                }))
            }),
        )
        .route(
            "/safe-stop",
            post(|Json(body): Json<Value>| async move {
                Json(json!({
                    "accepted": true,
                    "instance_id": body.get("instance_id").cloned().unwrap_or(Value::Null),
                    "dcc_type": body.get("dcc_type").cloned().unwrap_or(Value::Null),
                    "owner": body.get("owner").cloned().unwrap_or(Value::Null),
                    "session": body.get("session").cloned().unwrap_or(Value::Null)
                }))
            }),
        )
        .with_state((tools_list_calls.clone(), activate_group_calls.clone()));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
    });

    LocalMcpFixture {
        base_url: format!("http://{addr}"),
        shutdown: Some(shutdown_tx),
        thread: Some(thread),
        tools_list_calls,
        activate_group_calls,
    }
}

pub(crate) fn cli_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
}

pub(crate) fn run_json(args: &[&str]) -> Value {
    let output = cli_command().args(args).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

pub(crate) fn run_json_with_env(args: &[&str], envs: &[(&str, &str)]) -> Value {
    run_json_with_env_removed(args, envs, &[])
}

pub(crate) fn run_json_with_env_removed(
    args: &[&str],
    envs: &[(&str, &str)],
    removed_envs: &[&str],
) -> Value {
    let mut command = cli_command();
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    for key in removed_envs {
        command.env_remove(key);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

pub(crate) fn run_failure_with_env(args: &[&str], envs: &[(&str, &str)]) -> String {
    let mut command = cli_command();
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    assert!(
        !output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stderr).to_string()
}

pub(crate) fn run_text(args: &[&str]) -> String {
    let output = cli_command().args(args).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

pub(crate) fn unused_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

pub(crate) fn local_mcp_port(fixture: &LocalMcpFixture) -> u16 {
    fixture
        .base_url
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

pub(crate) struct AutoGatewayCleanup<'a> {
    pub(crate) host: &'a str,
    pub(crate) port: u16,
    pub(crate) envs: &'a [(&'a str, &'a str)],
}

impl Drop for AutoGatewayCleanup<'_> {
    fn drop(&mut self) {
        let mut command = cli_command();
        let port_s = self.port.to_string();
        command.args([
            "--no-auto-gateway",
            "gateway",
            "stop",
            "--host",
            self.host,
            "--port",
            port_s.as_str(),
        ]);
        for (key, value) in self.envs {
            command.env(key, value);
        }
        let _ = command.output();
    }
}

pub(crate) fn write_skill(
    root: &std::path::Path,
    relative: &str,
    content: &str,
) -> std::path::PathBuf {
    let dir = root.join(relative);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), content).unwrap();
    dir
}

pub(crate) fn run_git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "dcc-mcp-test")
        .env("GIT_AUTHOR_EMAIL", "dcc-mcp-test@example.com")
        .env("GIT_COMMITTER_NAME", "dcc-mcp-test")
        .env("GIT_COMMITTER_EMAIL", "dcc-mcp-test@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn git_head(repo: &std::path::Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

pub(crate) fn commit_git_skill_version(
    repo: &std::path::Path,
    version: &str,
    marker: &str,
) -> String {
    std::fs::write(
        repo.join("SKILL.md"),
        format!("---\nname: git-skill\ndescription: Git skill {version}\n---\n"),
    )
    .unwrap();
    std::fs::write(repo.join("marker.txt"), marker).unwrap();
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", version]);
    run_git(repo, &["tag", version]);
    git_head(repo)
}

pub(crate) fn write_zip(entries: &[(&str, &str)], dest: &std::path::Path) -> Vec<u8> {
    let file = std::fs::File::create(dest).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, content) in entries {
        zip.start_file(name, options).unwrap();
        std::io::Write::write_all(&mut zip, content.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    std::fs::read(dest).unwrap()
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}
