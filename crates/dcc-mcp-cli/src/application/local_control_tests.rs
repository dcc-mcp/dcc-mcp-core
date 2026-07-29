use super::*;

#[test]
fn local_tool_slug_round_trips() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    let slug = local_instance::local_tool_slug(&entry, "maya_scene__get_session_info");
    let parsed = parse_local_tool_slug(&slug);

    assert_eq!(parsed.dcc_type.as_deref(), Some("maya"));
    assert_eq!(
        parsed.instance_hint.as_deref(),
        Some(local_instance::instance_short(&entry).as_str())
    );
    assert_eq!(parsed.backend_tool, "maya_scene__get_session_info");
}

#[test]
fn local_call_retries_only_after_structured_unknown_sidecar_action() {
    assert!(should_retry_local_call_via_discovery(&json!({
        "success": false,
        "error": "unknown-action"
    })));
    assert!(!should_retry_local_call_via_discovery(&json!({
        "isError": true,
        "structuredContent": {"error": "dispatch-failed"}
    })));
}

#[test]
fn ui_control_calls_use_the_discovery_executor() {
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    entry.metadata.insert(
        "discovery_mcp_url".to_string(),
        "http://127.0.0.1:18081/mcp".to_string(),
    );

    assert_eq!(
        local_dispatch_mcp_url(&entry, "ui_control__snapshot"),
        "http://127.0.0.1:18081/mcp"
    );
    assert_eq!(
        local_dispatch_mcp_url(&entry, "maya_scene__get_session_info"),
        "http://127.0.0.1:18080/mcp"
    );
}

#[test]
fn load_skill_request_strips_local_routing_fields() {
    let request = split_load_skill_request(json!({
        "skill_name": "workflow",
        "dcc_type": "maya",
        "dcc": "maya-legacy",
        "instance_id": "abc12345",
        "target_tool_slug": "maya.abc12345.workflow__run",
        "meta": {"search_id": "search-1"},
        "_meta": {"response_format": "json"},
        "tool_group": "execution",
        "activate_groups": false
    }))
    .unwrap();

    assert_eq!(request.dcc_type.as_deref(), Some("maya"));
    assert_eq!(request.instance_id.as_deref(), Some("abc12345"));
    assert_eq!(
        request.target_tool_slug.as_deref(),
        Some("maya.abc12345.workflow__run")
    );
    assert_eq!(
        request.backend_body,
        json!({
            "skill_name": "workflow",
            "tool_group": "execution",
            "activate_groups": false,
            "strict_tool_group": true
        })
    );
}

#[test]
fn correlated_ungrouped_tool_does_not_request_strict_group_loading() {
    let request = split_load_skill_request(json!({
        "skill_name": "workflow",
        "target_tool_slug": "maya.abc12345.workflow__run"
    }))
    .unwrap();

    assert!(request.target_tool_slug.is_some());
    assert!(request.backend_body.get("strict_tool_group").is_none());
}

#[test]
fn local_post_load_hint_resolves_bare_declaration_to_canonical_action() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    let mut payload = json!({
        "loaded": true,
        "skill_name": "workflow",
        "registered_tools": ["workflow__run"],
        "tools": [{
            "name": "workflow__run",
            "inputSchema": {"type": "object", "properties": {}}
        }]
    });

    attach_local_post_load_hint(
        &mut payload,
        &entry,
        "blender.deadbeef.run",
        Some("workflow"),
    );

    assert_eq!(
        payload["next_step"]["arguments"]["tool_slug"],
        local_instance::local_tool_slug(&entry, "workflow__run")
    );
}

#[test]
fn local_no_arg_tool_requires_safety_hints_before_direct_call() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    let mut hits = Vec::new();
    extend_tool_hits(
        &mut hits,
        &entry,
        &json!({
            "tools": [{
                "name": "dangerous_reset",
                "description": "Reset the scene",
                "has_schema": false,
                "annotations": {"destructive_hint": true}
            }]
        }),
    );
    assert_eq!(hits[0]["next_step"]["action"], "describe");

    hits.clear();
    extend_tool_hits(
        &mut hits,
        &entry,
        &json!({
            "tools": [{
                "name": "dangerous_reset",
                "description": "Reset the scene",
                "has_schema": false,
                "annotations": {
                    "read_only_hint": false,
                    "destructive_hint": true,
                    "idempotent_hint": false,
                    "open_world_hint": false
                }
            }]
        }),
    );
    assert_eq!(hits[0]["next_step"]["action"], "call");
    assert_eq!(hits[0]["annotations"]["destructive_hint"], true);
}

#[test]
fn local_strict_object_schema_can_call_without_describe() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    let mut payload = json!({
        "loaded": true,
        "tools": [{
            "name": "workflow__run",
            "inputSchema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
                "additionalProperties": false
            },
            "annotations": {
                "read_only_hint": true,
                "destructive_hint": false,
                "idempotent_hint": true,
                "open_world_hint": false
            }
        }]
    });

    attach_local_post_load_hint(&mut payload, &entry, "workflow__run", Some("workflow"));

    assert_eq!(payload["next_step"]["action"], "call", "{payload}");
    assert_eq!(payload["compact_schema"]["complete_for_call"], true);
    assert_eq!(payload["compact_schema"]["has_schema"], true);
}

#[test]
fn local_complex_schema_requires_describe_even_with_safety_hints() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    for schema in [
        json!({"type": "object", "oneOf": []}),
        json!({"type": "object", "anyOf": []}),
        json!({"type": "object", "allOf": []}),
        json!({"type": "object", "$ref": "#/$defs/input", "$defs": {}}),
        json!({"type": "object", "additionalProperties": true}),
        json!({"type": "object", "additionalProperties": {"type": "string"}}),
        json!({"type": "object", "patternProperties": {}}),
        json!({"type": "object", "dependentRequired": {}}),
        json!({"type": "object", "if": {}, "then": {}}),
    ] {
        let mut payload = json!({
            "loaded": true,
            "tools": [{
                "name": "workflow__run",
                "inputSchema": schema,
                "annotations": {
                    "read_only_hint": true,
                    "destructive_hint": false,
                    "idempotent_hint": true,
                    "open_world_hint": false
                }
            }]
        });
        attach_local_post_load_hint(&mut payload, &entry, "workflow__run", Some("workflow"));
        assert_eq!(payload["next_step"]["action"], "describe", "{payload}");
        assert_eq!(payload["compact_schema"]["complete_for_call"], false);
        assert_eq!(payload["compact_schema"]["has_schema"], true);
    }
}

#[test]
fn reload_result_rejects_backend_failure_envelope() {
    assert!(!reload_result_succeeded(&json!({
        "success": false,
        "error": "no-source-file"
    })));
}

#[test]
fn call_result_rejects_nested_tool_failure_but_accepts_transport_success() {
    assert!(!call_result_succeeded(&json!({
        "success": true,
        "output": {"success": false, "message": "domain failure"}
    })));
    assert!(!call_result_succeeded(&json!({
        "success": false,
        "results": [{"ok": false}]
    })));
    assert!(!call_result_succeeded(&json!({
        "content": [{
            "type": "text",
            "text": r#"{"success":false,"message":"sidecar domain failure"}"#
        }],
        "isError": false
    })));
    assert!(call_result_succeeded(&json!({
        "success": true,
        "output": {"success": true}
    })));
}
