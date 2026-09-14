use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::TempDir;

fn catalog_entry(dcc: &str) -> Value {
    json!({
        "name": format!("dcc-mcp-{dcc}"),
        "description": "Catalog policy fixture",
        "dcc": [dcc],
        "tags": ["adapter"],
        "version": "1.0.0",
    })
}

fn write_catalog(root: &TempDir, entries: Vec<Value>) -> std::path::PathBuf {
    let path = root.path().join("catalog.json");
    std::fs::write(
        &path,
        json!({"version": "1", "entries": entries}).to_string(),
    )
    .unwrap();
    path
}

fn cli(catalog: &Path, dcc: &str, execute: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"));
    command
        .args(["install", "--dcc-type", dcc, "--catalog"])
        .arg(catalog)
        .arg("--json")
        .env_remove("DCC_MCP_INSTALL_DISABLED")
        .env_remove("DCC_MCP_INSTALL_PYTHON");
    if execute {
        command.arg("--execute");
    }
    command.output().unwrap()
}

#[test]
fn revoked_entries_and_unknown_policies_reject_informational_plans() {
    let root = TempDir::new().unwrap();
    for dcc in ["maya", "photoshop"] {
        for installation in ["not_available", "revoked", "available ", ""] {
            let mut entry = catalog_entry(dcc);
            entry["policy"] = json!({"installation": installation});
            let catalog = write_catalog(&root, vec![entry]);
            let output = cli(&catalog, dcc, false);
            assert!(!output.status.success(), "{dcc}: {installation}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            let expected = if installation == "not_available" {
                "not available for installation"
            } else {
                "unsupported installation policy"
            };
            assert!(stderr.contains(expected), "stderr: {stderr}");
            assert!(!String::from_utf8_lossy(&output.stdout).contains("next_steps"));
        }
    }
}

#[test]
fn incompatible_or_malformed_minimum_core_version_rejects_plans() {
    let root = TempDir::new().unwrap();
    for required in ["999.0.0", "not-semver", "0.19", ">=0.19.0"] {
        let mut entry = catalog_entry("blender");
        entry["min_core_version"] = json!(required);
        let catalog = write_catalog(&root, vec![entry]);
        let output = cli(&catalog, "blender", false);
        assert!(!output.status.success(), "{required}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let expected = if required == "999.0.0" {
            "requires dcc-mcp-core >= 999.0.0"
        } else {
            "invalid min_core_version"
        };
        assert!(stderr.contains(expected), "stderr: {stderr}");
    }
}

#[test]
fn rejected_catalog_entries_stop_execution_in_preflight() {
    let root = TempDir::new().unwrap();
    for policy_fields in [
        json!({"policy": {"installation": "not_available"}}),
        json!({"policy": {"installation": "revoked"}}),
        json!({"min_core_version": "999.0.0"}),
        json!({"min_core_version": "invalid"}),
    ] {
        let mut entry = catalog_entry("studio-editor");
        entry
            .as_object_mut()
            .unwrap()
            .extend(policy_fields.as_object().unwrap().clone());
        entry["install"] = json!({
            "type": "path",
            "url": root.path().join("must-not-be-opened"),
        });
        let catalog = write_catalog(&root, vec![entry]);
        let output = cli(&catalog, "studio-editor", true);
        assert_eq!(output.status.code(), Some(10));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["status"], "failed");
        assert_eq!(report["stage"], "preflight");
        assert_eq!(report["error"]["code"], "INSTALL_PLAN_FAILED");
        assert_eq!(report["steps"], json!([]));
        assert!(report["receipt_path"].is_null());
        assert_eq!(report["verify"]["directly_usable"], false);
        assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
    }
}

#[test]
fn available_entries_accept_the_running_core_version() {
    let root = TempDir::new().unwrap();
    let mut entry = catalog_entry("photoshop");
    entry["policy"] = json!({"installation": "available"});
    entry["min_core_version"] = json!(env!("CARGO_PKG_VERSION"));
    let catalog = write_catalog(&root, vec![entry]);
    let output = cli(&catalog, "photoshop", false);
    assert!(output.status.success(), "{:?}", output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-photoshop");
    assert_eq!(
        plan["adapter"]["min_core_version"],
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn discovery_does_not_advertise_revoked_or_incompatible_installations() {
    let root = TempDir::new().unwrap();
    let mut entries = Vec::new();
    for (dcc, policy_fields) in [
        ("maya", json!({"policy": {"installation": "not_available"}})),
        ("photoshop", json!({"policy": {"installation": "revoked"}})),
        ("blender", json!({"min_core_version": "999.0.0"})),
    ] {
        let mut entry = catalog_entry(dcc);
        entry
            .as_object_mut()
            .unwrap()
            .extend(policy_fields.as_object().unwrap().clone());
        entry["install"] = json!({"type": "path", "url": "unused"});
        entries.push(entry);
    }
    let catalog = write_catalog(&root, entries);
    let output = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
        .args(["dcc-types", "--catalog"])
        .arg(catalog)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let listing: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(listing["total"], 3);
    for dcc in listing["dcc_types"].as_array().unwrap() {
        assert_eq!(dcc["adapters"][0]["catalog_install_available"], false);
    }
}

#[test]
fn targeted_explicit_discovery_keeps_revoked_entries_non_installable() {
    let root = TempDir::new().unwrap();
    let registry = TempDir::new().unwrap();
    let mut entry = catalog_entry("photoshop");
    entry["policy"] = json!({"installation": "not_available"});
    entry["install"] = json!({"type": "path", "url": "unused"});
    let catalog = write_catalog(&root, vec![entry]);
    let output = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
        .args(["dcc-types", "--dcc-type", "photoshop", "--catalog"])
        .arg(catalog)
        .args(["--output", "json"])
        .env("DCC_MCP_REGISTRY_DIR", registry.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let decision: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(decision["live_instances"], 0);
    assert_eq!(decision["next_action"]["id"], "inspect_catalog");
    assert_eq!(decision["catalog"]["source"], "explicit");
    assert_eq!(decision["catalog"]["latest_checked"], false);
    assert_eq!(decision["released_catalog"], "unknown");
}

#[test]
fn corrupt_official_cache_reports_unavailable_without_claiming_bundled_fallback() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join("corrupt-cache.json");
    std::fs::write(&cache, b"not an authenticated envelope").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
        .args([
            "dcc-types",
            "--dcc-type",
            "maya",
            "--offline",
            "--output",
            "json",
        ])
        .env("DCC_MCP_INSTALL_CACHE", &cache)
        .env_remove("DCC_MCP_CATALOG_PATH")
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let decision: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(decision["failure_reason"], "CATALOG_LOAD_FAILED");
    assert_eq!(decision["catalog"]["source"], "unavailable");
    assert_eq!(decision["catalog"]["latest_checked"], false);
    assert_eq!(
        std::fs::read(cache).unwrap(),
        b"not an authenticated envelope"
    );
}

#[test]
fn explicit_source_does_not_consume_an_unusable_official_cache() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join("corrupt-cache.json");
    std::fs::write(&cache, b"not an authenticated envelope").unwrap();
    let catalog = write_catalog(&root, vec![catalog_entry("blender")]);
    let output = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
        .args(["install", "--dcc-type", "blender", "--catalog"])
        .arg(catalog)
        .arg("--json")
        .env("DCC_MCP_INSTALL_CACHE", &cache)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["catalog"]["source"], "explicit");
    assert_eq!(plan["catalog"]["latest_checked"], false);
    assert_eq!(plan["adapter"]["name"], "dcc-mcp-blender");
    assert_eq!(
        std::fs::read(cache).unwrap(),
        b"not an authenticated envelope"
    );
}

#[test]
fn rejected_official_cache_preserves_unavailable_execution_evidence() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join("corrupt-cache.json");
    std::fs::write(&cache, b"not an authenticated envelope").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
        .args([
            "install",
            "--dcc-type",
            "maya",
            "--offline",
            "--execute",
            "--json",
        ])
        .env("DCC_MCP_INSTALL_CACHE", &cache)
        .env_remove("DCC_MCP_CATALOG_PATH")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(10));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["error"]["code"], "INSTALL_CATALOG_UNAVAILABLE");
    assert_eq!(
        report["catalog"],
        json!({"source": "unavailable", "latest_checked": false})
    );
    assert_eq!(report["stage"], "preflight");
    assert_eq!(report["steps"], json!([]));
    assert!(report["receipt_path"].is_null());
    assert_eq!(
        std::fs::read(cache).unwrap(),
        b"not an authenticated envelope"
    );
}
