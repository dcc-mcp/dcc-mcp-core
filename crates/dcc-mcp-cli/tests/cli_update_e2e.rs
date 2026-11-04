mod support;

use axum::{Json, Router, routing::get};
use serde_json::Value;
use tempfile::TempDir;
use tokio::sync::oneshot;

use support::*;

/// A gateway that advertises a CLI update and also serves the release binary,
/// with a manifest digest computed from those exact bytes so the updater's
/// SHA-256 verification can pass end to end.
struct DownloadableUpdateFixture {
    base_url: String,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for DownloadableUpdateFixture {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn spawn_downloadable_update_fixture() -> DownloadableUpdateFixture {
    const BINARY: &[u8] = b"dcc-mcp-cli staged update fixture binary v99";
    let sha = sha256_hex(BINARY);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let download_url = format!("http://{addr}/binary");

    let check_sha = sha.clone();
    let check_url = download_url.clone();
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"ok": true})) }),
        )
        .route(
            "/v1/update/check",
            get(move || {
                let sha = check_sha.clone();
                let url = check_url.clone();
                async move {
                    Json(serde_json::json!({
                        "update_available": true,
                        "latest_version": "99.0.0",
                        "download_url": url,
                        "sha256": sha,
                        "release_notes": "CLI update"
                    }))
                }
            }),
        )
        .route("/binary", get(|| async { BINARY.to_vec() }));

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
    });

    DownloadableUpdateFixture {
        base_url: format!("http://{addr}"),
        shutdown: Some(shutdown_tx),
        thread: Some(thread),
    }
}

/// Run an update command with every per-user data directory redirected into a
/// throwaway `TempDir`, so a successful download + stage cannot write a pending
/// update into the developer's real installation.
fn run_staged_update(
    fixture: &DownloadableUpdateFixture,
    data: &TempDir,
    action_args: &[&str],
) -> std::process::Output {
    let appdata = data.path().join("appdata");
    let xdg_data = data.path().join("xdg-data");
    let home = data.path().join("home");
    let mut args = vec![
        "--no-auto-gateway",
        "--no-update-maintenance",
        "--base-url",
        fixture.base_url.as_str(),
        "--output",
        "json",
    ];
    args.extend_from_slice(action_args);
    cli_command()
        .args(&args)
        .env("APPDATA", &appdata)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("HOME", &home)
        .output()
        .unwrap()
}

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
fn update_check_advertises_the_one_command_update_policy() {
    let fixture = spawn_gateway_fixture();
    let cache = TempDir::new().unwrap();
    let cache_dir = cache.path().to_str().unwrap();

    let update = run_json_with_env(
        &[
            "--base-url",
            &fixture.base_url,
            "update",
            "check",
            "--binary",
            "dcc-mcp-cli",
            "--current-version",
            env!("CARGO_PKG_VERSION"),
        ],
        &[("DCC_MCP_UPDATE_CACHE_DIR", cache_dir)],
    );

    assert_eq!(update["update_available"], true);
    assert_eq!(update["latest_version"], "99.0.0");
    assert_eq!(update["version_status"], "update_available");
    assert_eq!(update["update_policy"]["requires_user_confirmation"], false);
    assert_eq!(
        update["update_policy"]["apply_supported_by_this_command"],
        true
    );
    assert_eq!(
        update["update_policy"]["apply_command"],
        "dcc-mcp-cli update"
    );
    assert_eq!(update["update_policy"]["staged_for_next_launch"], true);
    assert_eq!(update["update_policy"]["running_server_restarted"], false);
}

#[test]
fn bare_update_downloads_verifies_and_stages_in_one_step() {
    let fixture = spawn_downloadable_update_fixture();
    let data = TempDir::new().unwrap();

    let output = run_staged_update(&fixture, &data, &["update"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let update: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(update["status"], "staged");
    assert_eq!(update["version_status"], "staged");
    assert_eq!(update["update_available"], true);
    assert_eq!(update["current_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(update["latest_version"], "99.0.0");
    assert_eq!(update["update_policy"]["requires_user_confirmation"], false);
    assert_eq!(
        update["update_policy"]["apply_command"],
        "dcc-mcp-cli update"
    );
    assert_eq!(update["update_policy"]["staged_for_next_launch"], true);
    assert_eq!(update["update_policy"]["running_server_restarted"], false);
}

#[test]
fn update_apply_without_yes_no_longer_requires_confirmation() {
    let fixture = spawn_downloadable_update_fixture();
    let data = TempDir::new().unwrap();

    // Previously `update apply` without `--yes` short-circuited with
    // `confirmation_required` (exit code 2) before any network call. It now
    // runs the full check -> download -> stage flow like the bare command.
    let output = run_staged_update(&fixture, &data, &["update", "apply"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let update: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_ne!(update["status"], "confirmation_required");
    assert_eq!(update["status"], "staged");
    assert_eq!(update["version_status"], "staged");
    assert_eq!(update["update_policy"]["requires_user_confirmation"], false);
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
                    "requires_user_confirmation": false,
                    "apply_supported_by_this_command": true,
                    "apply_command": "dcc-mcp-cli update",
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
        false
    );
    assert_eq!(
        health["cli_update"]["update_policy"]["apply_command"],
        "dcc-mcp-cli update"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Run `dcc-mcp-cli update`"),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains("Ask the user before updating"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("update apply --yes"), "stderr: {stderr}");

    for show in [false, true] {
        let mut command = cli_command();
        command.args([
            "--base-url",
            &fixture.base_url,
            "--output",
            "json",
            "--no-auto-gateway",
            "list",
        ]);
        if show {
            command.arg("--show-maintenance");
        }
        let output = command
            .env("DCC_MCP_FORCE_UPDATE_CHECK", "1")
            .env("DCC_MCP_UPDATE_CACHE_DIR", cache_root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value.get("cli_update").is_some(), show);
        assert_eq!(
            String::from_utf8_lossy(&output.stderr).contains("Run `dcc-mcp-cli update`"),
            show
        );
    }
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
