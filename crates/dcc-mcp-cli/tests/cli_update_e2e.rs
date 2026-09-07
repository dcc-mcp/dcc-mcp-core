mod support;

use serde_json::Value;
use tempfile::TempDir;

use support::*;

#[test]
fn update_check_supports_server_binary_versions() {
    let fixture = spawn_gateway_fixture();

    let update = run_json(&[
        "--base-url",
        &fixture.base_url,
        "update",
        "check",
        "--binary",
        "dcc-mcp-server",
        "--current-version",
        "0.18.16",
    ]);

    assert_eq!(update["update_available"], true);
    assert_eq!(update["current_version"], "0.18.16");
    assert_eq!(update["latest_version"], "0.19.0");
    assert_eq!(
        update["download_url"],
        "https://example.invalid/dcc-mcp-server.zip"
    );
    assert_eq!(
        update["sha256"],
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(update["release_notes"], "Server update");
    assert_eq!(update["version_status"], "update_available");
    assert_eq!(update["check"]["source"], "live");
    assert_eq!(update["compatibility"]["status"], "eligible");
    assert_eq!(
        update["update_policy"]["apply_supported_by_this_command"],
        false
    );
    assert_eq!(update["update_policy"]["running_server_restarted"], false);
}

#[test]
fn update_apply_requires_explicit_user_confirmation() {
    let fixture = spawn_gateway_fixture();

    let output = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "--output",
            "json",
            "update",
            "apply",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let update: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(update["status"], "confirmation_required");
    assert_eq!(update["error"], "confirmation_required");
    assert_eq!(update["version_status"], "update_available");
    assert_eq!(update["current_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(update["latest_version"], "99.0.0");
    assert_eq!(
        update["update_policy"]["apply_command"],
        "dcc-mcp-cli update apply --yes"
    );
    assert_eq!(update["update_policy"]["staged_for_next_launch"], true);
    assert_eq!(update["update_policy"]["running_server_restarted"], false);
}

#[test]
fn update_apply_surfaces_check_failures_without_downloading() {
    let port = unused_loopback_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let output = cli_command()
        .args([
            "--no-auto-gateway",
            "--base-url",
            &base_url,
            "--output",
            "json",
            "update",
            "apply",
            "--yes",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let update: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(update["status"], "check_failed");
    assert_eq!(update["error"], "update_check_failed");
    assert_eq!(update["version_status"], "check_failed");
    assert_eq!(update["update_available"], false);
    assert_eq!(update["update_policy"]["running_server_restarted"], false);
}

#[test]
fn cached_cli_update_is_agent_readable_without_a_network_check() {
    let fixture = spawn_gateway_fixture();
    let cache_root = TempDir::new().unwrap();
    let cache_file = cache_root
        .path()
        .join("dcc-mcp")
        .join("update")
        .join("cli-check.json");
    std::fs::create_dir_all(cache_file.parent().unwrap()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        &cache_file,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "binary_name": "dcc-mcp-cli",
            "current_version": env!("CARGO_PKG_VERSION"),
            "checked_at_unix_secs": now,
            "successful": true,
            "payload": {
                "version_status": "update_available",
                "binary_name": "dcc-mcp-cli",
                "current_version": env!("CARGO_PKG_VERSION"),
                "latest_version": "99.0.0",
                "update_available": true,
                "check": {"source": "live", "cache_age_secs": 0},
                "update_policy": {
                    "requires_user_confirmation": true,
                    "apply_command": "dcc-mcp-cli update apply --yes",
                    "staged_for_next_launch": true,
                    "running_server_restarted": false
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "--output",
            "json",
            "health",
        ])
        .env("DCC_MCP_FORCE_UPDATE_CHECK", "1")
        .env("DCC_MCP_UPDATE_CACHE_DIR", cache_root.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let health: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(health["cli_update"]["version_status"], "update_available");
    assert_eq!(health["cli_update"]["check"]["source"], "cache");
    assert_eq!(
        health["cli_update"]["update_policy"]["requires_user_confirmation"],
        true
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Ask the user before updating"));
}

#[test]
fn update_check_preserves_gateway_error_payload() {
    let fixture = spawn_gateway_fixture();

    let output = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "update",
            "check",
            "--binary",
            "dcc-mcp-cli",
            "--current-version",
            "0.18.16",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "update check should fail when the gateway reports an update error"
    );
    let update: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        update["error"],
        "binary 'dcc-mcp-cli' not found in update manifest"
    );
    assert_eq!(update["binary_name"], "dcc-mcp-cli");
    assert_eq!(update["current_version"], "0.18.16");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("missing field"),
        "stderr should not expose serde decode failures"
    );
}
