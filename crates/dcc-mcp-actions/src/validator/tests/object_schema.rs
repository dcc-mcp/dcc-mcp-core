//! Object-level constraint tests: additionalProperties and nested objects.

use super::fixtures::make_meta_with_schema;
use super::*;
use serde_json::json;

// ── additionalProperties ──────────────────────────────────────────────────

#[test]
fn test_additional_properties_false_rejects_extra() {
    let meta = make_meta_with_schema(json!({
        "type": "object",
        "properties": { "name": { "type": "string" } },
        "additionalProperties": false
    }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!({"name": "x"})).is_valid());
    let result = v.validate_input(&json!({"name": "x", "unknown": 1}));
    assert!(!result.is_valid());
    assert!(result.errors[0].message.contains("unknown"));
}

#[test]
fn test_additional_properties_true_allows_extra() {
    let meta = make_meta_with_schema(json!({
        "type": "object",
        "properties": { "name": { "type": "string" } },
        "additionalProperties": true
    }));
    let v = ActionValidator::new(&meta);
    assert!(
        v.validate_input(&json!({"name": "x", "extra": 99}))
            .is_valid()
    );
}

// ── nested ────────────────────────────────────────────────────────────────

#[test]
fn test_nested_object_validation() {
    let meta = make_meta_with_schema(json!({
        "type": "object",
        "required": ["position"],
        "properties": {
            "position": {
                "type": "object",
                "required": ["x", "y", "z"],
                "properties": {
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "z": { "type": "number" }
                }
            }
        }
    }));
    let v = ActionValidator::new(&meta);
    assert!(
        v.validate_input(&json!({"position": {"x": 1.0, "y": 2.0, "z": 3.0}}))
            .is_valid()
    );
    let result = v.validate_input(&json!({"position": {"x": 1.0}}));
    assert!(!result.is_valid());
    // Missing y and z
    assert!(result.errors.len() >= 2);
}

// ── empty / trivial schema ────────────────────────────────────────────────

#[test]
fn test_empty_schema_accepts_anything() {
    let meta = make_meta_with_schema(json!({}));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!(null)).is_valid());
    assert!(v.validate_input(&json!({"any": "thing"})).is_valid());
}

// ── additionalProperties: false without properties ────────────────────────

#[test]
fn test_additional_properties_false_no_properties_key() {
    // `additionalProperties: false` without a `properties` key —
    // no known properties means every key is "additional".
    let v = ActionValidator::from_schema(json!({
        "type": "object",
        "additionalProperties": false
    }));
    // An empty object has no additional properties → valid
    assert!(v.validate_input(&json!({})).is_valid());
    // Any key is extra → invalid
    let result = v.validate_input(&json!({"anything": 1}));
    assert!(!result.is_valid());
}
