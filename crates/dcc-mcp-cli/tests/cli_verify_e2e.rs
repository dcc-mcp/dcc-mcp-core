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

#[test]
fn verify_rejects_target_core_below_catalog_minimum() {
    let python = target_python();
    let root = TempDir::new().unwrap();
    let site = root.path().join("site");
    std::fs::create_dir(&site).unwrap();
    std::fs::write(site.join("verify_fixture.py"), "__version__ = '1.0.0'\n").unwrap();
    std::fs::write(site.join("dcc_mcp_core.py"), "__version__ = '0.19.9'\n").unwrap();
    let metadata = site.join("dcc_mcp_verifyfixture-1.0.0.dist-info");
    std::fs::create_dir(&metadata).unwrap();
    std::fs::write(
        metadata.join("METADATA"),
        "Metadata-Version: 2.1\nName: dcc-mcp-verifyfixture\nVersion: 1.0.0\n",
    )
    .unwrap();
    let catalog = root.path().join("catalog.json");
    std::fs::write(
        &catalog,
        json!({"entries":[{
            "name":"dcc-mcp-verifyfixture", "description":"Verification fixture",
            "dcc":["verify-fixture"], "tags":["adapter"], "version":"1.0.0",
            "min_core_version":"0.20.0",
            "install":{"type":"pip","pip_package":"dcc-mcp-verifyfixture",
                "url":"https://example.invalid/dcc_mcp_verifyfixture-1.0.0-py3-none-any.whl",
                "sha256":"a".repeat(64),"entry_point":"verify_fixture:create_server"}
        }]})
        .to_string(),
    )
    .unwrap();

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
        .env("DCC_MCP_INSTALL_DISABLED", "true")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(40));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["error"]["code"], "ADAPTER_CORE_INCOMPATIBLE");
    assert_eq!(
        report["verify"]["observed_core_compatibility"]["requirement"],
        ">=0.20.0,<0.21.0"
    );
    assert_eq!(
        report["verify"]["observed_core_compatibility"]["status"],
        "core_older_than_declared"
    );
}

// Compile-gated CLI process evidence for the combined 2852 + 2862 slice. The
// fixture declares a bounded Core requirement so the probe returns
// incompatible_core. Run it after rebasing 2862 onto the corrected 2852.
#[test]
#[ignore = "requires the combined 2852 + 2862 Core tree"]
fn verify_reports_incompatible_target_core_after_combined_slice() {
    let python = target_python();
    let root = TempDir::new().unwrap();
    let site = root.path().join("site");
    std::fs::create_dir(&site).unwrap();
    std::fs::write(
        site.join("verify_fixture.py"),
        "__version__ = '1.0.0'\nimport dcc_mcp_core\ndef create_server():\n    raise RuntimeError('factory must not run')\n",
    )
    .unwrap();
    std::fs::write(site.join("dcc_mcp_core.py"), "__version__ = '0.22.0'\n").unwrap();
    let metadata = site.join("dcc_mcp_verifyfixture-1.0.0.dist-info");
    std::fs::create_dir(&metadata).unwrap();
    std::fs::write(
        metadata.join("METADATA"),
        "Metadata-Version: 2.1\nName: dcc-mcp-verifyfixture\nVersion: 1.0.0\nRequires-Dist: dcc-mcp-core>=0.20.24,<0.21.0\n\n",
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
        .env("DCC_MCP_INSTALL_DISABLED", "true")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(40));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["error"]["code"], "ADAPTER_CORE_INCOMPATIBLE");
    assert_eq!(
        report["verify"]["failure_reason"],
        "ADAPTER_CORE_INCOMPATIBLE"
    );
    assert_eq!(
        report["verify"]["observed_core_compatibility"]["status"],
        "core_newer_than_declared"
    );
    assert_eq!(report["verify"]["expected_adapter_version"], "1.0.0");
    assert_eq!(report["verify"]["observed_adapter_version"], "1.0.0");
    assert_eq!(
        report["verify"]["observed_distribution_source"],
        "importlib.metadata"
    );
    assert!(!site.join("__pycache__").exists());
}
