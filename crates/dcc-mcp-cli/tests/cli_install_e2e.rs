mod support;

use serde_json::json;
use tempfile::{NamedTempFile, TempDir};

use support::*;

fn fake_pip(temp: &TempDir, version: &str) -> std::path::PathBuf {
    let package = temp.path().join("pip");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(package.join("__init__.py"), "").unwrap();
    std::fs::write(
        package.join("__main__.py"),
        format!(
            "import sys\nif len(sys.argv) > 1 and sys.argv[1] == 'show':\n    print('Name: dcc-mcp-maya')\n    print('Version: {version}')\n"
        ),
    )
    .unwrap();
    temp.path().to_path_buf()
}

#[test]
fn install_execute_json_reports_manual_registration_as_deferred() {
    let temp = TempDir::new().unwrap();
    let plan = run_json(&["install", "--dcc-type", "maya", "--python", "python"]);
    let python_path = fake_pip(&temp, plan["version"].as_str().unwrap());
    let output = cli_command()
        .args([
            "install",
            "--dcc-type",
            "maya",
            "--python",
            "python",
            "--execute",
            "--json",
        ])
        .env_remove("DCC_MCP_INSTALL_DISABLED")
        .env_remove("DCC_MCP_CATALOG_PATH")
        .env_remove("DCC_MCP_INSTALL_PYTHON")
        .env("PYTHONPATH", &python_path)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "partial");
    assert_eq!(report["stage"], "complete");
    assert_eq!(report["exit_code"], 0);
    assert_eq!(report["error"], serde_json::Value::Null);
    let register_step = report["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["id"] == "register-dcc")
        .expect("real planner report must include register-dcc");
    assert_eq!(register_step["status"], "deferred");
    assert_ne!(register_step["status"], "ok");
    assert_eq!(register_step["rollback"]["attempted"], false);
    assert_eq!(register_step["rollback"]["status"], "not_available");
    assert_eq!(report["verify"]["directly_usable"], false);

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("register-dcc ... DEFERRED"));
    assert!(!stderr.contains("register-dcc ... OK"));
    assert!(!stdout.contains(python_path.to_str().unwrap()));
    assert!(!stderr.contains(python_path.to_str().unwrap()));
}

#[test]
fn install_execute_json_failure_emits_one_safe_execution_report() {
    let output = cli_command()
        .args([
            "install",
            "--dcc-type",
            "maya",
            "--python",
            "/__nonexistent__/python",
            "--execute",
            "--json",
        ])
        .env_remove("DCC_MCP_INSTALL_DISABLED")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(30));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["dcc_type"], "maya");
    assert_eq!(report["exit_code"], 30);
    assert_eq!(report["stage"], "install");
    assert_eq!(report["error"]["code"], "INSTALL_STEP_FAILED");
    assert_eq!(report["error"]["stage"], "install");
    assert_eq!(report["error"]["exit_code"], 30);
    assert_eq!(report["steps"][0]["id"], "install-pip");
    assert_eq!(report["steps"][0]["status"], "failed");
    assert_eq!(report["steps"][1]["status"], "not_run");
    assert_eq!(report["receipt_path"], serde_json::Value::Null);
    assert_eq!(report["verify"]["directly_usable"], false);
    assert_eq!(report["verify"]["failure_stage"], "install");
    assert_eq!(report["verify"]["failure_reason"], "INSTALL_STEP_FAILED");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for public_output in [&*stdout, &*stderr] {
        assert!(!public_output.contains("/__nonexistent__/python"));
        assert!(!public_output.contains("failed to launch"));
        assert!(!public_output.contains("No such file"));
    }
    assert!(!stderr.contains("requires confirmation"));
    assert!(!stderr.contains("Proceed with installation?"));
}

#[test]
fn install_json_alias_preserves_the_plan_only_response() {
    let output = cli_command()
        .args(["install", "--dcc-type", "maya", "--json"])
        .env_remove("DCC_MCP_INSTALL_DISABLED")
        .output()
        .unwrap();

    assert!(output.status.success());
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["dcc_type"], "maya");
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-maya");
    assert!(plan.get("schema_version").is_none());
    assert!(
        plan["steps"]
            .as_array()
            .is_some_and(|steps| !steps.is_empty())
    );
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
            "--version",
            "0.9.22",
            "--python",
            "C:/Autodesk/Maya2026/bin/mayapy.exe",
        ],
        &[],
        &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
    );

    assert_eq!(plan["dcc_type"], "maya");
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-maya");
    assert_eq!(plan["adapter"]["min_core_version"], "0.19.45");
    assert_eq!(plan["steps"][0]["name"], "install-pip");
    assert_eq!(plan["steps"][0]["action"]["type"], "PipInstall");
    assert_eq!(plan["steps"][0]["action"]["package"], "dcc-mcp-maya");
    assert_eq!(plan["steps"][0]["action"]["version"], "0.9.22");
    assert_eq!(
        plan["steps"][0]["action"]["sha256"],
        "b288c2bf95014f827d3833ef5af4f7c36b2ff8a465ac647f9feb2163bc44397b"
    );
    assert!(
        plan["steps"][0]["action"]["artifact_url"]
            .as_str()
            .unwrap()
            .ends_with("dcc_mcp_maya-0.9.22-py3-none-any.whl")
    );
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
fn install_policy_env_disables_execute_with_a_stable_preflight_report() {
    let output = cli_command()
        .args([
            "install",
            "--dcc-type",
            "maya",
            "--python",
            "/__nonexistent__/python",
            "--execute",
            "--json",
        ])
        .env("DCC_MCP_INSTALL_DISABLED", "1")
        .env(
            "DCC_MCP_INSTALL_DISABLED_PROMPT",
            "Auto install unavailable; contact PipelineTD to deploy {adapter} for {dcc_type}.",
        )
        .env_remove("DCC_MCP_CATALOG_PATH")
        .env_remove("DCC_MCP_INSTALL_PYTHON")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(10));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["dcc_type"], "maya");
    assert_eq!(report["stage"], "preflight");
    assert_eq!(report["exit_code"], 10);
    assert_eq!(report["error"]["code"], "AUTO_INSTALL_DISABLED");
    assert!(
        report["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|step| step["status"] == "not_run")
    );
    let public = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!public.contains("/__nonexistent__/python"));
    assert!(!public.contains("PipelineTD"));
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
fn bundled_catalog_reports_current_unity_adapter_version() {
    let plan = run_json_with_env_removed(
        &["install", "--dcc-type", "unity"],
        &[],
        &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
    );

    assert_eq!(plan["adapter"]["name"], "dcc-mcp-unity");
    assert_eq!(plan["adapter"]["version"], "0.11.2");
}

#[test]
fn install_rejects_pip_version_without_a_catalog_pinned_artifact() {
    let output = cli_command()
        .args([
            "install",
            "--dcc-type",
            "maya",
            "--version",
            "0.9.21",
            "--output",
            "json",
        ])
        .env_remove("DCC_MCP_CATALOG_PATH")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("differs from catalog version '0.9.22'")
    );
}

#[test]
fn unpublished_adapters_do_not_advertise_automatic_install_steps() {
    for dcc_type in ["tiled", "material-maker", "wwise"] {
        let plan = run_json_with_env_removed(
            &["install", "--dcc-type", dcc_type],
            &[],
            &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
        );
        assert!(plan["adapter"]["install"].is_null(), "{dcc_type}");
        assert!(
            plan["steps"]
                .as_array()
                .unwrap()
                .iter()
                .all(|step| step["action"].is_null()),
            "{dcc_type}"
        );
    }
}

#[test]
fn bundled_catalog_reports_current_core_adapter_versions() {
    for (dcc_type, expected_version) in [
        ("maya", "0.9.22"),
        ("houdini", "0.31.5"),
        ("blender", "0.1.43"),
        ("3dsmax", "0.1.40"),
        ("photoshop", "0.1.37"),
    ] {
        let plan = run_json_with_env_removed(
            &["install", "--dcc-type", dcc_type],
            &[],
            &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
        );

        assert_eq!(plan["adapter"]["version"], expected_version, "{dcc_type}");
    }
}

#[test]
fn bundled_catalog_reports_marmoset_adapter() {
    let plan = run_json_with_env_removed(
        &["install", "--dcc-type", "marmoset"],
        &[],
        &["DCC_MCP_CATALOG_PATH", "DCC_MCP_INSTALL_PYTHON"],
    );

    assert_eq!(plan["adapter"]["name"], "dcc-mcp-marmoset");
    assert_eq!(plan["adapter"]["version"], "0.1.1");
    assert_eq!(plan["adapter"]["min_core_version"], "0.19.86");
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
