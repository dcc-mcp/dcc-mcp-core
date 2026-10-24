mod support;

use serde_json::{Value, json};
use tempfile::TempDir;

use support::cli_command;

fn pip_entry(name: &str, dcc_type: &str) -> Value {
    json!({
        "name": name,
        "description": format!("{dcc_type} adapter fixture"),
        "dcc": [dcc_type],
        "tags": ["adapter"],
        "version": "1.0.0",
        "install": {
            "type": "pip",
            "pip_package": name,
            "url": format!(
                "https://example.invalid/{}-1.0.0-py3-none-any.whl",
                name.replace('-', "_")
            ),
            "sha256": "a".repeat(64)
        }
    })
}

fn pip_execution_sentinel(root: &TempDir) -> std::path::PathBuf {
    let package = root.path().join("pip");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(package.join("__init__.py"), "").unwrap();
    std::fs::write(
        package.join("__main__.py"),
        concat!(
            "import os\n",
            "from pathlib import Path\n",
            "Path(os.environ['INSTALL_TEST_SENTINEL']).write_text('executed')\n",
            "raise SystemExit(91)\n"
        ),
    )
    .unwrap();
    root.path().join("pip-was-executed")
}

fn execute_catalog(root: &TempDir, sentinel: &std::path::Path) -> std::process::Output {
    cli_command()
        .args([
            "install",
            "--dcc-type",
            "godot",
            "--python",
            "python",
            "--execute",
            "--json",
            "--catalog",
        ])
        .arg(root.path().join("catalog.json"))
        .env_remove("DCC_MCP_INSTALL_DISABLED")
        .env_remove("DCC_MCP_CATALOG_PATH")
        .env_remove("DCC_MCP_INSTALL_PYTHON")
        .env("PYTHONPATH", root.path())
        .env("INSTALL_TEST_SENTINEL", sentinel)
        .output()
        .unwrap()
}

#[test]
fn invalid_explicit_catalogs_fail_before_the_real_cli_launches_pip() {
    let root = TempDir::new().unwrap();
    let sentinel = pip_execution_sentinel(&root);
    let selected = pip_entry("dcc-mcp-godot", "godot");
    let other = pip_entry("dcc-mcp-blender", "blender");
    let mut bad_digest = other.clone();
    bad_digest["install"]["sha256"] = json!("private-invalid-digest-marker");
    let mut wrong_wheel = other.clone();
    wrong_wheel["version"] = json!("2.0.0");
    let mut invalid_requirement = other;
    invalid_requirement["core_requirement"] = json!(">=0.20.0,<1.0.0");
    let mut duplicate = selected.clone();
    duplicate["name"] = json!("DCC-MCP-GODOT");

    for yaml in [false, true] {
        for invalid in [
            bad_digest.clone(),
            wrong_wheel.clone(),
            invalid_requirement.clone(),
            duplicate.clone(),
        ] {
            let document = json!({"entries": [selected.clone(), invalid]});
            let contents = if yaml {
                serde_yaml_ng::to_string(&document).unwrap()
            } else {
                serde_json::to_string(&document).unwrap()
            };
            std::fs::write(root.path().join("catalog.json"), contents).unwrap();
            let output = execute_catalog(&root, &sentinel);
            assert_eq!(
                output.status.code(),
                Some(10),
                "stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["schema_version"], 1);
            assert_eq!(report["status"], "failed");
            assert_eq!(report["stage"], "preflight");
            assert_eq!(report["exit_code"], 10);
            assert_eq!(report["error"]["code"], "INSTALL_PLAN_FAILED");
            assert_eq!(report["steps"], json!([]));
            assert_eq!(report["rollback"]["attempted"], false);
            assert_eq!(report["receipt_path"], Value::Null);
            assert_eq!(report["verify"]["directly_usable"], false);
            assert!(!sentinel.exists(), "rejected catalog launched pip");
            let stdout = String::from_utf8(output.stdout).unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert_eq!(stdout.lines().count(), 1);
            for public_output in [&stdout, &stderr] {
                assert!(!public_output.contains("private-invalid-digest-marker"));
                assert!(!public_output.contains(root.path().to_str().unwrap()));
            }
        }
    }

    // Positive control: the same real CLI and harmless pip module would execute
    // an admitted catalog. This is a subprocess sentinel, not a package install.
    std::fs::write(
        root.path().join("catalog.json"),
        serde_json::to_string(&json!({"entries": [selected]})).unwrap(),
    )
    .unwrap();
    let output = execute_catalog(&root, &sentinel);
    assert_eq!(output.status.code(), Some(30));
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "executed");
}

#[test]
fn malformed_explicit_document_shapes_preserve_the_bounded_preflight_contract() {
    let root = TempDir::new().unwrap();
    let sentinel = pip_execution_sentinel(&root);
    let selected = pip_entry("dcc-mcp-godot", "godot");
    // Explicit --catalog already failed closed for a missing collection. The
    // default-path fallback regression is covered by the service tests instead.
    for yaml in [false, true] {
        for document in [
            json!({"version": "1", "entires": [selected.clone()]}),
            json!({"version": "1"}),
            json!({"entries": [selected.clone()], "skills": []}),
            json!(["1", [selected.clone()]]),
        ] {
            let contents = if yaml {
                serde_yaml_ng::to_string(&document).unwrap()
            } else {
                serde_json::to_string(&document).unwrap()
            };
            std::fs::write(root.path().join("catalog.json"), contents).unwrap();
            let output = execute_catalog(&root, &sentinel);
            assert_eq!(output.status.code(), Some(10));
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["stage"], "preflight");
            assert_eq!(report["error"]["code"], "INSTALL_PLAN_FAILED");
            assert_eq!(report["steps"], json!([]));
            assert_eq!(report["receipt_path"], Value::Null);
            assert_eq!(report["rollback"]["attempted"], false);
            assert!(!sentinel.exists(), "malformed document launched pip");
            assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
        }
    }
}

#[test]
fn repeated_json_collection_keys_fail_before_the_real_cli_launches_pip() {
    let root = TempDir::new().unwrap();
    let sentinel = pip_execution_sentinel(&root);
    let entries = serde_json::to_string(&vec![pip_entry("dcc-mcp-godot", "godot")]).unwrap();
    for (key, repeated_key) in [
        ("entries", "entries"),
        ("items", "items"),
        ("skills", "skills"),
        ("entries", r"entr\u0069es"),
        ("entries", "items"),
        ("items", "skills"),
    ] {
        for (first, last) in [(entries.as_str(), "[]"), ("[]", entries.as_str())] {
            let text = format!(r#"{{"{key}":{first},"{repeated_key}":{last}}}"#);
            std::fs::write(root.path().join("catalog.json"), text).unwrap();
            let output = execute_catalog(&root, &sentinel);
            assert_eq!(output.status.code(), Some(10));
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["stage"], "preflight");
            assert_eq!(report["error"]["code"], "INSTALL_PLAN_FAILED");
            assert_eq!(report["steps"], json!([]));
            assert_eq!(report["receipt_path"], Value::Null);
            assert_eq!(report["rollback"]["attempted"], false);
            assert!(!sentinel.exists(), "repeated collection launched pip");
            assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
        }
    }
}
