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
    assert_eq!(plan["version"], "1.5.1");
    let install = &plan["adapter"]["install"];
    assert_eq!(
        install["url"],
        "https://files.pythonhosted.org/packages/50/37/65b42856500d50548d5673a399ccfa1a85a61a7458858bb52881422ed6c2/dcc_mcp_obs-1.5.1-py3-none-any.whl"
    );
    assert_eq!(
        install["sha256"],
        "c47e2127e2331e5b8650a98ff777e97f76cedfccaf0fe2a5187981dfbffe57af"
    );
    assert_eq!(
        install["instructions_url"],
        "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-obs/475046ba823a199195f32ee729983770c2da6898/install.md"
    );
    let pip = plan["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["action"]["type"] == "PipInstall")
        .unwrap();
    assert_eq!(pip["action"]["version"], "1.5.1");
    assert_eq!(pip["action"]["artifact_url"], install["url"]);
    assert_eq!(pip["action"]["sha256"], install["sha256"]);
}

#[test]
fn godot_plan_uses_approved_release_bytes_without_the_resolved_hold() {
    let plan = bundled_plan("godot");
    assert_eq!(plan["version"], "0.9.6");
    assert_eq!(plan["adapter"]["min_core_version"], "0.20.41");
    assert!(plan["adapter"]["policy"]["reason"].is_null());
    let install = &plan["adapter"]["install"];
    assert_eq!(
        install["url"],
        "https://files.pythonhosted.org/packages/c3/f3/4a104c92c6a7b9dd9dc7519d08a8804c50896a1ecc9bbf796a80493f89a6/dcc_mcp_godot-0.9.6-py3-none-any.whl"
    );
    assert_eq!(
        install["sha256"],
        "1aa17201803e800638033e2581002e5d01d00c6ac772d6b54688bf2a1cc384dc"
    );
    assert_eq!(
        install["instructions_url"],
        "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-godot/141a2a2253d72035ef099dab8e81e0f98aa4db80/install.md"
    );
    let pip = plan["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["action"]["type"] == "PipInstall")
        .unwrap();
    assert_eq!(pip["action"]["version"], "0.9.6");
    assert_eq!(pip["action"]["artifact_url"], install["url"]);
    assert_eq!(pip["action"]["sha256"], install["sha256"]);
}
