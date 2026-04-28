//! Type checks, required fields, and property validation tests.

use super::fixtures::make_meta_with_schema;
use super::*;
use serde_json::json;

// ── type checks ───────────────────────────────────────────────────────────

#[test]
fn test_valid_object_passes() {
    let meta = make_meta_with_schema(json!({ "type": "object" }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!({})).is_valid());
    assert!(v.validate_input(&json!({"x": 1})).is_valid());
}

#[test]
fn test_wrong_type_fails() {
    let meta = make_meta_with_schema(json!({ "type": "object" }));
    let v = ActionValidator::new(&meta);
    assert!(!v.validate_input(&json!("hello")).is_valid());
    assert!(!v.validate_input(&json!(42)).is_valid());
    assert!(!v.validate_input(&json!(null)).is_valid());
}

#[test]
fn test_number_type_accepts_int_and_float() {
    let meta = make_meta_with_schema(json!({ "type": "number" }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!(1.5)).is_valid());
    assert!(v.validate_input(&json!(42)).is_valid());
}

#[test]
fn test_integer_type_rejects_float() {
    let meta = make_meta_with_schema(json!({ "type": "integer" }));
    let v = ActionValidator::new(&meta);
    // serde_json represents 1.5 as f64, not integer
    assert!(v.validate_input(&json!(42)).is_valid());
    // 1.5 will be f64
    assert!(!v.validate_input(&json!(1.5)).is_valid());
}

#[test]
fn test_boolean_type() {
    let meta = make_meta_with_schema(json!({ "type": "boolean" }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!(true)).is_valid());
    assert!(!v.validate_input(&json!(1)).is_valid());
}

#[test]
fn test_null_type() {
    let meta = make_meta_with_schema(json!({ "type": "null" }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!(null)).is_valid());
    assert!(!v.validate_input(&json!(0)).is_valid());
}

#[test]
fn test_union_type() {
    let meta = make_meta_with_schema(json!({ "type": ["string", "null"] }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!("hello")).is_valid());
    assert!(v.validate_input(&json!(null)).is_valid());
    assert!(!v.validate_input(&json!(1)).is_valid());
}

// ── required ──────────────────────────────────────────────────────────────

#[test]
fn test_required_field_present() {
    let meta = make_meta_with_schema(json!({
        "type": "object",
        "required": ["name"],
        "properties": { "name": { "type": "string" } }
    }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!({"name": "sphere"})).is_valid());
}

#[test]
fn test_required_field_missing() {
    let meta = make_meta_with_schema(json!({
        "type": "object",
        "required": ["name"],
        "properties": { "name": { "type": "string" } }
    }));
    let v = ActionValidator::new(&meta);
    let result = v.validate_input(&json!({}));
    assert!(!result.is_valid());
    assert!(
        result.errors[0]
            .message
            .contains("missing required field 'name'")
    );
}

#[test]
fn test_multiple_required_fields_missing() {
    let meta = make_meta_with_schema(json!({
        "type": "object",
        "required": ["x", "y", "z"]
    }));
    let v = ActionValidator::new(&meta);
    let result = v.validate_input(&json!({"x": 1}));
    assert!(!result.is_valid());
    assert_eq!(result.errors.len(), 2); // missing y, z
}

// ── properties ────────────────────────────────────────────────────────────

#[test]
fn test_property_type_check() {
    let meta = make_meta_with_schema(json!({
        "type": "object",
        "properties": {
            "radius": { "type": "number" }
        }
    }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!({"radius": 1.5})).is_valid());
    let result = v.validate_input(&json!({"radius": "big"}));
    assert!(!result.is_valid());
    assert!(result.errors[0].path.contains("radius"));
}
