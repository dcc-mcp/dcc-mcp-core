mod support;

use serde_json::json;
use tempfile::NamedTempFile;

use support::*;

#[test]
fn non_interactive_install_execute_skips_confirmation_and_runs_plan() {
    let output = cli_command()
        .args([
            "install",
            "--dcc-type",
            "maya",
            "--python",
            "/__nonexistent__/python",
            "--execute",
            "--non-interactive",
            "--output",
            "json",
        ])
        .env_remove("DCC_MCP_INSTALL_DISABLED")
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed to launch /__nonexistent__/python"));
    assert!(!stderr.contains("requires confirmation"));
    assert!(!stderr.contains("Proceed with installation?"));
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
