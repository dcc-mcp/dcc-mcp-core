use std::process::Command;

use axum::Router;
use axum::extract::Json;
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

fn spawn_gateway_fixture() -> GatewayFixture {
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"ok": true, "service": "dcc-mcp-gateway"})) }),
        )
        .route(
            "/mcp",
            post(|Json(body): Json<Value>| async move {
                let method = body.get("method").and_then(Value::as_str).unwrap_or("");
                match method {
                    "initialize" => Json(json!({
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
                    "tools/list" => Json(json!({
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
                    _ => Json(json!({
                        "jsonrpc": "2.0",
                        "id": body.get("id").cloned().unwrap_or(json!(null)),
                        "error": {
                            "code": -32601,
                            "message": "method not found"
                        }
                    })),
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
                        "mcp_url": "http://127.0.0.1:18080/mcp"
                    }]
                }))
            }),
        )
        .route(
            "/v1/search",
            post(|Json(body): Json<Value>| async move {
                Json(json!({
                    "total": 1,
                    "hits": [{
                        "slug": "maya.abc12345.create_sphere",
                        "skill": "modeling",
                        "action": "create_sphere",
                        "dcc": body.get("dcc_type").and_then(Value::as_str).unwrap_or("maya"),
                        "summary": body.get("query").and_then(Value::as_str).unwrap_or("sphere"),
                        "loaded": true,
                        "scope": "gateway"
                    }]
                }))
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
            "/v1/call",
            post(|Json(body): Json<Value>| async move {
                Json(json!({
                    "success": true,
                    "tool_slug": body["tool_slug"],
                    "arguments": body["arguments"]
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
    ]);
    assert_eq!(search["hits"][0]["slug"], "maya.abc12345.create_sphere");

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
        workspace_root.join("skills/core"),
        workspace_root.join("skills/dcc-skills-creator"),
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
