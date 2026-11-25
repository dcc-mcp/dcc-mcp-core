//! Install SOP v1 full-lifecycle chain: install -> verify -> uninstall.
//!
//! `directly_usable` is promoted only when a registered live instance answers
//! a complete direct readiness probe. The no-host case must stay unverified.

mod support;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use serde_json::{Value, json};
use tempfile::TempDir;

use support::*;

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
    panic!("the lifecycle chain regression requires a Python interpreter");
}

fn fixture_catalog(root: &TempDir) -> std::path::PathBuf {
    let catalog = root.path().join("catalog.json");
    std::fs::write(
        &catalog,
        json!({"entries":[{
            "name":"dcc-mcp-chainfixture", "description":"Lifecycle chain fixture",
            "dcc":["chainfixture"], "tags":["adapter"], "version":"1.0.0",
            "install":{"type":"pip","pip_package":"dcc-mcp-chainfixture",
                "url":"https://example.invalid/dcc_mcp_chainfixture-1.0.0-py3-none-any.whl",
                "sha256":"f36b78f6b00e4ffc0ca0ae15c97cc8653705681563fe446176452b3464f11b33","entry_point":"verify_fixture:create_server"}
        }]})
        .to_string(),
    )
    .unwrap();
    catalog
}

fn site_with_adapter(root: &TempDir) -> std::path::PathBuf {
    let site = root.path().join("site");
    std::fs::create_dir(&site).unwrap();
    std::fs::write(site.join("verify_fixture.py"), "__version__ = '1.0.0'\n").unwrap();
    let core = site.join("dcc_mcp_core");
    std::fs::create_dir(&core).unwrap();
    std::fs::write(core.join("__init__.py"), "__version__ = '0.20.53'\n").unwrap();
    // The install path invokes Core's import-light lifecycle module through
    // the target interpreter. Keep the fixture package-shaped so that this
    // module can coexist with the Core version probe.
    std::fs::write(
        core.join("install_lifecycle.py"),
        r#"import json
import sys

if sys.argv[1:2] != ['prepare-update']:
    raise SystemExit(2)

print(json.dumps({"success": True, "status": "ready"}))
"#,
    )
    .unwrap();
    let metadata = site.join("dcc_mcp_chainfixture-1.0.0.dist-info");
    std::fs::create_dir(&metadata).unwrap();
    std::fs::write(
        metadata.join("METADATA"),
        "Metadata-Version: 2.1\nName: dcc-mcp-chainfixture\nVersion: 1.0.0\n",
    )
    .unwrap();
    site
}

/// A metadata-only ready row: dispatch_ready controls whether the chain may
/// promote the verify verdict, `ready` controls the direct readyz evidence.
fn register_instance(registry_dir: &TempDir, mcp_url: &str, dispatch_ready: bool) -> FileRegistry {
    let registry = FileRegistry::new(registry_dir.path()).unwrap();
    let mut entry = ServiceEntry::new("chainfixture", "127.0.0.1", 18081);
    entry.metadata.insert("mcp_url".into(), mcp_url.into());
    if dispatch_ready {
        entry
            .metadata
            .insert("dispatch_status".into(), "ready".into());
    } else {
        entry
            .metadata
            .insert("dispatch_status".into(), "pending".into());
    }
    registry.register(entry).unwrap();
    registry
}

fn verify_chain(
    catalog: &std::path::Path,
    python: &str,
    site: &std::path::Path,
    registry: &TempDir,
) -> Value {
    let output = cli_command()
        .args([
            "--gateway",
            "must-not-be-opened",
            "verify",
            "--dcc-type",
            "chainfixture",
            "--python",
            python,
            "--json",
            "--catalog",
        ])
        .arg(catalog)
        .env("PYTHONPATH", site)
        .env("DCC_MCP_REGISTRY_DIR", registry.path())
        .env("DCC_MCP_INSTALL_DISABLED", "true")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn install_verify_uninstall_chain_promotes_only_with_live_readiness() {
    let python = target_python();
    let root = TempDir::new().unwrap();
    let catalog = fixture_catalog(&root);
    let site = site_with_adapter(&root);

    // No registered instance: static checks pass but the verdict stays partial.
    let empty_registry = TempDir::new().unwrap();
    let unverified = verify_chain(&catalog, &python, &site, &empty_registry);
    assert_eq!(unverified["verify"]["directly_usable"], false);
    assert_eq!(
        unverified["verify"]["failure_reason"],
        "LIVE_DCC_VERIFICATION_REQUIRED"
    );
    assert_eq!(unverified["status"], "partial");
    assert_eq!(unverified["steps"][0]["id"], "verify");

    // A dispatch-ready instance whose readyz reports all required fields
    // promotes the same static result to a usable verdict.
    let live = spawn_local_mcp_fixture();
    let live_registry = TempDir::new().unwrap();
    let _registry = register_instance(&live_registry, &live.mcp_url(), true);
    let ready = verify_chain(&catalog, &python, &site, &live_registry);
    assert_eq!(ready["verify"]["directly_usable"], true);
    assert_eq!(ready["verify"]["failure_reason"], Value::Null);
    assert_eq!(ready["verify"]["failure_stage"], Value::Null);
    assert_eq!(ready["status"], "ok");

    // Uninstall remains receipt-driven and never claims a host effect; the
    // verify verdict does not relax its ownership gate.
    let receipt_root = TempDir::new().unwrap();
    let output = cli_command()
        .args([
            "uninstall",
            "--dcc-type",
            "chainfixture",
            "--python",
            &python,
            "--json",
            "--yes",
            "--catalog",
        ])
        .arg(&catalog)
        .env(
            "DCC_MCP_INSTALL_RECEIPT_DIR",
            receipt_root.path().join("receipts"),
        )
        .env("PYTHONPATH", &site)
        .output()
        .unwrap();
    let uninstall: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(uninstall["stage"], "uninstall");
    assert!(
        uninstall["verify"]["failure_stage"].is_null()
            || uninstall["verify"]["failure_stage"].as_str() == Some("uninstall")
    );
}

#[test]
fn registered_but_unready_instance_stays_unverified() {
    let python = target_python();
    let root = TempDir::new().unwrap();
    let catalog = fixture_catalog(&root);
    let site = site_with_adapter(&root);
    let live = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let _guard = register_instance(&registry, &live.mcp_url(), false);

    let report = verify_chain(&catalog, &python, &site, &registry);
    assert_eq!(report["verify"]["directly_usable"], false);
    assert_eq!(report["status"], "partial");
}

/// `install --execute` must also promote `directly_usable` from a real live
/// readiness probe, without flipping the deferred register-dcc step to ok.
#[test]
fn install_execute_promotes_directly_usable_with_live_readiness() {
    let python = target_python();
    let root = TempDir::new().unwrap();
    let catalog = fixture_catalog(&root);
    let site = site_with_adapter(&root);

    // Minimal fake pip so the execute path can run fully offline: report the
    // pinned package as installed and record that the install ran once.
    let package = site.join("pip");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(package.join("__init__.py"), "").unwrap();
    std::fs::write(
        package.join("__main__.py"),
        r#"import json
import os
import sys
root = os.path.dirname(os.path.dirname(__file__))
if sys.argv[1:2] == ['show']:
    print('Name: dcc-mcp-chainfixture')
    print('Version: 1.0.0')
    print('Location: ' + root)
elif sys.argv[1:2] == ['install']:
    pass
elif sys.argv[1:2] == ['download']:
    dest = sys.argv[sys.argv.index('--dest') + 1]
    with open(os.path.join(dest, 'dcc_mcp_chainfixture-1.0.0-py3-none-any.whl'), 'wb') as stream:
        stream.write(b'fixture-wheel')
"#,
    )
    .unwrap();
    let dist = site.join("dcc_mcp_chainfixture-1.0.0.dist-info");
    std::fs::write(
        dist.join("direct_url.json"),
        json!({
            "url": "https://example.invalid/dcc_mcp_chainfixture-1.0.0-py3-none-any.whl",
            "archive_info": {"hashes": {"sha256": "f36b78f6b00e4ffc0ca0ae15c97cc8653705681563fe446176452b3464f11b33"}}
        })
        .to_string(),
    )
    .unwrap();

    let live = spawn_local_mcp_fixture();
    let registry = TempDir::new().unwrap();
    let _guard = register_instance(&registry, &live.mcp_url(), true);
    let receipt_dir = root.path().join("receipts");

    let output = cli_command()
        .args([
            "install",
            "--dcc-type",
            "chainfixture",
            "--python",
            &python,
            "--execute",
            "--json",
            "--catalog",
        ])
        .arg(&catalog)
        .env("PYTHONPATH", &site)
        .env("DCC_MCP_REGISTRY_DIR", registry.path())
        .env("DCC_MCP_INSTALL_RECEIPT_DIR", &receipt_dir)
        .env_remove("DCC_MCP_INSTALL_DISABLED")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["verify"]["directly_usable"], true);
    assert_eq!(report["verify"]["failure_reason"], Value::Null);
    assert_eq!(report["verify"]["failure_stage"], Value::Null);
    // The live probe promotes usability but must not fake host registration.
    assert_eq!(report["status"], "partial");
    assert!(
        report["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["id"] == "register-dcc" && step["status"] == "deferred")
    );
}
