//! Executable regressions: inspection must bypass staged updates and maintenance.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;

struct IsolatedCli {
    root: TempDir,
    staging: PathBuf,
}

#[test]
fn fixed_local_component_failures_never_apply_staged_updates_or_open_profiles() {
    for yes in [false, true] {
        let isolated = IsolatedCli::new();
        let manifest = isolated.root.path().join("manifest.json");
        std::fs::write(&manifest, b"{}").unwrap();
        let mut command = isolated.command();
        command.args([
            "--gateway",
            "missing-profile-must-not-be-opened",
            "components",
            "install-verified",
            "dcc-cua",
            "--target-core-cli",
            "missing/dcc-mcp-cli",
            "--expected-core-cli-sha256",
            &"a".repeat(64),
            "--manifest",
            manifest.to_str().unwrap(),
            "--manifest-sha256",
            &"a".repeat(64),
            "--previous-manifest",
            "missing/previous.json",
            "--previous-manifest-sha256",
            &"a".repeat(64),
            "--source-root",
            "missing/source",
            "--version",
            "1.11.1",
            "--expected-current-sha256",
            &"a".repeat(64),
            "--expected-new-sha256",
            &"b".repeat(64),
        ]);
        if yes {
            command.arg("--yes");
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(if yes {
                "qualified install manifest SHA-256 mismatch"
            } else {
                "explicit --yes"
            }),
            "{error}"
        );
        isolated.assert_staging_untouched();
        assert!(!isolated.cache_root().exists());
    }
}

impl IsolatedCli {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = if cfg!(target_os = "macos") {
            root.path().join("Library/Application Support")
        } else {
            root.path().to_owned()
        };
        let staging = data.join("update/dcc-mcp-cli");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("pending.marker"), b"sentinel-marker").unwrap();
        std::fs::write(staging.join("pending.bin"), b"sentinel-binary").unwrap();
        Self { root, staging }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"));
        command
            .env("APPDATA", self.root.path())
            .env("XDG_DATA_HOME", self.root.path())
            .env("HOME", self.root.path())
            .env("DCC_MCP_UPDATE_CACHE_DIR", self.cache_root())
            .env("DCC_MCP_FORCE_UPDATE_CHECK", "1")
            .env_remove("DCC_MCP_DISABLE_UPDATE_CHECK")
            .env_remove("DCC_MCP_UPDATE_BACKGROUND")
            .env_remove("DCC_MCP_OUTPUT")
            .env_remove("DCC_MCP_GATEWAY_PROFILE")
            .env_remove("DCC_MCP_TIMEOUT_SECS");
        command
    }

    fn cache_root(&self) -> PathBuf {
        self.root.path().join("inspection-cache")
    }

    fn assert_staging_untouched(&self) {
        assert_eq!(
            std::fs::read(self.staging.join("pending.marker")).unwrap(),
            b"sentinel-marker"
        );
        assert_eq!(
            std::fs::read(self.staging.join("pending.bin")).unwrap(),
            b"sentinel-binary"
        );
        assert_eq!(std::fs::read_dir(&self.staging).unwrap().count(), 2);
    }
}

#[test]
fn isolated_staging_fixture_is_observed_by_normal_startup() {
    let isolated = IsolatedCli::new();
    let output = isolated
        .command()
        .args([
            "--gateway",
            "read-only-test-missing-profile",
            "--no-auto-gateway",
            "health",
        ])
        .env("DCC_MCP_UPDATE_BACKGROUND", "1")
        .env_remove("DCC_MCP_UPDATE_JUST_APPLIED")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!isolated.staging.join("pending.marker").exists());
    assert!(
        std::fs::read_dir(&isolated.staging)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("legacy-unsigned-"))
    );
}

#[test]
fn ordinary_inventory_opt_out_preserves_pending_and_update_cache() {
    let isolated = IsolatedCli::new();
    let registry = isolated.root.path().join("private-registry");
    std::fs::create_dir_all(&registry).unwrap();
    let catalog = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    catalog.set_nonblocking(true).unwrap();
    let marketplace = isolated.root.path().join("marketplace");
    std::fs::create_dir_all(&marketplace).unwrap();
    let ledger = marketplace.join("installed.json");
    let installed = serde_json::to_vec(&json!({ "packages": [{
        "name": "fixture-tools", "dcc": "maya", "version": "1.0.0",
        "path": marketplace.join("fixture-tools"), "source_name": "fixture",
        "source_url": format!("http://{}/marketplace.json", catalog.local_addr().unwrap()),
        "install_type": "git", "install_url": null, "install_ref": null,
        "installed_at_ms": 1
    }] }))
    .unwrap();
    std::fs::write(&ledger, &installed).unwrap();
    let output = isolated
        .command()
        .args([
            "--no-update-maintenance",
            "--no-auto-gateway",
            "--gateway",
            "local",
            "--output",
            "json",
            "list",
        ])
        .env("DCC_MCP_REGISTRY_DIR", &registry)
        .env_remove("DCC_MCP_BASE_URL")
        .env("DCC_MCP_MARKETPLACE_INSTALL_ROOT", &marketplace)
        .env("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1")
        .env(
            "DCC_MCP_GATEWAY_PROFILES_FILE",
            isolated.root.path().join("profiles.json"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["total"], 0);
    assert!(report.get("cli_update").is_none());
    assert!(report.get("marketplace_updates").is_none());
    isolated.assert_staging_untouched();
    assert!(!isolated.cache_root().exists());
    assert_eq!(std::fs::read(&ledger).unwrap(), installed);
    match catalog.accept() {
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock),
        Ok(_) => panic!("incidental marketplace update check contacted the fixture catalog"),
    }
}

#[test]
fn doctor_preserves_staging_and_update_cache_without_opt_out() {
    for pending in [true, false] {
        assert_doctor_preserves_update_state(pending);
    }
}

fn assert_doctor_preserves_update_state(pending: bool) {
    let isolated = IsolatedCli::new();
    if !pending {
        std::fs::remove_file(isolated.staging.join("pending.marker")).unwrap();
        std::fs::remove_file(isolated.staging.join("pending.bin")).unwrap();
    }
    let missing_binary = isolated.root.path().join("missing-doctor-fixture-binary");
    let registry = isolated.root.path().join("private-registry");
    let catalog = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    catalog.set_nonblocking(true).unwrap();
    let marketplace = isolated.root.path().join("marketplace");
    std::fs::create_dir_all(&marketplace).unwrap();
    let ledger = marketplace.join("installed.json");
    let installed = serde_json::to_vec(&json!({ "packages": [{
        "name": "fixture-tools", "dcc": "maya", "version": "1.0.0",
        "path": marketplace.join("fixture-tools"), "source_name": "fixture",
        "source_url": format!("http://{}/marketplace.json", catalog.local_addr().unwrap()),
        "install_type": "git", "install_url": null, "install_ref": null,
        "installed_at_ms": 1
    }] }))
    .unwrap();
    std::fs::write(&ledger, &installed).unwrap();
    let output = isolated
        .command()
        .args([
            "--gateway",
            "local",
            "--output",
            "json",
            "--auto-gateway-bin",
            missing_binary.to_str().unwrap(),
            "doctor",
            "--gateway-port",
            "0",
        ])
        .env("DCC_MCP_REGISTRY_DIR", &registry)
        .env("XDG_CONFIG_HOME", isolated.root.path().join("config"))
        .env("XDG_CACHE_HOME", isolated.root.path().join("cache"))
        .env("XDG_STATE_HOME", isolated.root.path().join("state"))
        .env("LOCALAPPDATA", isolated.root.path())
        .env("USERPROFILE", isolated.root.path())
        .env_remove("DCC_MCP_BASE_URL")
        .env("DCC_MCP_PYTHON_EXECUTABLE", "")
        .env("DCC_MCP_MARKETPLACE_INSTALL_ROOT", &marketplace)
        .env(
            "DCC_MCP_MARKETPLACE_SOURCES_FILE",
            isolated.root.path().join("sources.json"),
        )
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .env("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1")
        .env(
            "DCC_MCP_GATEWAY_PROFILES_FILE",
            isolated.root.path().join("profiles.json"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["local"]["inventory"]["mode"], "read_only_snapshot");
    assert_eq!(report["local"]["inventory"]["total"], 0);
    assert!(!registry.exists());
    assert!(!isolated.root.path().join("profiles.json").exists());
    for directory in ["config", "cache", "state"] {
        assert!(!isolated.root.path().join(directory).exists());
    }
    assert!(report.get("cli_update").is_none());
    assert!(report.get("marketplace_updates").is_none());
    if pending {
        isolated.assert_staging_untouched();
    } else {
        assert_eq!(std::fs::read_dir(&isolated.staging).unwrap().count(), 0);
    }
    assert!(!isolated.cache_root().exists());
    assert_eq!(std::fs::read(&ledger).unwrap(), installed);
    match catalog.accept() {
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock),
        Ok(_) => panic!("incidental marketplace update check contacted the fixture catalog"),
    }
}

#[test]
fn offline_inspection_bypasses_gateway_staged_update_and_cache_writes() {
    let isolated = IsolatedCli::new();
    let output = isolated
        .command()
        .args([
            "--gateway",
            "profile-that-does-not-exist",
            "--require-gateway",
            "--base-url",
            "http://127.0.0.1:1",
            "update",
            "inspect",
            "--offline",
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], "dcc-mcp.update-inspection.v1");
    assert_eq!(report["read_only"], true);
    assert_eq!(report["version_status"], "unknown");
    assert_eq!(
        report["versions"]["runtime"]["source"],
        "executing_cli_build"
    );
    assert_eq!(
        report["versions"]["runtime"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert!(report.get("cli_update").is_none());
    isolated.assert_staging_untouched();
    assert!(!isolated.cache_root().exists());
}

#[test]
fn malformed_inspection_and_help_never_enter_legacy_startup() {
    for (args, expected) in [
        (vec!["update", "inspect", "--channel"], 2),
        (vec!["update", "inspect", "--help"], 0),
        (vec!["--output", "invalid", "update", "inspect"], 2),
    ] {
        let isolated = IsolatedCli::new();
        let output = isolated.command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(expected));
        isolated.assert_staging_untouched();
        assert!(!isolated.cache_root().exists());
    }
}

#[test]
fn offline_cache_is_unchanged_and_does_not_inherit_fix_claims() {
    let isolated = IsolatedCli::new();
    let cache = isolated.cache_root().join("dcc-mcp/update/cli-check.json");
    write_cache(&cache);
    let before = std::fs::read(&cache).unwrap();
    let output = isolated
        .command()
        .args([
            "--output",
            "json",
            "update",
            "inspect",
            "--offline",
            "--issue",
            "https://github.com/dcc-mcp/dcc-mcp-core/issues/42",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["latest_version"], "99.0.0");
    assert_eq!(report["source"]["provider"], "gateway_update_cache");
    assert_eq!(report["source"]["ttl_secs"], 86400);
    assert_eq!(
        report["source"]["reason"],
        "official_release_provenance_not_recorded"
    );
    assert_eq!(report["fix"]["status"], "unknown");
    assert_eq!(std::fs::read(&cache).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(cache.parent().unwrap()).unwrap().count(),
        1
    );
    isolated.assert_staging_untouched();
}

fn write_cache(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(&json!({
        "schema_version": 1, "binary_name": "dcc-mcp-cli", "current_version": env!("CARGO_PKG_VERSION"),
        "checked_at_unix_secs": 100, "successful": true,
        "payload": { "latest_version": "99.0.0", "release_notes": "Fixes #42", "update_available": true }
    })).unwrap()).unwrap();
}

#[test]
fn diagnostic_bundle_bypasses_startup_and_implicit_probes() {
    let isolated = IsolatedCli::new();
    let finding_path = isolated.root.path().join("finding.json");
    let finding = json!({
        "schema_version": 1, "fingerprint": format!("sha256:{}", "a".repeat(64)),
        "dcc_type": "godot", "adapter": "dcc-mcp-godot", "adapter_version": "0.3.0+studio_private",
        "core_version": "0.20.11", "host_version": "4.4.1", "os": "windows",
        "phase": "startup", "severity": "blocker", "tool_slug": null,
        "intent": "studio_private", "observed": "studio_private", "expected": "studio_private",
        "repro": {"argv": ["dcc-mcp-cli", "status"], "steps": []}, "evidence": {"error_kind": "startup_failed"},
        "redaction_status": {
            "mode": "public-safe", "redaction_markers_detected": false,
            "raw_payloads_excluded": true, "prompts_excluded": true, "scripts_excluded": true,
            "auth_material_excluded": true, "local_urls_excluded": true,
            "absolute_paths_excluded": true, "private_identifiers_excluded": true
        }
    });
    std::fs::write(&finding_path, serde_json::to_vec(&finding).unwrap()).unwrap();
    let before = std::fs::read(&finding_path).unwrap();
    let output = isolated
        .command()
        .args([
            "--gateway",
            "profile-that-does-not-exist",
            "--require-gateway",
            "--base-url",
            "http://127.0.0.1:1",
            "feedback",
            "bundle",
        ])
        .arg(&finding_path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], "dcc-mcp.feedback-bundle.v2");
    assert_eq!(
        report["components"]["doctor"]["reason"],
        "not_collected_by_default"
    );
    assert_eq!(
        report["components"]["host_errors"]["reason"],
        "host_errors_not_collected_by_default"
    );
    assert_eq!(report["redaction_status"]["secret_free_guarantee"], false);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("studio_private"));
    assert_eq!(std::fs::read(finding_path).unwrap(), before);
    isolated.assert_staging_untouched();
    assert!(!isolated.cache_root().exists());
}

#[test]
fn invalid_feedback_plan_cannot_enter_startup_or_contact_tracker() {
    let isolated = IsolatedCli::new();
    let output = isolated
        .command()
        .args(["feedback", "file", "absent-private-file.json", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let report: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(
        serde_json::to_string(&report)
            .unwrap()
            .contains("FEEDBACK_PREVIEW_FAILED")
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("absent-private-file"));
    isolated.assert_staging_untouched();
    assert!(!isolated.cache_root().exists());
}
