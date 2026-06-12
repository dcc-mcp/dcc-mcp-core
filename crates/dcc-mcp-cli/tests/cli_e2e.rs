use std::process::Command;

use axum::Router;
use axum::extract::{Json, Path, Query};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use dcc_mcp_skills::parse_skill_md;
use serde_json::{Value, json};
use tempfile::{NamedTempFile, TempDir};
use tokio::sync::oneshot;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};

struct GatewayFixture {
    base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
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

struct LocalMcpFixture {
    base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LocalMcpFixture {
    fn mcp_url(&self) -> String {
        format!("{}/mcp", self.base_url)
    }

    fn safe_stop_url(&self) -> String {
        format!("{}/safe-stop", self.base_url)
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

fn json_or_compact_fixture_response(
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

fn spawn_gateway_fixture() -> GatewayFixture {
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"ok": true, "service": "dcc-mcp-gateway"})) }),
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
                                "tools": [{
                                    "name": "search_tools",
                                    "description": "Search tools",
                                    "inputSchema": {"type": "object"}
                                }]
                            }
                        })),
                    ),
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
            post(|Json(body): Json<Value>| async move {
                Json(json!({
                    "success": true,
                    "tool_slug": body["tool_slug"],
                    "arguments": body["arguments"]
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
                            "sha256": "abc123",
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
                        "download_url": "https://example.invalid/dcc-mcp-server.zip"
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

fn spawn_local_mcp_fixture() -> LocalMcpFixture {
    let app = Router::new()
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
                    "tools/list" => (
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
                    ),
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
                                        "dcc": "maya",
                                        "skill_name": "maya-scene"
                                    }],
                                    "skill_candidates": [{
                                        "kind": "skill_candidate",
                                        "skill_name": "workflow",
                                        "description": "Workflow tools",
                                        "tags": ["workflow"],
                                        "dcc": "maya",
                                        "scope": "repo",
                                        "tool_count": 1,
                                        "matching_tools": ["workflow__run"],
                                        "requires_load_skill": true,
                                        "load_hint": {
                                            "tool": "load_skill",
                                            "arguments": {"skill_name": "workflow"}
                                        }
                                    }]
                                })
                            }
                            "load_skill" => {
                                if arguments.get("dcc_type").is_some()
                                    || arguments.get("dcc").is_some()
                                    || arguments.get("instance_id").is_some()
                                {
                                    return (
                                        StatusCode::OK,
                                        Json(json!({
                                            "jsonrpc": "2.0",
                                            "id": body.get("id").cloned().unwrap_or(json!(null)),
                                            "error": {
                                                "code": -32602,
                                                "message": "load_skill received local routing fields"
                                            }
                                        })),
                                    );
                                }
                                json!({
                                    "loaded": true,
                                    "skill_name": arguments.get("skill_name").cloned().unwrap_or(Value::Null),
                                    "registered_tools": ["workflow__run"],
                                    "tool_count": 1,
                                    "tools": [{
                                        "name": "workflow__run",
                                        "inputSchema": {"type": "object"}
                                    }]
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
                        "skill_catalog": true,
                        "dispatcher": true,
                        "host_execution_bridge": true,
                        "main_thread_executor": true
                    }
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

    LocalMcpFixture {
        base_url: format!("http://{addr}"),
        shutdown: Some(shutdown_tx),
        thread: Some(thread),
    }
}

fn cli_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
}

fn run_json(args: &[&str]) -> Value {
    let output = cli_command().args(args).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_json_with_env(args: &[&str], envs: &[(&str, &str)]) -> Value {
    run_json_with_env_removed(args, envs, &[])
}

fn run_json_with_env_removed(args: &[&str], envs: &[(&str, &str)], removed_envs: &[&str]) -> Value {
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

fn run_failure_with_env(args: &[&str], envs: &[(&str, &str)]) -> String {
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

fn run_text(args: &[&str]) -> String {
    let output = cli_command().args(args).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn unused_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn local_mcp_port(fixture: &LocalMcpFixture) -> u16 {
    fixture
        .base_url
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

struct AutoGatewayCleanup<'a> {
    host: &'a str,
    port: u16,
    envs: &'a [(&'a str, &'a str)],
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

fn write_skill(root: &std::path::Path, relative: &str, content: &str) -> std::path::PathBuf {
    let dir = root.join(relative);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), content).unwrap();
    dir
}

fn run_git(repo: &std::path::Path, args: &[&str]) {
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

fn commit_git_skill_version(repo: &std::path::Path, version: &str, marker: &str) {
    std::fs::write(
        repo.join("SKILL.md"),
        format!("---\nname: git-skill\ndescription: Git skill {version}\n---\n"),
    )
    .unwrap();
    std::fs::write(repo.join("marker.txt"), marker).unwrap();
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", version]);
    run_git(repo, &["tag", version]);
}

fn write_zip(entries: &[(&str, &str)], dest: &std::path::Path) -> Vec<u8> {
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

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn list_search_describe_and_call_gateway_rest_surface() {
    let fixture = spawn_gateway_fixture();

    let list = run_json(&["--base-url", &fixture.base_url, "list"]);
    assert_eq!(list["total"], 1);
    assert_eq!(list["instances"][0]["dcc_type"], "maya");
    assert_eq!(list["gateway"]["current"]["name"], "Maya-main-15084");
    assert_eq!(
        list["gateway"]["candidates"][0]["name"],
        "Maya-layout-120920"
    );

    let search = run_json(&[
        "--base-url",
        &fixture.base_url,
        "search",
        "--query",
        "sphere",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
    ]);
    assert_eq!(search["hits"][0]["slug"], "maya.abc12345.create_sphere");
    assert_eq!(search["hits"][0]["instance_id"], "abc12345");

    let describe = run_json(&[
        "--base-url",
        &fixture.base_url,
        "describe",
        "maya.abc12345.create_sphere",
    ]);
    assert_eq!(
        describe["record"]["tool_slug"],
        "maya.abc12345.create_sphere"
    );

    let loaded = run_json(&[
        "--base-url",
        &fixture.base_url,
        "load-skill",
        "workflow",
        "--dcc-type",
        "3dsmax",
        "--instance-id",
        "80321760",
    ]);
    assert_eq!(loaded["loaded"], true);
    assert_eq!(loaded["skill_name"], "workflow");
    assert_eq!(loaded["dcc_type"], "3dsmax");
    assert_eq!(loaded["instance_id"], "80321760");
    assert_eq!(loaded["registered_tools"][0], "workflow__run");

    let loaded_from_json = run_json(&[
        "--base-url",
        &fixture.base_url,
        "load-skill",
        "--json",
        r#"{"skill_name":"workflow","dcc_type":"3dsmax","instance_id":"80321760","activate_groups":false}"#,
    ]);
    assert_eq!(loaded_from_json["loaded"], true);
    assert_eq!(loaded_from_json["activate_groups"], false);
    assert_eq!(loaded_from_json["registered_tools"][0], "workflow__run");

    let call = run_json(&[
        "--base-url",
        &fixture.base_url,
        "call",
        "maya.abc12345.create_sphere",
        "--json",
        r#"{"radius":2}"#,
    ]);
    assert_eq!(call["success"], true);
    assert_eq!(call["arguments"]["radius"], 2);

    let direct_call = run_json(&[
        "--base-url",
        &fixture.base_url,
        "call",
        "maya_scene__get_session_info",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
        "--json",
        r#"{}"#,
    ]);
    assert_eq!(direct_call["success"], true);
    assert_eq!(direct_call["dcc_type"], "maya");
    assert_eq!(direct_call["instance_id"], "abc12345");
    assert_eq!(direct_call["backend_tool"], "maya_scene__get_session_info");

    let ready = run_json(&[
        "--base-url",
        &fixture.base_url,
        "wait-ready",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
        "--require",
        "skill_catalog,host_execution_bridge",
        "--timeout-secs",
        "1",
    ]);
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["readiness_source"], "gateway_readyz");
    assert_eq!(ready["gateway_readyz_error"], Value::Null);
    assert_eq!(ready["missing"].as_array().unwrap().len(), 0);

    let stop = run_json(&[
        "--base-url",
        &fixture.base_url,
        "stop-instance",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
        "--expected-owner",
        "release-smoke-test",
        "--expected-session",
        "test",
    ]);
    assert_eq!(stop["ok"], true);
    assert_eq!(stop["stopping"], true);
    assert_eq!(stop["expected_owner"], "release-smoke-test");
}

#[test]
fn local_list_reads_file_registry_after_gateway_ensure() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    let port = local_mcp_port(&fixture);
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", port);
    entry.display_name = Some("Maya-Rig".to_string());
    entry
        .metadata
        .insert("owner".to_string(), "release-smoke-test".to_string());
    file_registry.register(entry).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = registry.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS", "1"),
    ];

    let list = run_json_with_env(&["list"], &envs);

    assert_eq!(list["source"], "local_registry");
    assert_eq!(list["total"], 1);
    assert_eq!(list["instances"][0]["dcc_type"], "maya");
    assert_eq!(list["instances"][0]["display_name"], "Maya-Rig");
    assert_eq!(list["instances"][0]["mcp_url"], fixture.mcp_url());
    assert_eq!(list["gateway"]["current"]["role"], "local");
}

#[test]
fn local_list_uses_core_default_registry_without_env_override() {
    let fixture = spawn_local_mcp_fixture();
    let temp = TempDir::new().unwrap();
    let default_registry = temp.path().join("dcc-mcp-registry");
    let file_registry = FileRegistry::new(&default_registry).unwrap();
    let port = local_mcp_port(&fixture);
    let mut entry = ServiceEntry::new("photoshop", "127.0.0.1", port);
    entry.display_name = Some("Photoshop-Default-Registry".to_string());
    file_registry.register(entry).unwrap();

    let temp_s = temp.path().to_string_lossy().to_string();
    let default_registry_s = default_registry.to_string_lossy().to_string();
    let profiles = temp.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("TMP", temp_s.as_str()),
        ("TEMP", temp_s.as_str()),
        ("TMPDIR", temp_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS", "1"),
    ];

    let list = run_json_with_env_removed(
        &["list"],
        &envs,
        &[
            "DCC_MCP_REGISTRY_DIR",
            "DCC_MCP_GATEWAY_PROFILE",
            "DCC_MCP_BASE_URL",
        ],
    );

    assert_eq!(list["source"], "local_registry");
    assert_eq!(list["registry_dir"], default_registry_s);
    assert_eq!(list["total"], 1);
    assert_eq!(list["instances"][0]["dcc_type"], "photoshop");
    assert_eq!(
        list["instances"][0]["display_name"],
        "Photoshop-Default-Registry"
    );
}

#[test]
fn local_profile_controls_registered_instance_through_direct_mcp() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    entry.display_name = Some("Maya-Local".to_string());
    entry
        .metadata
        .insert("mcp_url".to_string(), fixture.mcp_url());
    entry
        .metadata
        .insert("owner".to_string(), "release-smoke-test".to_string());
    entry
        .metadata
        .insert("session".to_string(), "test".to_string());
    entry
        .metadata
        .insert("safe_stop_url".to_string(), fixture.safe_stop_url());
    let instance_id = entry.instance_id.to_string();
    let instance_short = entry.instance_id.simple().to_string()[..8].to_string();
    file_registry.register(entry).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = registry.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
        ("DCC_MCP_BASE_URL", ""),
    ];

    let search = run_json_with_env(&["search", "--query", "scene", "--dcc-type", "maya"], &envs);
    assert_eq!(search["source"], "local_mcp");
    assert_eq!(search["total"], 2);
    assert_eq!(
        search["hits"][0]["backend_tool"],
        "maya_scene__get_session_info"
    );
    assert_eq!(search["hits"][0]["instance_id"], instance_id);
    let slug = search["hits"][0]["slug"].as_str().unwrap();
    assert!(slug.starts_with(&format!("maya.{instance_short}.")));

    let describe = run_json_with_env(&["describe", slug], &envs);
    assert_eq!(describe["source"], "local_mcp");
    assert_eq!(describe["record"]["tool_slug"], slug);
    assert_eq!(describe["tool"]["name"], "maya_scene__get_session_info");

    let loaded = run_json_with_env(
        &[
            "load-skill",
            "workflow",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
        ],
        &envs,
    );
    assert_eq!(loaded["source"], "local_mcp");
    assert_eq!(loaded["loaded"], true);
    assert_eq!(loaded["registered_tools"][0], "workflow__run");

    let call = run_json_with_env(&["call", slug, "--json", r#"{"detail":true}"#], &envs);
    assert_eq!(call["source"], "local_mcp");
    assert_eq!(call["success"], true);
    assert_eq!(call["tool_slug"], slug);
    assert_eq!(call["result"]["isError"], false);

    let direct_call = run_json_with_env(
        &[
            "call",
            "workflow__run",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
            "--json",
            r#"{"name":"demo"}"#,
        ],
        &envs,
    );
    assert_eq!(direct_call["success"], true);
    assert_eq!(direct_call["backend_tool"], "workflow__run");

    let reload = run_json_with_env(
        &[
            "reload-skills",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
        ],
        &envs,
    );
    assert_eq!(reload["source"], "local_mcp");
    assert_eq!(reload["reloaded"], true);
    assert_eq!(reload["count"], 1);
    assert_eq!(
        reload["results"][0]["backend_tool"],
        "dcc_admin__reload_skills"
    );
    assert_eq!(reload["results"][0]["reloaded"], true);

    let ready = run_json_with_env(
        &[
            "wait-ready",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
            "--require",
            "dispatcher,host_execution_bridge",
            "--timeout-secs",
            "1",
        ],
        &envs,
    );
    assert_eq!(ready["source"], "local_mcp");
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["missing"].as_array().unwrap().len(), 0);

    let stop = run_json_with_env(
        &[
            "stop-instance",
            "--dcc-type",
            "maya",
            "--instance-id",
            &instance_short,
            "--expected-owner",
            "release-smoke-test",
            "--expected-session",
            "test",
        ],
        &envs,
    );
    assert_eq!(stop["source"], "local_mcp");
    assert_eq!(stop["ok"], true);
    assert_eq!(stop["response"]["accepted"], true);
}

#[test]
fn local_search_without_query_lists_tools_for_dcc_filter() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    entry
        .metadata
        .insert("mcp_url".to_string(), fixture.mcp_url());
    file_registry.register(entry).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = registry.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
        ("DCC_MCP_BASE_URL", ""),
    ];

    let search = run_json_with_env(&["search", "--dcc-type", "maya"], &envs);

    assert_eq!(search["source"], "local_mcp");
    assert_eq!(search["query"], Value::Null);
    let hit_names: Vec<&str> = search["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|hit| hit["backend_tool"].as_str())
        .collect();
    assert!(
        hit_names.contains(&"maya_scene__get_session_info"),
        "empty-query local search should list loaded tools: {search}"
    );
    assert!(
        hit_names.contains(&"workflow__run"),
        "empty-query local search should include all loaded tools: {search}"
    );
}

#[test]
fn local_search_routes_ready_sidecar_and_skips_unavailable_rows() {
    let fixture = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let file_registry = FileRegistry::new(registry.path()).unwrap();

    let mut diagnostic = ServiceEntry::new("maya", "127.0.0.1", 9);
    diagnostic.display_name = Some("Maya-Diagnostic".to_string());
    diagnostic.status = ServiceStatus::Booting;
    diagnostic
        .metadata
        .insert("dispatch_status".to_string(), "unavailable".to_string());
    diagnostic
        .metadata
        .insert("failure_stage".to_string(), "gateway-health".to_string());
    diagnostic.metadata.insert(
        "failure_reason".to_string(),
        "gateway health OK before sidecar dispatch".to_string(),
    );
    diagnostic
        .metadata
        .insert("mcp_url".to_string(), "http://127.0.0.1:9/mcp".to_string());
    file_registry.register(diagnostic).unwrap();

    let mut unavailable_sidecar = ServiceEntry::new("maya", "127.0.0.1", 9);
    unavailable_sidecar.display_name = Some("Maya-Sidecar-Unavailable".to_string());
    unavailable_sidecar.status = ServiceStatus::Available;
    unavailable_sidecar
        .metadata
        .insert("dispatch_status".to_string(), "unavailable".to_string());
    unavailable_sidecar
        .metadata
        .insert("dcc_mcp_role".to_string(), "per-dcc-sidecar".to_string());
    unavailable_sidecar
        .metadata
        .insert("failure_stage".to_string(), "host-rpc-connect".to_string());
    unavailable_sidecar.metadata.insert(
        "failure_reason".to_string(),
        "connection refused".to_string(),
    );
    unavailable_sidecar.metadata.insert(
        "host_rpc_uri".to_string(),
        "commandport://127.0.0.1:6000".to_string(),
    );
    unavailable_sidecar
        .metadata
        .insert("host_rpc_scheme".to_string(), "commandport".to_string());
    unavailable_sidecar
        .metadata
        .insert("sidecar_pid".to_string(), "4242".to_string());
    unavailable_sidecar.metadata.insert(
        "stdio_log_dir".to_string(),
        "C:/tmp/dcc-sidecar-logs".to_string(),
    );
    unavailable_sidecar.metadata.insert(
        "stdio_stdout_path".to_string(),
        "C:/tmp/dcc-sidecar-logs/sidecar-maya-4242.stdout.log".to_string(),
    );
    unavailable_sidecar.metadata.insert(
        "stdio_stderr_path".to_string(),
        "C:/tmp/dcc-sidecar-logs/sidecar-maya-4242.stderr.log".to_string(),
    );
    unavailable_sidecar
        .metadata
        .insert("mcp_url".to_string(), "http://127.0.0.1:9/mcp".to_string());
    file_registry.register(unavailable_sidecar).unwrap();

    let mut ready_sidecar = ServiceEntry::new("maya", "127.0.0.1", 0);
    ready_sidecar.display_name = Some("Maya-Sidecar-Ready".to_string());
    ready_sidecar
        .metadata
        .insert("dispatch_status".to_string(), "ready".to_string());
    ready_sidecar
        .metadata
        .insert("dcc_mcp_role".to_string(), "per-dcc-sidecar".to_string());
    ready_sidecar
        .metadata
        .insert("mcp_url".to_string(), fixture.mcp_url());
    let ready_id = ready_sidecar.instance_id.to_string();
    file_registry.register(ready_sidecar).unwrap();

    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = registry.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILE", "local"),
    ];

    let list = run_json_with_env(&["list"], &envs);
    assert_eq!(list["source"], "local_registry");
    assert_eq!(list["total"], 3);
    let instances = list["instances"].as_array().unwrap();
    assert!(
        instances
            .iter()
            .any(|instance| instance["display_name"] == "Maya-Diagnostic"),
        "local list should keep diagnostic rows visible"
    );
    assert!(
        instances
            .iter()
            .any(|instance| instance["display_name"] == "Maya-Sidecar-Unavailable"),
        "local list should keep unavailable sidecar rows visible"
    );
    let diagnostic_row = instances
        .iter()
        .find(|instance| instance["display_name"] == "Maya-Diagnostic")
        .unwrap();
    assert_eq!(diagnostic_row["direct_control"]["ready"], false);
    assert_eq!(diagnostic_row["direct_control"]["reason"], "service_status");
    assert!(
        diagnostic_row["direct_control"]["recommended_next_action"]
            .as_str()
            .unwrap()
            .contains("wait-ready")
    );
    let sidecar_row = instances
        .iter()
        .find(|instance| instance["display_name"] == "Maya-Sidecar-Unavailable")
        .unwrap();
    assert_eq!(sidecar_row["direct_control"]["ready"], false);
    assert_eq!(sidecar_row["direct_control"]["reason"], "dispatch_status");
    assert_eq!(
        sidecar_row["direct_control"]["diagnostics"]["failure_stage"],
        "host-rpc-connect"
    );
    assert_eq!(
        sidecar_row["direct_control"]["diagnostics"]["failure_reason"],
        "connection refused"
    );
    assert_eq!(
        sidecar_row["direct_control"]["diagnostics"]["host_rpc_uri"],
        "commandport://127.0.0.1:6000"
    );
    assert_eq!(
        sidecar_row["direct_control"]["diagnostics"]["logs"]["stderr_path"],
        "C:/tmp/dcc-sidecar-logs/sidecar-maya-4242.stderr.log"
    );
    assert!(
        sidecar_row["direct_control"]["recommended_next_action"]
            .as_str()
            .unwrap()
            .contains("dispatch_status=ready")
    );
    let ready_row = instances
        .iter()
        .find(|instance| instance["display_name"] == "Maya-Sidecar-Ready")
        .unwrap();
    assert_eq!(ready_row["direct_control"]["ready"], true);
    assert_eq!(ready_row["direct_control"]["route"], "local_mcp");
    assert_eq!(
        ready_row["direct_control"]["recommended_next_action"],
        "Use this instance through the local MCP route."
    );

    let search = run_json_with_env(&["search", "--query", "scene", "--dcc-type", "maya"], &envs);
    assert_eq!(search["source"], "local_mcp");
    assert_eq!(search["total"], 2);
    assert!(
        search["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["instance_id"] == ready_id),
        "local search should only route to direct dispatch-ready instances: {search}"
    );
}

#[test]
fn gateway_profiles_select_remote_gateway_for_list() {
    let fixture = spawn_gateway_fixture();
    let config = TempDir::new().unwrap();
    let profiles = config.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str())];

    let registered = run_json_with_env(
        &["gateway", "register", &fixture.base_url, "--name", "pcA"],
        &envs,
    );
    assert_eq!(registered["registered"], true);
    assert_eq!(registered["name"], "pcA");
    assert_eq!(registered["base_url"], fixture.base_url);

    let selected = run_json_with_env(&["gateway", "set", "pcA"], &envs);
    assert_eq!(selected["current"], "pcA");
    assert_eq!(selected["mode"], "remote");

    let profiles = run_json_with_env(&["gateway", "list"], &envs);
    assert_eq!(profiles["current"], "pcA");
    assert_eq!(profiles["selected"]["mode"], "remote");
    assert_eq!(profiles["selected"]["base_url"], fixture.base_url);
    assert_eq!(profiles["profiles"][0]["name"], "pcA");
    assert_eq!(profiles["profiles"][0]["base_url"], fixture.base_url);

    let list = run_json_with_env(&["list"], &envs);
    assert_eq!(list["total"], 1);
    assert_eq!(list["instances"][0]["dcc_type"], "maya");
    assert_eq!(list["gateway"]["current"]["name"], "Maya-main-15084");

    let local = run_json_with_env(&["gateway", "set", "local"], &envs);
    assert_eq!(local["current"], "local");
    assert_eq!(local["mode"], "local");

    let env_selected = run_json_with_env_removed(
        &["list"],
        &[
            ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
            ("DCC_MCP_GATEWAY_PROFILE", "pcA"),
        ],
        &["DCC_MCP_BASE_URL"],
    );
    assert_eq!(env_selected["total"], 1);
    assert_eq!(
        env_selected["gateway"]["current"]["name"],
        "Maya-main-15084"
    );

    let overridden = run_json_with_env(&["list", "--gateway", "pcA"], &envs);
    assert_eq!(overridden["total"], 1);
    assert_eq!(overridden["gateway"]["current"]["name"], "Maya-main-15084");
}

#[test]
fn gateway_profiles_route_all_dcc_control_commands_to_remote_gateway() {
    let fixture = spawn_gateway_fixture();
    let config = TempDir::new().unwrap();
    let profiles = config.path().join("gateway-profiles.json");
    let profiles_s = profiles.to_string_lossy().to_string();
    let envs = [("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str())];

    let registered = run_json_with_env(
        &["gateway", "register", &fixture.base_url, "--name", "pcA"],
        &envs,
    );
    assert_eq!(registered["registered"], true);
    let selected = run_json_with_env(&["gateway", "set", "pcA"], &envs);
    assert_eq!(selected["mode"], "remote");

    let search = run_json_with_env(
        &[
            "search",
            "--query",
            "sphere",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
        ],
        &envs,
    );
    assert_eq!(search["hits"][0]["scope"], "gateway");
    assert_eq!(search["hits"][0]["instance_id"], "abc12345");

    let describe = run_json_with_env(&["describe", "maya.abc12345.create_sphere"], &envs);
    assert_eq!(
        describe["record"]["tool_slug"],
        "maya.abc12345.create_sphere"
    );

    let loaded = run_json_with_env(
        &[
            "load-skill",
            "workflow",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
        ],
        &envs,
    );
    assert_eq!(loaded["loaded"], true);
    assert_eq!(loaded["skill_name"], "workflow");
    assert_eq!(loaded["dcc_type"], "maya");
    assert_eq!(loaded["instance_id"], "abc12345");

    let call = run_json_with_env(
        &[
            "call",
            "maya.abc12345.create_sphere",
            "--json",
            r#"{"radius":2}"#,
        ],
        &envs,
    );
    assert_eq!(call["success"], true);
    assert_eq!(call["tool_slug"], "maya.abc12345.create_sphere");
    assert_eq!(call["arguments"]["radius"], 2);

    let direct_call = run_json_with_env(
        &[
            "call",
            "maya_scene__get_session_info",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
            "--json",
            r#"{}"#,
        ],
        &envs,
    );
    assert_eq!(direct_call["success"], true);
    assert_eq!(direct_call["backend_tool"], "maya_scene__get_session_info");

    let reload = run_json_with_env(
        &[
            "reload-skills",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
        ],
        &envs,
    );
    assert_eq!(reload["source"], "gateway");
    assert_eq!(reload["count"], 1);
    assert_eq!(
        reload["results"][0]["backend_tool"],
        "dcc_admin__reload_skills"
    );
    assert_eq!(
        reload["results"][0]["result"]["backend_tool"],
        "dcc_admin__reload_skills"
    );
    assert_eq!(
        reload["results"][0]["result"]["instance_id"],
        "abc12345-0000-0000-0000-000000000000"
    );

    let ready = run_json_with_env(
        &[
            "wait-ready",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
            "--require",
            "skill_catalog,host_execution_bridge",
            "--timeout-secs",
            "1",
        ],
        &envs,
    );
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["readiness_source"], "gateway_readyz");
    assert_eq!(ready["gateway_readyz_error"], Value::Null);
    assert_eq!(ready["missing"].as_array().unwrap().len(), 0);

    let stop = run_json_with_env(
        &[
            "stop-instance",
            "--dcc-type",
            "maya",
            "--instance-id",
            "abc12345",
            "--expected-owner",
            "release-smoke-test",
            "--expected-session",
            "test",
        ],
        &envs,
    );
    assert_eq!(stop["ok"], true);
    assert_eq!(stop["stopping"], true);
    assert_eq!(stop["expected_owner"], "release-smoke-test");
}

#[test]
fn gateway_daemon_status_uses_formal_subcommand() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();

    let status = run_json(&[
        "gateway",
        "daemon",
        "status",
        "--host",
        "127.0.0.1",
        "--port",
        &port_s,
        "--registry-dir",
        &registry_s,
    ]);

    assert_eq!(status["healthy"], false);
    assert_eq!(status["running"], false);
    assert_eq!(status["pid"], Value::Null);
    assert_eq!(status["registry_dir"], registry_s);
    assert_eq!(
        status["pidfile"],
        registry
            .path()
            .join("gateway.pid")
            .to_string_lossy()
            .to_string()
    );
    assert_eq!(
        status["health_url"],
        format!("http://127.0.0.1:{port}/health")
    );
    assert!(status["cli_version"].as_str().unwrap().starts_with("0."));
}

#[test]
fn doctor_reports_local_defaults_without_starting_gateway() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = NamedTempFile::new().unwrap();
    let profiles_s = profiles.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
    ];

    let doctor = run_json_with_env(
        &[
            "--auto-gateway-bin",
            cli_bin,
            "doctor",
            "--gateway-port",
            &port_s,
        ],
        &envs,
    );

    assert_eq!(doctor["status"], "ok");
    assert_eq!(doctor["cli"]["name"], "dcc-mcp-cli");
    assert!(doctor["cli"]["version"].as_str().unwrap().starts_with("0."));
    assert_eq!(doctor["profile"]["stored_current"], "local");
    assert_eq!(doctor["profile"]["selected"]["name"], "local");
    assert_eq!(doctor["profile"]["selected"]["mode"], "local");
    assert_eq!(doctor["local"]["registry_dir"], registry_s);
    assert_eq!(doctor["local"]["inventory"]["ok"], true);
    assert_eq!(doctor["local"]["inventory"]["total"], 0);
    assert_eq!(doctor["local"]["inventory"]["direct_control"]["ready"], 0);
    assert_eq!(
        doctor["local"]["inventory"]["direct_control"]["not_ready"],
        0
    );
    assert_eq!(doctor["gateway"]["auto_start_enabled"], true);
    assert_eq!(
        doctor["gateway"]["default_base_url"],
        format!("http://127.0.0.1:{port}")
    );
    assert_eq!(doctor["gateway"]["status"]["healthy"], false);
    assert_eq!(doctor["server_binary"]["status"], "ok");
    assert_eq!(doctor["server_binary"]["source"], "explicit");
    assert_eq!(doctor["server_binary"]["path"], cli_bin);
    assert_eq!(doctor["server_binary"]["would_download_if_started"], false);
    assert!(
        doctor["server_binary"]["version"]
            .as_str()
            .unwrap()
            .contains("dcc-mcp-cli")
    );
}

#[test]
fn doctor_summarizes_local_direct_control_readiness() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let profiles = NamedTempFile::new().unwrap();
    let profiles_s = profiles.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let file_registry = FileRegistry::new(registry.path()).unwrap();

    let mut booting = ServiceEntry::new("maya", "127.0.0.1", 18080);
    booting.status = ServiceStatus::Booting;
    booting
        .metadata
        .insert("dispatch_status".to_string(), "unavailable".to_string());
    booting
        .metadata
        .insert("failure_stage".to_string(), "host-rpc-connect".to_string());
    booting.metadata.insert(
        "failure_reason".to_string(),
        "connection refused".to_string(),
    );
    booting.metadata.insert(
        "host_rpc_uri".to_string(),
        "commandport://127.0.0.1:6000".to_string(),
    );
    file_registry.register(booting).unwrap();

    let mut sidecar = ServiceEntry::new("maya", "127.0.0.1", 18081);
    sidecar
        .metadata
        .insert("dispatch_status".to_string(), "ready".to_string());
    sidecar
        .metadata
        .insert("dcc_mcp_role".to_string(), "per-dcc-sidecar".to_string());
    file_registry.register(sidecar).unwrap();

    let mut direct = ServiceEntry::new("maya", "127.0.0.1", 18082);
    direct
        .metadata
        .insert("dispatch_status".to_string(), "ready".to_string());
    file_registry.register(direct).unwrap();

    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_PROFILES_FILE", profiles_s.as_str()),
    ];

    let doctor = run_json_with_env(
        &[
            "--auto-gateway-bin",
            cli_bin,
            "doctor",
            "--gateway-port",
            &port_s,
        ],
        &envs,
    );

    assert_eq!(doctor["local"]["inventory"]["ok"], true);
    assert_eq!(doctor["local"]["inventory"]["total"], 3);
    assert_eq!(doctor["local"]["inventory"]["direct_control"]["ready"], 2);
    assert_eq!(
        doctor["local"]["inventory"]["direct_control"]["not_ready"],
        1
    );
    assert_eq!(
        doctor["local"]["inventory"]["direct_control"]["reasons"]["service_status"],
        1
    );
    let not_ready = doctor["local"]["inventory"]["direct_control"]["not_ready_instances"]
        .as_array()
        .unwrap();
    assert_eq!(not_ready.len(), 1);
    assert_eq!(not_ready[0]["reason"], "service_status");
    assert_eq!(
        not_ready[0]["diagnostics"]["failure_stage"],
        "host-rpc-connect"
    );
    assert_eq!(
        not_ready[0]["diagnostics"]["failure_reason"],
        "connection refused"
    );
    assert_eq!(
        not_ready[0]["diagnostics"]["host_rpc_uri"],
        "commandport://127.0.0.1:6000"
    );
    assert!(
        doctor["local"]["inventory"]["direct_control"]["reasons"]
            .get("per_dcc_sidecar")
            .is_none()
    );
}

#[test]
fn update_check_auto_starts_builtin_local_gateway() {
    let port = unused_loopback_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS", "1"),
    ];
    let _cleanup = AutoGatewayCleanup {
        host: "127.0.0.1",
        port,
        envs: &envs,
    };

    let output = {
        let mut command = cli_command();
        command.args([
            "--base-url",
            &base_url,
            "--auto-gateway-bin",
            cli_bin,
            "--auto-gateway-timeout-secs",
            "15",
            "update",
            "check",
            "--binary",
            "dcc-mcp-server",
            "--current-version",
            "0.0.0",
        ]);
        for (key, value) in &envs {
            command.env(key, value);
        }
        command.output().unwrap()
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "manifest-free gateway should return a structured update error"
    );
    assert!(
        stderr.contains("auto-started gateway"),
        "update check should auto-start the local gateway before querying updates: {stderr}"
    );
    let update: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(update["status"], "not_configured");
    assert_eq!(update["error"], "update_manifest_url_not_configured");
    assert_eq!(update["binary_name"], "dcc-mcp-server");
    assert_eq!(update["current_version"], "0.0.0");
}

#[test]
fn smoke_with_explicit_url_does_not_auto_start_gateway() {
    let port = unused_loopback_port();
    let port_s = port.to_string();
    let base_url = format!("http://127.0.0.1:{port}");
    let registry = TempDir::new().unwrap();
    let registry_s = registry.path().to_string_lossy().to_string();
    let cli_bin = env!("CARGO_BIN_EXE_dcc-mcp-cli");
    let envs = [
        ("DCC_MCP_REGISTRY_DIR", registry_s.as_str()),
        ("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS", "1"),
    ];
    let _cleanup = AutoGatewayCleanup {
        host: "127.0.0.1",
        port,
        envs: &envs,
    };

    let mcp_url = format!("{base_url}/mcp");
    let output = {
        let mut command = cli_command();
        command.args([
            "--base-url",
            &base_url,
            "--auto-gateway-bin",
            cli_bin,
            "--auto-gateway-timeout-secs",
            "2",
            "smoke",
            "--url",
            &mcp_url,
            "--timeout-secs",
            "1",
        ]);
        for (key, value) in &envs {
            command.env(key, value);
        }
        command.output().unwrap()
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        !output.status.success(),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("auto-started gateway"),
        "explicit smoke URL should not trigger auto-start: {stderr}"
    );
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["base_url"], base_url);
    assert_eq!(value["mcp_url"], mcp_url);
    let checks = value["checks"].as_array().unwrap();
    assert!(
        checks
            .iter()
            .any(|check| check["name"] == "health" && check["ok"] == false),
        "expected failed health check without auto-start: {checks:#?}"
    );

    let status = run_json_with_env(
        &[
            "--no-auto-gateway",
            "gateway",
            "status",
            "--host",
            "127.0.0.1",
            "--port",
            &port_s,
        ],
        &envs,
    );
    assert_eq!(status["healthy"], false);
}

#[test]
fn update_check_supports_server_binary_versions() {
    let fixture = spawn_gateway_fixture();

    let update = run_json(&[
        "--base-url",
        &fixture.base_url,
        "update",
        "check",
        "--binary",
        "dcc-mcp-server",
        "--current-version",
        "0.18.16",
    ]);

    assert_eq!(update["update_available"], true);
    assert_eq!(update["current_version"], "0.18.16");
    assert_eq!(update["latest_version"], "0.19.0");
    assert_eq!(
        update["download_url"],
        "https://example.invalid/dcc-mcp-server.zip"
    );
    assert_eq!(update["sha256"], "abc123");
    assert_eq!(update["release_notes"], "Server update");
}

#[test]
fn update_check_preserves_gateway_error_payload() {
    let fixture = spawn_gateway_fixture();

    let output = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "update",
            "check",
            "--binary",
            "dcc-mcp-cli",
            "--current-version",
            "0.18.16",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "update check should fail when the gateway reports an update error"
    );
    let update: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        update["error"],
        "binary 'dcc-mcp-cli' not found in update manifest"
    );
    assert_eq!(update["binary_name"], "dcc-mcp-cli");
    assert_eq!(update["current_version"], "0.18.16");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("missing field"),
        "stderr should not expose serde decode failures"
    );
}

#[test]
fn search_and_load_skill_decode_json_when_gateway_defaults_to_compact() {
    let fixture = spawn_gateway_fixture();

    let search = run_json(&[
        "--base-url",
        &fixture.base_url,
        "search",
        "--query",
        "sphere",
        "--dcc-type",
        "maya",
    ]);
    assert_eq!(search["hits"][0]["slug"], "maya.abc12345.create_sphere");

    let loaded = run_json(&[
        "--base-url",
        &fixture.base_url,
        "load-skill",
        "workflow",
        "--dcc-type",
        "maya",
        "--instance-id",
        "abc12345",
    ]);
    assert_eq!(loaded["loaded"], true);
    assert_eq!(loaded["registered_tools"][0], "workflow__run");
}

#[test]
fn pretty_list_shows_gateway_owner_and_candidates() {
    let fixture = spawn_gateway_fixture();

    let output = run_text(&[
        "--base-url",
        &fixture.base_url,
        "--output",
        "pretty",
        "list",
    ]);

    assert!(output.contains("Gateway"));
    assert!(output.contains("owner      Maya-main-15084"));
    assert!(output.contains("Maya-layout-120920"));
    assert!(output.contains("Instances"));
    assert!(output.contains("maya"));
}

#[test]
fn smoke_checks_gateway_mcp_and_rest_surfaces() {
    let fixture = spawn_gateway_fixture();
    let value = run_json(&[
        "--base-url",
        &fixture.base_url,
        "smoke",
        "--url",
        &format!("{}/mcp", fixture.base_url),
    ]);

    assert_eq!(value["ok"], true);
    assert_eq!(value["mcp_url"], format!("{}/mcp", fixture.base_url));
    let checks = value["checks"].as_array().unwrap();
    for expected in ["health", "mcp_initialize", "mcp_tools_list", "rest_search"] {
        assert!(
            checks
                .iter()
                .any(|check| check["name"] == expected && check["ok"] == true),
            "missing successful smoke check {expected}: {checks:#?}"
        );
    }
}

#[test]
fn install_builds_auditable_plan_from_catalog() {
    let mut catalog = NamedTempFile::new().unwrap();
    std::io::Write::write_all(
        &mut catalog,
        br#"
version: "1"
entries:
  - name: "dcc-mcp-maya"
    description: "Maya adapter"
    dcc: ["maya"]
    url: "https://example.invalid/maya"
    tags: ["adapter", "official"]
"#,
    )
    .unwrap();

    let catalog_path = catalog.path().to_string_lossy().to_string();
    let plan = run_json(&[
        "install",
        "--dcc-type",
        "maya",
        "--version",
        "2026",
        "--catalog",
        &catalog_path,
    ]);

    assert_eq!(plan["dcc_type"], "maya");
    assert_eq!(plan["version"], "2026");
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-maya");
    assert_eq!(plan["steps"].as_array().unwrap().len(), 4);
    assert_eq!(plan["next_steps"][0]["name"], "start-dcc-plugin");
    assert!(plan["next_steps"][0]["command"].is_null());
    assert_eq!(
        plan["next_steps"][1]["command"],
        json!(["dcc-mcp-cli", "doctor"])
    );
    assert_eq!(
        plan["next_steps"][3]["command"],
        json!(["dcc-mcp-cli", "wait-ready", "--dcc-type", "maya"])
    );
    assert_eq!(plan["next_steps"][3]["requires_live_instance"], true);
}

#[test]
fn install_uses_bundled_adapter_metadata_and_python_override() {
    let plan = run_json_with_env_removed(
        &[
            "install",
            "--dcc-type",
            "maya",
            "--python",
            "C:/Autodesk/Maya2026/bin/mayapy.exe",
        ],
        &[],
        &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
    );

    assert_eq!(plan["dcc_type"], "maya");
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-maya");
    assert_eq!(plan["adapter"]["min_core_version"], "0.18.20");
    assert_eq!(plan["steps"][0]["name"], "install-pip");
    assert_eq!(plan["steps"][0]["action"]["type"], "PipInstall");
    assert_eq!(plan["steps"][0]["action"]["package"], "dcc-mcp-maya");
    assert_eq!(
        plan["steps"][0]["action"]["python"],
        "C:/Autodesk/Maya2026/bin/mayapy.exe"
    );
    assert_eq!(plan["steps"][1]["action"]["type"], "RegisterDcc");
    assert_eq!(plan["next_steps"][0]["name"], "read-install-instructions");
    assert_eq!(
        plan["next_steps"][0]["url"],
        "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-maya/main/install.md"
    );
    assert!(plan["next_steps"][0]["command"].is_null());
    assert_eq!(
        plan["next_steps"][5]["command"],
        json!([
            "dcc-mcp-cli",
            "search",
            "--dcc-type",
            "maya",
            "--query",
            "diagnostics"
        ])
    );
    assert_eq!(
        plan["next_steps"][7]["command"],
        json!(["dcc-mcp-cli", "marketplace", "inspect", "<package-name>"])
    );
    assert_eq!(
        plan["next_steps"][8]["command"],
        json!([
            "dcc-mcp-cli",
            "marketplace",
            "install",
            "<package-name>",
            "--dcc",
            "maya"
        ])
    );
    assert_eq!(
        plan["next_steps"][9]["command"],
        json!(["dcc-mcp-cli", "reload-skills", "--dcc-type", "maya"])
    );
}

#[test]
fn install_policy_env_disables_execute_and_returns_custom_prompt() {
    let plan = run_json_with_env_removed(
        &[
            "install",
            "--dcc-type",
            "maya",
            "--python",
            "/__nonexistent__/python",
            "--execute",
        ],
        &[
            ("DCC_MCP_INSTALL_DISABLED", "1"),
            (
                "DCC_MCP_INSTALL_DISABLED_PROMPT",
                "Auto install unavailable; contact PipelineTD to deploy {adapter} for {dcc_type}.",
            ),
        ],
        &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
    );

    assert_eq!(plan["dcc_type"], "maya");
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-maya");
    assert_eq!(plan["steps"][0]["action"]["type"], "PipInstall");
    assert_eq!(
        plan["steps"][0]["action"]["python"],
        "/__nonexistent__/python"
    );
    assert_eq!(plan["install_policy"]["auto_install_enabled"], false);
    assert_eq!(
        plan["install_policy"]["prompt"],
        "Auto install unavailable; contact PipelineTD to deploy dcc-mcp-maya for maya."
    );
}

#[test]
fn install_bundled_catalog_covers_non_maya_first_party_adapters() {
    let plan = run_json_with_env_removed(
        &["install", "--dcc-type", "blender"],
        &[],
        &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
    );

    assert_eq!(plan["dcc_type"], "blender");
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-blender");
    assert_eq!(plan["steps"][0]["action"]["type"], "PipInstall");
    assert_eq!(plan["steps"][0]["action"]["package"], "dcc-mcp-blender");
    assert_eq!(plan["next_steps"][0]["name"], "read-install-instructions");
    assert_eq!(
        plan["next_steps"][0]["url"],
        "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-blender/main/install.md"
    );
}

#[test]
fn install_prefers_adapter_over_same_dcc_skill_pack() {
    let plan = run_json_with_env_removed(
        &["install", "--dcc-type", "photoshop"],
        &[],
        &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
    );

    assert_eq!(plan["dcc_type"], "photoshop");
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-photoshop");
    assert_eq!(plan["steps"][0]["action"]["type"], "PipInstall");
    assert_eq!(plan["steps"][0]["action"]["package"], "dcc-mcp-photoshop");
}

#[test]
fn install_accepts_human_dcc_name_aliases() {
    let plan = run_json_with_env_removed(
        &["install", "--dcc-type", "3ds Max"],
        &[],
        &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
    );

    assert_eq!(plan["dcc_type"], "3ds Max");
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-3dsmax");
    assert_eq!(plan["steps"][0]["action"]["type"], "PipInstall");
    assert_eq!(plan["steps"][0]["action"]["package"], "dcc-mcp-3dsmax");
}

#[test]
fn marketplace_add_list_search_and_inspect_local_source() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    std::fs::write(
        &catalog_path,
        r#"
{
  "version": "1",
  "entries": [{
    "name": "dcc-asset-hunyuan-download",
    "description": "Search and download Hunyuan 3D models via official API",
    "dcc": ["maya", "blender"],
    "tags": ["asset", "hunyuan", "download", "domain"],
    "version": "0.1.0",
    "min_core_version": "0.17.0",
    "maintainer": "dcc-mcp",
    "install": {
      "type": "git",
      "url": "https://github.com/dcc-mcp/dcc-asset-hunyuan-download",
      "ref": "v0.1.0"
    }
  }, {
    "name": "dcc-asset-polyhaven",
    "description": "Search and download Poly Haven CC0 assets",
    "dcc": ["blender"],
    "tags": ["asset", "polyhaven", "download"],
    "version": "0.1.0",
    "install": {
      "type": "git",
      "url": "https://github.com/dcc-mcp/dcc-asset-polyhaven",
      "ref": "v0.1.0"
    }
  }]
}
"#,
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    let sources = run_json_with_env(&["marketplace", "add", &source], &envs);
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert_eq!(sources[0]["url"], source);

    let listed = run_json_with_env(&["marketplace", "list"], &envs);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["origin"], "config");

    let search = run_json_with_env(
        &[
            "marketplace",
            "search",
            "--query",
            "download",
            "--dcc",
            "maya",
        ],
        &envs,
    );
    assert_eq!(search["count"], 1);
    assert_eq!(
        search["hits"][0]["entry"]["name"],
        "dcc-asset-hunyuan-download"
    );
    assert_eq!(search["hits"][0]["entry"]["install"]["type"], "git");

    let inspect = run_json_with_env(
        &[
            "marketplace",
            "inspect",
            "dcc-asset-hunyuan-download",
            "--source",
            &source,
        ],
        &envs,
    );
    assert_eq!(inspect["count"], 1);
    assert_eq!(inspect["matches"][0]["entry"]["install"]["ref"], "v0.1.0");
}

#[test]
fn marketplace_install_list_and_uninstall_path_package() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = write_skill(
        tmp.path(),
        "source-skill",
        "---\nname: dcc-asset-hunyuan-download\ndescription: Hunyuan downloads\n---\n",
    );
    std::fs::write(
        skill_dir.join("tools.yaml"),
        "tools:\n  - name: download\n    description: Download\n",
    )
    .unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "dcc-asset-hunyuan-download",
            "description": "Search and download Hunyuan 3D models via official API",
            "dcc": ["maya", "blender"],
            "tags": ["asset", "hunyuan", "download", "domain"],
            "version": "0.1.0",
            "install": {
                "type": "path",
                "url": skill_dir.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "dcc-asset-hunyuan-download",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    assert_eq!(installed["installed"], true);
    assert_eq!(installed["dcc"], "maya");
    assert_eq!(installed["install_type"], "path");
    assert_eq!(installed["reload_required"], true);
    let installed_path = installed["path"].as_str().unwrap();
    assert!(
        std::path::Path::new(installed_path)
            .join("SKILL.md")
            .is_file()
    );
    assert!(
        installed["skill_search_path"]
            .as_str()
            .unwrap()
            .ends_with("maya")
    );

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["packages"][0]["name"], "dcc-asset-hunyuan-download");
    assert_eq!(listed["packages"][0]["install_type"], "path");

    let uninstalled = run_json_with_env(
        &[
            "marketplace",
            "uninstall",
            "dcc-asset-hunyuan-download",
            "--dcc",
            "maya",
        ],
        &envs,
    );
    assert_eq!(uninstalled["uninstalled"], true);
    assert_eq!(uninstalled["removed_files"], true);
    assert_eq!(uninstalled["removed_state"], true);
    assert!(!std::path::Path::new(installed_path).exists());

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["count"], 0);
}

#[test]
fn marketplace_install_zip_package_verifies_sha256_and_flattens_archive_root() {
    let tmp = TempDir::new().unwrap();
    let zip_path = tmp.path().join("zip-skill.zip");
    let zip_bytes = write_zip(
        &[
            (
                "zip-skill-main/SKILL.md",
                "---\nname: zip-skill\ndescription: Zip skill\n---\n",
            ),
            ("zip-skill-main/tools.yaml", "tools: []\n"),
        ],
        &zip_path,
    );
    let digest = sha256_hex(&zip_bytes);

    let catalog_path = tmp.path().join("marketplace.json");
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "zip-skill",
            "description": "Zip skill package",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "zip",
                "url": zip_path.to_string_lossy(),
                "sha256": format!("sha256:{digest}")
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "zip-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    let installed_path = std::path::PathBuf::from(installed["path"].as_str().unwrap());
    assert_eq!(installed["install_type"], "zip");
    assert!(installed_path.join("SKILL.md").is_file());
    assert!(installed_path.join("tools.yaml").is_file());
    assert!(!installed_path.join("zip-skill-main").exists());

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["packages"][0]["install_type"], "zip");
}

#[test]
fn marketplace_install_zip_rejects_sha256_mismatch_without_replacing_existing_package() {
    let tmp = TempDir::new().unwrap();
    let good_skill = write_skill(
        tmp.path(),
        "good-skill",
        "---\nname: zip-skill\ndescription: Existing skill\n---\n",
    );
    let zip_path = tmp.path().join("zip-skill.zip");
    write_zip(
        &[(
            "SKILL.md",
            "---\nname: zip-skill\ndescription: Broken hash skill\n---\n",
        )],
        &zip_path,
    );

    let catalog_path = tmp.path().join("marketplace.json");
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let good_catalog = json!({
        "version": "1",
        "entries": [{
            "name": "zip-skill",
            "description": "Existing skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "path",
                "url": good_skill.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&good_catalog).unwrap(),
    )
    .unwrap();
    let source = catalog_path.to_string_lossy().to_string();
    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "zip-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    let installed_path = std::path::PathBuf::from(installed["path"].as_str().unwrap());

    let bad_catalog = json!({
        "version": "1",
        "entries": [{
            "name": "zip-skill",
            "description": "Bad hash skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.2.0",
            "install": {
                "type": "zip",
                "url": zip_path.to_string_lossy(),
                "sha256": "sha256:0000"
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&bad_catalog).unwrap(),
    )
    .unwrap();

    let stderr = run_failure_with_env(
        &[
            "marketplace",
            "install",
            "zip-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
            "--force",
        ],
        &envs,
    );
    assert!(stderr.contains("SHA-256 mismatch"));
    assert!(installed_path.join("SKILL.md").is_file());

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["packages"][0]["version"], "0.1.0");
    assert_eq!(listed["packages"][0]["install_type"], "path");
}

#[test]
fn marketplace_rejects_unsafe_install_components() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = write_skill(
        tmp.path(),
        "source-skill",
        "---\nname: safe-skill\ndescription: Safe skill\n---\n",
    );
    let catalog_path = tmp.path().join("marketplace.json");
    let catalog = json!({
        "version": "1",
        "entries": [{
            "name": "../unsafe-skill",
            "description": "Unsafe name",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "path",
                "url": skill_dir.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog).unwrap(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let stderr = run_failure_with_env(
        &[
            "marketplace",
            "install",
            "../unsafe-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    assert!(stderr.contains("invalid marketplace package name"));

    let stderr = run_failure_with_env(
        &[
            "marketplace",
            "uninstall",
            "../unsafe-skill",
            "--dcc",
            "maya",
        ],
        &envs,
    );
    assert!(stderr.contains("invalid marketplace package name"));
}

#[test]
fn marketplace_force_install_keeps_existing_package_when_replacement_fails() {
    let tmp = TempDir::new().unwrap();
    let good_skill = write_skill(
        tmp.path(),
        "good-skill",
        "---\nname: replaceable-skill\ndescription: Replaceable skill\n---\n",
    );
    let bad_skill = tmp.path().join("bad-skill");
    std::fs::create_dir_all(&bad_skill).unwrap();

    let catalog_path = tmp.path().join("marketplace.json");
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let good_catalog = json!({
        "version": "1",
        "entries": [{
            "name": "replaceable-skill",
            "description": "Replaceable skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "path",
                "url": good_skill.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&good_catalog).unwrap(),
    )
    .unwrap();
    let source = catalog_path.to_string_lossy().to_string();
    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "replaceable-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    let installed_path = std::path::PathBuf::from(installed["path"].as_str().unwrap());
    assert!(installed_path.join("SKILL.md").is_file());

    let bad_catalog = json!({
        "version": "1",
        "entries": [{
            "name": "replaceable-skill",
            "description": "Broken replacement",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.2.0",
            "install": {
                "type": "path",
                "url": bad_skill.to_string_lossy()
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&bad_catalog).unwrap(),
    )
    .unwrap();

    let stderr = run_failure_with_env(
        &[
            "marketplace",
            "install",
            "replaceable-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
            "--force",
        ],
        &envs,
    );
    assert!(stderr.contains("does not contain SKILL.md"));
    assert!(installed_path.join("SKILL.md").is_file());
}

#[test]
fn marketplace_update_git_package_uses_latest_catalog_ref() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("git-skill-repo");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.name", "dcc-mcp-test"]);
    run_git(&repo, &["config", "user.email", "dcc-mcp-test@example.com"]);
    commit_git_skill_version(&repo, "v0.1.0", "v1");
    commit_git_skill_version(&repo, "v0.2.0", "v2");

    let catalog_path = tmp.path().join("marketplace.json");
    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let install_root = tmp
        .path()
        .join("marketplace-root")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
        ("DCC_MCP_MARKETPLACE_INSTALL_ROOT", install_root.as_str()),
    ];

    let catalog_v1 = json!({
        "version": "1",
        "entries": [{
            "name": "git-skill",
            "description": "Git skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.1.0",
            "install": {
                "type": "git",
                "url": repo.to_string_lossy(),
                "ref": "v0.1.0"
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog_v1).unwrap(),
    )
    .unwrap();

    let installed = run_json_with_env(
        &[
            "marketplace",
            "install",
            "git-skill",
            "--dcc",
            "maya",
            "--source",
            &source,
        ],
        &envs,
    );
    let installed_path = std::path::PathBuf::from(installed["path"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(installed_path.join("marker.txt")).unwrap(),
        "v1"
    );

    let catalog_v2 = json!({
        "version": "1",
        "entries": [{
            "name": "git-skill",
            "description": "Git skill",
            "dcc": ["maya"],
            "tags": ["test"],
            "version": "0.2.0",
            "install": {
                "type": "git",
                "url": repo.to_string_lossy(),
                "ref": "v0.2.0"
            }
        }]
    });
    std::fs::write(
        &catalog_path,
        serde_json::to_string_pretty(&catalog_v2).unwrap(),
    )
    .unwrap();

    let outdated = run_json_with_env(
        &["marketplace", "outdated", "git-skill", "--dcc", "maya"],
        &envs,
    );
    assert_eq!(outdated["count"], 1);
    assert_eq!(outdated["packages"][0]["latest_version"], "0.2.0");
    assert_eq!(outdated["packages"][0]["install_ref"], "v0.2.0");

    let updated = run_json_with_env(
        &["marketplace", "update", "git-skill", "--dcc", "maya"],
        &envs,
    );
    assert_eq!(updated[0]["new_version"], "0.2.0");
    assert_eq!(
        std::fs::read_to_string(installed_path.join("marker.txt")).unwrap(),
        "v2"
    );

    let listed = run_json_with_env(&["marketplace", "list-installed", "--dcc", "maya"], &envs);
    assert_eq!(listed["packages"][0]["version"], "0.2.0");
    assert_eq!(listed["packages"][0]["install_ref"], "v0.2.0");
}

#[test]
fn lint_recurses_two_levels_and_reports_validation_errors() {
    let tmp = TempDir::new().unwrap();
    write_skill(
        tmp.path(),
        "studio/maya-tools",
        "---\nname: maya-tools\ndescription: Valid test skill\n---\n",
    );
    write_skill(tmp.path(), "studio/bad-skill", "no frontmatter\n");
    write_skill(tmp.path(), "too/deep/ignored-skill", "no frontmatter\n");

    let output = cli_command().arg("lint").arg(tmp.path()).output().unwrap();

    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["checked"], 2);
    assert_eq!(value["errors"], 1);
    let reports = value["reports"].as_array().unwrap();
    assert!(reports.iter().any(|report| {
        report["skill_dir"]
            .as_str()
            .is_some_and(|path| path.contains("bad-skill"))
    }));
    assert!(!reports.iter().any(|report| {
        report["skill_dir"]
            .as_str()
            .is_some_and(|path| path.contains("ignored-skill"))
    }));
}

#[test]
fn lint_bundled_skills_are_present_and_clean() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap();
    let builtin_skill_roots = [
        workspace_root.join("skills/dcc-cli-gateway"),
        workspace_root.join("python/dcc_mcp_core/skills"),
    ];

    for root in &builtin_skill_roots {
        assert!(
            root.is_dir(),
            "missing bundled skill root: {}",
            root.display()
        );
    }

    let output = cli_command()
        .arg("lint")
        .arg("--max-depth")
        .arg("4")
        .args(&builtin_skill_roots)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["checked"].as_u64().unwrap() > 0,
        "expected bundled skills to be linted"
    );
    assert_eq!(value["errors"], 0);
    assert_eq!(value["warnings"], 0);
}

#[test]
fn dcc_cli_gateway_skill_is_local_first_without_required_gateway_env() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap();
    let skill_dir = workspace_root.join("skills/dcc-cli-gateway");

    let meta = parse_skill_md(&skill_dir).expect("dcc-cli-gateway SKILL.md parses");

    assert_eq!(meta.name, "dcc-cli-gateway");
    assert!(meta.description.contains("dcc-mcp-cli local registry"));
    assert!(meta.required_env_vars().is_empty());
    assert_eq!(meta.primary_env(), None);
}

#[test]
fn marketplace_schema_validation_rejects_empty_name() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    // Entry with empty name — passes serde but fails schema (minLength: 1).
    std::fs::write(
        &catalog_path,
        r#"{
  "version": "1",
  "entries": [{
    "name": "",
    "description": "Has empty name",
    "dcc": ["maya"]
  }]
}"#,
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Without --skip-validation, search should fail with a validation error.
    let stderr = run_failure_with_env(&["marketplace", "search", "--source", &source], &envs);
    assert!(
        stderr.contains("validation"),
        "expected validation error, got: {stderr}"
    );
}

#[test]
fn marketplace_skip_validation_flag_filters_invalid_entries() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("marketplace.json");
    // One valid entry, one with empty name (schema-invalid).
    std::fs::write(
        &catalog_path,
        r#"{
  "version": "1",
  "entries": [
    {
      "name": "valid-skill",
      "description": "A valid skill",
      "dcc": ["maya"]
    },
    {
      "name": "",
      "description": "Empty name entry",
      "dcc": ["blender"]
    }
  ]
}"#,
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // With --skip-validation, the invalid entry should be silently dropped.
    let search = run_json_with_env(
        &[
            "marketplace",
            "search",
            "--source",
            &source,
            "--skip-validation",
        ],
        &envs,
    );
    assert_eq!(search["count"], 1);
    assert_eq!(search["hits"][0]["entry"]["name"], "valid-skill");
}

#[test]
fn marketplace_merge_priority_explicit_overrides_config() {
    let tmp = TempDir::new().unwrap();

    // Config source (lower priority) — old version
    let config_catalog = tmp.path().join("config-marketplace.json");
    std::fs::write(
        &config_catalog,
        json!({
            "version": "1",
            "entries": [{
                "name": "shared-skill",
                "description": "From config source — old version",
                "dcc": ["maya"],
                "version": "0.1.0"
            }]
        })
        .to_string(),
    )
    .unwrap();

    // Explicit source (higher priority) — newer version
    let explicit_catalog = tmp.path().join("explicit-marketplace.json");
    std::fs::write(
        &explicit_catalog,
        json!({
            "version": "1",
            "entries": [{
                "name": "shared-skill",
                "description": "From explicit source — new version",
                "dcc": ["maya"],
                "version": "0.3.0"
            }]
        })
        .to_string(),
    )
    .unwrap();

    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    // Pre-configure the config source
    let config_source_url = config_catalog.to_string_lossy().to_string();
    std::fs::write(
        &config_path,
        json!({"sources": [{"name": "config-catalog", "url": config_source_url}]}).to_string(),
    )
    .unwrap();

    let explicit_source = explicit_catalog.to_string_lossy().to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Search with explicit source — explicit's entry should win.
    let search = run_json_with_env(
        &[
            "marketplace",
            "search",
            "--source",
            &explicit_source,
            "--query",
            "shared-skill",
        ],
        &envs,
    );
    assert_eq!(search["count"], 1);
    assert_eq!(search["hits"][0]["entry"]["version"], "0.3.0");
    assert_eq!(
        search["hits"][0]["entry"]["description"],
        "From explicit source — new version"
    );
    assert_eq!(search["hits"][0]["source"]["origin"], "explicit");
}

#[test]
fn marketplace_search_dedupes_same_entry_from_multiple_sources() {
    let tmp = TempDir::new().unwrap();

    // Two config sources with overlapping entry names
    let catalog1 = tmp.path().join("catalog1.json");
    std::fs::write(
        &catalog1,
        json!({
            "version": "1",
            "entries": [
                {"name": "skill-a", "description": "From catalog 1", "dcc": ["maya"]},
                {"name": "skill-b", "description": "Shared skill from catalog 1", "dcc": ["blender"]}
            ]
        })
        .to_string(),
    )
    .unwrap();

    let catalog2 = tmp.path().join("catalog2.json");
    std::fs::write(
        &catalog2,
        json!({
            "version": "1",
            "entries": [
                {"name": "skill-b", "description": "Shared skill from catalog 2", "dcc": ["blender"]},
                {"name": "skill-c", "description": "From catalog 2", "dcc": ["houdini"]}
            ]
        })
        .to_string(),
    )
    .unwrap();

    let source1 = catalog1.to_string_lossy().to_string();
    let source2 = catalog2.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    // Register catalog1 (lower priority — registered first in config)
    // and pass catalog2 as explicit (higher priority).
    std::fs::write(
        &config_path,
        json!({"sources": [{"name": "catalog1", "url": source1}]}).to_string(),
    )
    .unwrap();

    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Search with explicit source for catalog2 — only catalog2 is searched
    // because explicit sources are exclusive (replace configured sources).
    let search = run_json_with_env(&["marketplace", "search", "--source", &source2], &envs);
    // Should have exactly 2 unique entries (skill-b, skill-c) from catalog2.
    // catalog1 is not searched because --source is exclusive.
    assert_eq!(search["count"], 2);
    let skill_b = search["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["entry"]["name"] == "skill-b")
        .unwrap();
    assert_eq!(
        skill_b["entry"]["description"],
        "Shared skill from catalog 2"
    );
    assert_eq!(skill_b["source"]["origin"], "explicit");
    // skill-a from catalog1 must not appear.
    assert!(
        !search["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["entry"]["name"] == "skill-a"),
        "configured-source entries must not appear when explicit --source is given"
    );
}

#[test]
fn marketplace_explicit_source_is_exclusive_regression() {
    let tmp = TempDir::new().unwrap();

    // Configured source with one entry
    let config_catalog = tmp.path().join("config-catalog.json");
    std::fs::write(
        &config_catalog,
        json!({
            "version": "1",
            "entries": [
                {"name": "config-only", "description": "Only in configured source", "dcc": ["maya"]}
            ]
        })
        .to_string(),
    )
    .unwrap();

    // Explicit source with a different entry
    let explicit_catalog = tmp.path().join("explicit-catalog.json");
    std::fs::write(
        &explicit_catalog,
        json!({
            "version": "1",
            "entries": [
                {"name": "explicit-only", "description": "Only in explicit source", "dcc": ["blender"]}
            ]
        })
        .to_string(),
    )
    .unwrap();

    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    std::fs::write(
        &config_path,
        json!({"sources": [{"name": "config", "url": config_catalog.to_string_lossy()}]})
            .to_string(),
    )
    .unwrap();

    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Search with explicit source — config-only must NOT appear.
    let search = run_json_with_env(
        &[
            "marketplace",
            "search",
            "--source",
            explicit_catalog.to_string_lossy().as_ref(),
            "--query",
            "config",
        ],
        &envs,
    );
    let hit_names: Vec<&str> = search["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["entry"]["name"].as_str().unwrap())
        .collect();
    assert!(
        !hit_names.contains(&"config-only"),
        "configured-source entries must not appear when explicit --source is given; got {hit_names:?}"
    );
}

#[test]
fn marketplace_entry_with_icon_validates() {
    let tmp = TempDir::new().unwrap();
    let catalog_path = tmp.path().join("catalog-with-icon.json");
    // Entry with an icon field — must pass schema validation.
    std::fs::write(
        &catalog_path,
        json!({
            "version": "1",
            "entries": [{
                "name": "skill-with-icon",
                "description": "A skill that ships an icon",
                "dcc": ["maya"],
                "icon": "icon.png"
            }]
        })
        .to_string(),
    )
    .unwrap();

    let source = catalog_path.to_string_lossy().to_string();
    let config_path = tmp
        .path()
        .join("sources.json")
        .to_string_lossy()
        .to_string();
    let envs = [
        ("DCC_MCP_MARKETPLACE_SOURCES_FILE", config_path.as_str()),
        ("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1"),
    ];

    // Without --skip-validation, search should succeed because icon is a valid
    // property in the schema.
    let search = run_json_with_env(&["marketplace", "search", "--source", &source], &envs);
    assert_eq!(search["count"], 1);
    assert_eq!(search["hits"][0]["entry"]["name"], "skill-with-icon");
}
