mod support;

use support::{run_failure_with_env, run_json, spawn_gateway_fixture};

#[test]
fn selectable_cli_transport_uses_mcp_or_strict_rest() {
    let fixture = spawn_gateway_fixture();
    let explicit_mcp = run_json(&[
        "--base-url",
        &fixture.base_url,
        "--transport",
        "mcp",
        "call",
        "jobs_get_status",
        "--json",
        r#"{"job_id":"job-42"}"#,
    ]);
    assert_eq!(explicit_mcp["slug"], "jobs_get_status");
    assert_eq!(explicit_mcp["output"]["status"], "completed");

    let explicit_rest = run_failure_with_env(
        &[
            "--base-url",
            &fixture.base_url,
            "--transport",
            "rest",
            "call",
            "jobs_get_status",
            "--json",
            r#"{"job_id":"job-42"}"#,
        ],
        &[],
    );
    assert!(
        explicit_rest.contains("invalid tool slug"),
        "REST-only transport should not silently fall back to MCP: {explicit_rest}"
    );
}
