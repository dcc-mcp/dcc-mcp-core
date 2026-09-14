use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn bundled_plan(dcc: &str) -> Value {
    let root = TempDir::new().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
        .args(["install", "--dcc-type", dcc, "--offline", "--json"])
        .env("DCC_MCP_INSTALL_CACHE", root.path().join("catalog.json"))
        .env_remove("DCC_MCP_CATALOG_PATH")
        .env_remove("DCC_MCP_INSTALL_DISABLED")
        .env_remove("DCC_MCP_INSTALL_PYTHON")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn obs_plan_uses_approved_release_bytes_and_matching_instructions() {
    let plan = bundled_plan("obs");
    assert_eq!(plan["version"], "1.4.0");
    let install = &plan["adapter"]["install"];
    assert_eq!(
        install["url"],
        "https://files.pythonhosted.org/packages/c1/7e/e928e483625278c43a84aced9f6c9243964f94e521bce8a77f66c0aedf03/dcc_mcp_obs-1.4.0-py3-none-any.whl"
    );
    assert_eq!(
        install["sha256"],
        "31afb647da5c999147fd6629e8abb5a2020b30519a1fc68d39fb5c134bdbbddb"
    );
    assert_eq!(
        install["instructions_url"],
        "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-obs/4ae80d4a7f062c6a496ad2848cf3eaeaea08ffbe/install.md"
    );
    let pip = plan["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["action"]["type"] == "PipInstall")
        .unwrap();
    assert_eq!(pip["action"]["version"], "1.4.0");
    assert_eq!(pip["action"]["artifact_url"], install["url"]);
    assert_eq!(pip["action"]["sha256"], install["sha256"]);
}

#[test]
fn godot_plan_explains_why_the_newer_unapproved_release_is_held() {
    let plan = bundled_plan("godot");
    assert_eq!(plan["version"], "0.4.0");
    let policy = &plan["adapter"]["policy"];
    assert_eq!(policy["installation"], "available");
    let reason = policy["reason"].as_str().unwrap();
    assert!(reason.contains("0.8.0"));
    assert!(reason.contains("release-commit CI"));
    assert!(reason.contains("https://github.com/dcc-mcp/dcc-mcp-godot/actions/runs/34250828960"));
    assert_eq!(
        plan["adapter"]["install"]["instructions_url"],
        "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-godot/f8aaf7a3d8d810eec372b5cb77f1f79aa44a236f/README.md"
    );
}
