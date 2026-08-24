mod support;

use serde_json::{Value, json};

use support::*;

#[test]
fn gateway_feedback_cli_works_without_discovering_a_live_instance() {
    let fixture = spawn_gateway_fixture();
    let output = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "--output",
            "json",
            "feedback",
            "--tool-name",
            "houdini.ui_control__act",
            "--intent",
            "Open the render menu",
            "--attempt",
            "Invoked the semantic action",
            "--blocker",
            "The instance exited",
            "--severity",
            "blocked",
            "--dcc-type",
            "houdini",
            "--instance-id",
            "deadbeef",
            "--request-id",
            "request-42",
        ])
        .env("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["feedback_id"], "11111111-1111-4111-8111-111111111111");
    assert_eq!(body["report"]["instance_id"], "deadbeef");
    assert_eq!(body["report"]["request_id"], "request-42");
}

#[test]
fn gateway_feedback_list_queries_persisted_records() {
    let fixture = spawn_gateway_fixture();
    let output = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "feedback",
            "list",
            "--range",
            "24h",
            "--dcc",
            "houdini",
            "--severity",
            "blocked",
            "--limit",
            "25",
            "--json",
        ])
        .env("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["entries"][0]["id"], "feedback-42");
    assert_eq!(body["filters"]["range"], "24h");
    assert_eq!(body["filters"]["dcc"], "houdini");
    assert_eq!(body["filters"]["severity"], "blocked");
    assert_eq!(body["filters"]["limit"], "25");
}

#[test]
fn gateway_feedback_export_uses_bounded_export_default() {
    let fixture = spawn_gateway_fixture();
    let output = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "feedback",
            "export",
            "--json",
        ])
        .env("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["filters"]["range"], "7d");
    assert_eq!(body["filters"]["limit"], "1000");
}

#[test]
fn feedback_route_resolves_adapter_issue_tracker_without_a_gateway() {
    let temp = tempfile::tempdir().unwrap();
    let finding_path = temp.path().join("finding.json");
    std::fs::write(
        &finding_path,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "fingerprint": format!("sha256:{}", "a".repeat(64)),
            "dcc_type": "godot",
            "adapter": "dcc-mcp-godot",
            "adapter_version": "0.1.9",
            "core_version": "0.20.11",
            "host_version": "4.4.1",
            "os": "windows",
            "phase": "install",
            "severity": "blocker",
            "intent": "Install the Godot adapter",
            "observed": "The add-on was not enabled",
            "expected": "The adapter starts with the project",
            "repro": {"argv": ["dcc-mcp-cli", "install", "godot"]},
            "evidence": {"error_kind": "install_failed"},
            "redaction_status": {
                "mode": "needs-review",
                "redaction_markers_detected": false
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let finding_arg = finding_path.to_string_lossy().into_owned();

    let output = cli_command()
        .args(["--output", "json", "feedback", "route", &finding_arg])
        .current_dir(temp.path())
        .env("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["repo"], "dcc-mcp/dcc-mcp-godot");
    assert_eq!(
        body["issues_url"],
        "https://github.com/dcc-mcp/dcc-mcp-godot/issues"
    );
    assert_eq!(body["rationale"], "adapter_phase");
}

#[test]
fn feedback_bundle_assembles_public_safe_bounded_evidence() {
    let fixture = spawn_gateway_fixture();
    let temp = tempfile::tempdir().unwrap();
    let finding_path = temp.path().join("finding.json");
    let log_dir = temp.path().join("logs");
    std::fs::create_dir(&log_dir).unwrap();
    std::fs::write(
        &finding_path,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "fingerprint": format!("sha256:{}", "b".repeat(64)),
            "dcc_type": "godot",
            "adapter": "dcc-mcp-godot",
            "adapter_version": "0.3.0",
            "core_version": "0.20.11",
            "host_version": "4.4.1",
            "os": "windows",
            "phase": "startup",
            "severity": "blocker",
            "intent": "Start the Godot adapter",
            "observed": "The bridge did not start",
            "expected": "The bridge becomes ready",
            "repro": {"argv": ["dcc-mcp-cli", "status"]},
            "evidence": {
                "request_id": "request-42",
                "error_kind": "startup_failed",
                "dcc_pid": 4321
            },
            "redaction_status": {
                "mode": "public-safe",
                "redaction_markers_detected": false,
                "raw_payloads_excluded": true,
                "prompts_excluded": true,
                "scripts_excluded": true,
                "auth_material_excluded": true,
                "local_urls_excluded": true,
                "absolute_paths_excluded": true,
                "private_identifiers_excluded": true
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        log_dir.join("dcc-mcp-godot.4321.host-errors.log"),
        format!(
            "2026-08-24T00:00:00Z ERROR dcc_mcp_core.host_errors: {}",
            json!({
                "event": "dcc_host_error",
                "dcc_type": "godot",
                "dcc_pid": 4321,
                "phase": "bootstrap",
                "level": "error",
                "message": "C:\\studio\\secret.godot token=private",
                "traceback": "private traceback",
                "core_version": "0.20.11"
            })
        ),
    )
    .unwrap();
    let finding_arg = finding_path.to_string_lossy().into_owned();
    let log_dir_arg = log_dir.to_string_lossy().into_owned();

    let output = cli_command()
        .args([
            "--base-url",
            &fixture.base_url,
            "feedback",
            "bundle",
            &finding_arg,
            "--log-dir",
            &log_dir_arg,
            "--host-error-lines",
            "1",
            "--json",
        ])
        .current_dir(temp.path())
        .env("DCC_MCP_CLI_NO_AUTO_GATEWAY", "true")
        .env("DCC_MCP_REGISTRY_DIR", temp.path().join("registry"))
        .env(
            "DCC_MCP_GATEWAY_PROFILES_FILE",
            temp.path().join("profiles.json"),
        )
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    let encoded = serde_json::to_string(&body).unwrap();
    assert_eq!(body["schema_version"], "dcc-mcp.feedback-bundle.v1");
    assert_eq!(body["privacy_mode"], "public-safe");
    assert_eq!(body["complete"], false);
    assert_eq!(body["components"]["issue_report"]["status"], "included");
    assert_eq!(body["components"]["doctor"]["status"], "included");
    assert_eq!(body["components"]["host_errors"]["status"], "included");
    assert_eq!(
        body["components"]["install_execution_report"]["status"],
        "unavailable"
    );
    for private in [
        "secret.godot",
        "token=private",
        "private traceback",
        "4321",
        &finding_arg,
        &log_dir_arg,
        &fixture.base_url,
    ] {
        assert!(!encoded.contains(private), "bundle leaked {private}");
    }
}
