use super::*;

use serde_json::json;

#[test]
fn human_output_preserves_not_run_without_support_claim() {
    let value = json!({
        "backend_tool": "describe_capabilities",
        "dcc_type": "comfyui",
        "instance_short": "abc12345",
        "result": {
            "structuredContent": {
                "context": {
                    "host_version": "0.32.0",
                    "adapter_version": "0.1.6",
                    "probed_at": "2026-11-20T00:00:00Z",
                    "available": [],
                    "declared": [],
                    "unavailable": [],
                    "not_run": [{
                        "key": "host.capabilities",
                        "supported": false,
                        "probe_result": "NOT_RUN",
                        "probed_at": "2026-11-20T00:00:00Z",
                        "reason": "No host capability provider is configured"
                    }],
                    "totals": {
                        "available": 0,
                        "declared": 0,
                        "unavailable": 0,
                        "not_run": 1
                    }
                }
            }
        }
    });

    let rendered = render_human(&value);

    assert!(rendered.contains("NOT_RUN (1)"));
    assert!(
        rendered.contains(
            "host.capabilities [NOT_RUN] reason=No host capability provider is configured"
        )
    );
    assert!(!rendered.contains("AVAILABLE (1)"));
}

#[test]
fn human_output_keeps_four_groups_and_does_not_promote_not_run() {
    let value = json!({
        "backend_tool": "describe_capabilities",
        "dcc_type": "houdini",
        "instance_short": "abc12345",
        "result": {
            "structuredContent": {
                "context": {
                    "host_version": "20.5",
                    "adapter_version": "0.3.1",
                    "probed_at": "2026-11-20T00:00:00Z",
                    "available": [{"key": "render.lpe", "probe_result": "PASS"}],
                    "declared": [{
                        "key": "script.terminal_mode",
                        "probe_result": "NOT_RUN",
                        "supported": true,
                        "declaration_only": true,
                        "reason": "adapter declaration only"
                    }],
                    "unavailable": [{
                        "key": "render.cryptomatte",
                        "probe_result": "UNSUPPORTED",
                        "reason": "not exposed by adapter"
                    }],
                    "not_run": [{
                        "key": "capture.in_process_qt",
                        "probe_result": "NOT_RUN",
                        "supported": false,
                        "reason": "no capability provider configured"
                    }],
                    "totals": {
                        "available": 1,
                        "declared": 1,
                        "unavailable": 1,
                        "not_run": 1
                    }
                }
            }
        }
    });

    let rendered = render_human(&value);
    let lines: Vec<&str> = rendered.lines().collect();

    assert_eq!(lines.len(), 10);
    assert_eq!(
        lines[0],
        "status=OK host=20.5 adapter=0.3.1 probed_at=2026-11-20T00:00:00Z"
    );
    assert_eq!(lines[1], "tool=describe_capabilities host=houdini/abc12345");
    assert_eq!(lines[2], "AVAILABLE (1)");
    assert_eq!(lines[3], "  render.lpe [PASS]");
    assert_eq!(lines[4], "DECLARED (1)");
    assert_eq!(
        lines[5],
        "  script.terminal_mode [NOT_RUN] declaration_only reason=adapter declaration only"
    );
    assert_eq!(lines[6], "UNAVAILABLE (1)");
    assert_eq!(
        lines[7],
        "  render.cryptomatte [UNSUPPORTED] reason=not exposed by adapter"
    );
    assert_eq!(lines[8], "NOT_RUN (1)");
    assert_eq!(
        lines[9],
        "  capture.in_process_qt [NOT_RUN] reason=no capability provider configured"
    );
}

#[test]
fn human_output_marks_a_failed_backend_result() {
    let value = json!({
        "ok": false,
        "error": {"error": "host_capability_provider_failed"},
        "backend_tool": "describe_capabilities",
        "dcc_type": "photoshop",
        "instance_short": "def67890",
        "result": {
            "structuredContent": {
                "context": {
                    "host_version": "2026.5",
                    "adapter_version": "0.4.0",
                    "probed_at": "-",
                    "available": [],
                    "declared": [],
                    "unavailable": [],
                    "not_run": []
                }
            }
        }
    });

    let rendered = render_human(&value);

    assert!(rendered.starts_with("status=FAILED "));
    assert!(rendered.contains("NOT_RUN (0)"));
}

#[test]
fn human_output_reads_a_standard_mcp_text_summary() {
    let summary = json!({
        "host_version": "16.0",
        "probed_at": "2026-11-20T00:00:00Z",
        "available": [],
        "declared": [],
        "unavailable": [],
        "not_run": [{"key": "host.capabilities", "probe_result": "NOT_RUN"}],
    });
    let value = json!({
        "result": {
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({"context": summary})).unwrap(),
            }],
        },
    });

    let rendered = render_human(&value);

    assert!(rendered.starts_with("status=OK host=16.0"));
    assert!(rendered.contains("NOT_RUN (1)"));
}
