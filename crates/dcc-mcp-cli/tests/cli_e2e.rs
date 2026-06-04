use std::process::Command;

use axum::Router;
use axum::extract::{Json, Path};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use tempfile::{NamedTempFile, TempDir};
use tokio::sync::oneshot;

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
    let mut command = cli_command();
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
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

fn write_skill(root: &std::path::Path, relative: &str, content: &str) -> std::path::PathBuf {
    let dir = root.join(relative);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), content).unwrap();
    dir
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
