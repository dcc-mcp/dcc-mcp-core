//! Standalone verification uses an explicit interpreter, never installation/host lifecycle.
mod support;

use serde_json::{Value, json};
use tempfile::TempDir;

fn target_python() -> String {
    for candidate in std::env::var("PYTHON")
        .into_iter()
        .chain(["python3".into(), "python".into()])
    {
        if let Ok(output) = std::process::Command::new(&candidate)
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            && output.status.success()
        {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return path;
            }
        }
    }
    panic!("the verification process regression requires a Python interpreter");
}

#[test]
fn verify_checks_real_target_import_and_pinned_metadata_without_installation() {
    let python = target_python();
    for (module_source, dist_version, expected_code, expected_reason) in [
        (
            "__version__ = '1.0.0'\n",
            "1.0.0",
            0,
            "LIVE_DCC_VERIFICATION_REQUIRED",
        ),
        (
            "raise RuntimeError('broken editable install')\n",
            "1.0.0",
            40,
            "ADAPTER_IMPORT_FAILED",
        ),
        (
            "__version__ = '0.9.0'\n",
            "0.9.0",
            40,
            "ADAPTER_VERSION_MISMATCH",
        ),
        (
            "__version__ = '0.9.0'\n",
            "1.0.0",
            40,
            "ADAPTER_VERSION_MISMATCH",
        ),
    ] {
        let root = TempDir::new().unwrap();
        let site = root.path().join("site");
        std::fs::create_dir(&site).unwrap();
        std::fs::write(site.join("verify_fixture.py"), format!(
            "{module_source}\ndef create_server():\n    raise RuntimeError('factory must not run')\n"
        )).unwrap();
        let metadata = site.join(format!("dcc_mcp_verifyfixture-{dist_version}.dist-info"));
        std::fs::create_dir(&metadata).unwrap();
        std::fs::write(
            metadata.join("METADATA"),
            format!(
                "Metadata-Version: 2.1\nName: dcc-mcp-verifyfixture\nVersion: {dist_version}\n"
            ),
        )
        .unwrap();
        let catalog = root.path().join("catalog.json");
        std::fs::write(
            &catalog,
            json!({"entries":[{
                "name":"dcc-mcp-verifyfixture", "description":"Verification fixture",
                "dcc":["verify-fixture"], "tags":["adapter"], "version":"1.0.0",
                "install":{"type":"pip","pip_package":"dcc-mcp-verifyfixture",
                    "url":"https://example.invalid/dcc_mcp_verifyfixture-1.0.0-py3-none-any.whl",
                    "sha256":"a".repeat(64),"entry_point":"verify_fixture:create_server"}
            }]})
            .to_string(),
        )
        .unwrap();
        let untouched = root.path().join("registry-and-profiles");
        std::fs::write(&untouched, "not valid registry/profile JSON").unwrap();
        let output = support::cli_command()
            .args([
                "--gateway",
                "must-not-be-opened",
                "verify",
                "--dcc-type",
                "verify-fixture",
                "--python",
                &python,
                "--json",
                "--catalog",
            ])
            .arg(&catalog)
            .env("PYTHONPATH", &site)
            .env("DCC_MCP_REGISTRY_DIR", &untouched)
            .env("DCC_MCP_GATEWAY_PROFILES_FILE", &untouched)
            .env("DCC_MCP_UPDATE_CACHE_DIR", root.path().join("update-cache"))
            .env("DCC_MCP_INSTALL_DISABLED", "true")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected_code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["verify"]["failure_reason"], expected_reason);
        assert_eq!(report["verify"]["directly_usable"], false);
        assert_eq!(report["rollback"]["attempted"], false);
        assert_eq!(report["receipt_path"], Value::Null);
        assert_eq!(report["steps"].as_array().unwrap().len(), 1);
        assert_eq!(report["steps"][0]["id"], "verify");
        assert_eq!(
            std::fs::read_to_string(&untouched).unwrap(),
            "not valid registry/profile JSON"
        );
        assert!(!root.path().join("update-cache").exists());
        assert!(!site.join("__pycache__").exists());
    }
}

#[test]
fn verify_requires_explicit_python_and_rejects_gui_before_any_probe() {
    let missing = support::cli_command()
        .args(["verify", "--dcc-type", "maya"])
        .env_remove("DCC_MCP_INSTALL_PYTHON")
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    let gui = support::cli_command()
        .args([
            "verify",
            "--dcc-type",
            "maya",
            "--python",
            "maya.exe",
            "--json",
        ])
        .env_remove("DCC_MCP_CATALOG_PATH")
        .output()
        .unwrap();
    assert_eq!(gui.status.code(), Some(10));
    let report: Value = serde_json::from_slice(&gui.stdout).unwrap();
    assert_eq!(report["stage"], "preflight");
    assert_eq!(report["rollback"]["attempted"], false);
}
