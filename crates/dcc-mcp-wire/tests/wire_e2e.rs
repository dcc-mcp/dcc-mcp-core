//! End-to-end tests for `dcc-mcp-wire`.
//! These tests exercise the full encode/decode + normalisation paths
//! without hitting real servers, simulating real payloads from hosts/connectors.

use dcc_mcp_wire::{
    WireError,
    validate::{validate_call_batch_params, validate_call_tool_params},
    wire::{decode_call_tool, decode_rest_call, encode_error_response, encode_rest_call_response},
};
use serde_json::json;

// ── Happy paths ────────────────────────────────────────//

#[test]
fn e2e_decode_valid_mcp_request() {
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "maya__create_sphere",
            "arguments": {"radius": 2.0, "segments": 32}
        }
    });
    let (name, args, meta) = decode_call_tool(&req).unwrap();
    assert_eq!(name, "maya__create_sphere");
    assert_eq!(args, json!({"radius": 2.0, "segments": 32}));
    assert!(meta.is_none());
}

#[test]
fn e2e_decode_string_arguments_normalised() {
    // Host passed arguments as JSON string → normalised to object
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "maya__print",
            "arguments": "{\"code\": \"print(1)\"}"
        }
    });
    let (_name, args, _) = decode_call_tool(&req).unwrap();
    assert_eq!(args, json!({"code": "print(1)"}));
}

#[test]
fn e2e_decode_rest_call_valid() {
    let req = json!({
        "tool_slug": "maya__bake",
        "arguments": {"frame": 1, "output": "/tmp/out.iff"}
    });
    let (slug, args, _) = decode_rest_call(&req).unwrap();
    assert_eq!(slug, "maya__bake");
    assert_eq!(args, json!({"frame": 1, "output": "/tmp/out.iff"}));
}

#[test]
fn e2e_roundtrip_encode_decode() {
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "test",
            "arguments": {"x": 1}
        }
    });
    let (name, args, _meta) = decode_call_tool(&req).unwrap();
    let response = encode_rest_call_response(&name, &args, false, "req-123");
    let obj = response.as_object().unwrap();
    assert_eq!(obj.get("slug").unwrap(), "test");
    assert_eq!(obj.get("request_id").unwrap(), "req-123");
}

// ── Error paths (no panics) ──────────────────────────────────//

#[test]
fn e2e_reject_null_envelope() {
    let err = decode_call_tool(&json!(null)).unwrap_err();
    assert_eq!(err.kind(), "envelope-not-object");
}

#[test]
fn e2e_reject_array_envelope() {
    let err = decode_call_tool(&json!([1, 2])).unwrap_err();
    assert_eq!(err.kind(), "envelope-not-object");
}

#[test]
fn e2e_reject_missing_params() {
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call"
        // missing "params"
    });
    let err = decode_call_tool(&req).unwrap_err();
    assert_eq!(err.kind(), "missing-field");
}

#[test]
fn e2e_reject_missing_name() {
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {"arguments": {"x": 1}}
    });
    let err = decode_call_tool(&req).unwrap_err();
    assert_eq!(err.kind(), "missing-field");
}

#[test]
fn e2e_reject_double_stringified() {
    // arguments = string that decodes to string, not object
    let inner = r#""hello""#.to_string();
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "test",
            "arguments": inner
        }
    });
    let err = decode_call_tool(&req).unwrap_err();
    assert_eq!(err.kind(), "arguments-decoded-not-object");
}

#[test]
fn e2e_reject_arguments_array() {
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "test",
            "arguments": [1, 2, 3]
        }
    });
    let err = decode_call_tool(&req).unwrap_err();
    assert_eq!(err.kind(), "arguments-not-object");
}

#[test]
fn e2e_reject_arguments_number() {
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "test",
            "arguments": 42
        }
    });
    let err = decode_call_tool(&req).unwrap_err();
    assert_eq!(err.kind(), "arguments-not-object");
}

#[test]
fn e2e_reject_arguments_boolean() {
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "test",
            "arguments": true
        }
    });
    let err = decode_call_tool(&req).unwrap_err();
    assert_eq!(err.kind(), "arguments-not-object");
}

// ── validate_call_tool_params ───────────────────────────────//

#[test]
fn e2e_validate_with_schema_success() {
    let schema = json!({
        "type": "object",
        "required": ["file_path"],
        "properties": {"file_path": {"type": "string"}}
    });
    let args = json!({"file_path": "/tmp/test.txt"});
    let result = validate_call_tool_params(Some("test_tool"), Some(args), Some(&schema)).unwrap();
    assert_eq!(result, json!({"file_path": "/tmp/test.txt"}));
}

#[test]
fn e2e_validate_with_schema_missing_field() {
    let schema = json!({
        "type": "object",
        "required": ["file_path"],
        "properties": {"file_path": {"type": "string"}}
    });
    let args = json!({});
    let err = validate_call_tool_params(Some("test_tool"), Some(args), Some(&schema)).unwrap_err();
    assert_eq!(err.kind(), "schema-validation-failed");
}

// ── encode_error_response shape ──────────────────────────────//

#[test]
fn e2e_error_response_has_stable_keys() {
    let err = WireError::MissingField {
        field: "name".to_string(),
    };
    let resp = encode_error_response(&err, Some("req-1"));
    let obj = resp.as_object().unwrap();
    assert!(obj.contains_key("kind"));
    assert!(obj.contains_key("message"));
    assert!(obj.contains_key("hint"));
    assert_eq!(obj.get("request_id").unwrap(), "req-1");
}

// ── Batch e2e ───────────────────────────────────────────//

#[test]
fn e2e_validate_batch_happy() {
    let calls = vec![
        json!({"name": "tool_a", "arguments": {"x": 1}}),
        json!({"tool_slug": "tool_b", "params": {"y": 2}}),
    ];
    let results = validate_call_batch_params(&calls).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "tool_a");
    assert_eq!(results[1].0, "tool_b");
}

#[test]
fn e2e_validate_batch_item_not_object() {
    let calls = vec![json!("not an object")];
    let err = validate_call_batch_params(&calls).unwrap_err();
    assert_eq!(err.kind(), "batch-item-invalid");
}

#[test]
fn e2e_validate_batch_item_missing_name() {
    let calls = vec![json!({"arguments": {"x": 1}})];
    let err = validate_call_batch_params(&calls).unwrap_err();
    assert_eq!(err.kind(), "batch-item-invalid");
}
