mod support;

use serde_json::Value;

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
