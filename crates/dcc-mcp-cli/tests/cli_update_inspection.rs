//! Executable regressions: inspection must bypass staged updates and maintenance.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use tempfile::TempDir;

struct IsolatedCli {
    root: TempDir,
    staging: PathBuf,
}

impl IsolatedCli {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let data = if cfg!(target_os = "macos") {
            root.path().join("Library/Application Support")
        } else {
            root.path().to_owned()
        };
        let staging = data.join("dcc-mcp/update/dcc-mcp-cli");
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
