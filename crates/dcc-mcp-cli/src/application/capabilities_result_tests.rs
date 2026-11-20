use serde_json::json;

use super::*;

#[test]
fn successful_result_keeps_evidence_and_routing_metadata() {
    let result = json!({
        "success": true,
        "context": {
            "host_version": "20.5",
            "available": [{"key": "render.lpe", "evidence_ref": "artifact://probe-1"}]
        },
        "control_route": "gateway",
        "gateway_stats_recorded": true
    });

    assert_eq!(project_result(result.clone()), result);
    assert_eq!(project_result(result.clone()), result);
    assert_eq!(
        backend_summary(&result).unwrap()["available"][0]["evidence_ref"],
        "artifact://probe-1"
    );
}

#[test]
fn gateway_envelope_reads_context_through_output() {
    let result = json!({
        "slug": "describe_capabilities",
        "output": {
            "success": true,
            "context": {
                "host_version": "20.5",
                "available": [{"key": "render.lpe", "probe_result": "PASS"}]
            }
        }
    });

    assert_eq!(backend_summary(&result).unwrap()["host_version"], "20.5");
    assert_eq!(
        backend_summary(&result).unwrap()["available"][0]["key"],
        "render.lpe"
    );
}

#[test]
fn missing_summary_is_not_projected_into_fake_evidence() {
    let result = json!({"success": true, "context": "not-an-object"});

    assert!(backend_summary(&result).unwrap().is_string());
    assert!(backend_summary_ref(&result).unwrap().is_string());
    let projected = project_result(result);
    assert_eq!(projected["ok"], false);
    assert_eq!(projected["error"]["code"], "invalid_capabilities_response");
    assert!(!command_succeeded(&projected["error"]["response"]));
}

#[test]
fn backend_failure_keeps_the_original_payload_and_identity() {
    let result = json!({
        "success": false,
        "error": "host_capability_provider_failed",
        "dcc_type": "comfyui",
        "instance_id": "abc12345-0000-0000-0000-000000000000",
        "instance_short": "abc12345"
    });

    let projected = project_result(result);

    assert_eq!(projected["ok"], false);
    assert_eq!(
        projected["error"]["error"],
        "host_capability_provider_failed"
    );
    assert_eq!(projected["dcc_type"], "comfyui");
    assert_eq!(projected["instance_short"], "abc12345");
    assert_eq!(projected["backend_tool"], "describe_capabilities");
}

#[test]
fn standard_mcp_text_summary_is_recognized() {
    let summary = json!({"host_version": "20.5", "available": []});
    let result = json!({
        "result": {
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({"context": summary})).unwrap(),
            }],
        },
    });

    assert_eq!(backend_summary(&result).unwrap()["host_version"], "20.5");
    assert_eq!(project_result(result.clone()), result);
    assert!(command_succeeded(&result));
}

#[test]
fn malformed_text_summary_fails_without_changing_the_original_payload() {
    let result = json!({
        "success": true,
        "result": {
            "content": [{"type": "text", "text": "not-json"}],
        },
        "dcc_type": "unreal",
        "instance_short": "abc12345",
    });

    let projected = project_result(result.clone());

    assert!(backend_summary(&result).is_none());
    assert!(backend_summary_ref(&result).is_none());
    assert!(!command_succeeded(&result));
    assert_eq!(projected["ok"], false);
    assert_eq!(projected["error"]["code"], "invalid_capabilities_response");
    assert_eq!(projected["dcc_type"], "unreal");
    assert_eq!(projected["instance_short"], "abc12345");
}
