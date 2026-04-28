//! Numeric, string, array, and enum constraint tests.

use super::fixtures::make_meta_with_schema;
use super::*;
use serde_json::json;

// ── numeric constraints ───────────────────────────────────────────────────

#[test]
fn test_minimum_passes() {
    let meta = make_meta_with_schema(json!({
        "type": "number",
        "minimum": 0.0
    }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!(0.0)).is_valid());
    assert!(v.validate_input(&json!(100.0)).is_valid());
}

#[test]
fn test_minimum_fails() {
    let meta = make_meta_with_schema(json!({ "type": "number", "minimum": 0.0 }));
    let v = ActionValidator::new(&meta);
    assert!(!v.validate_input(&json!(-0.1)).is_valid());
}

#[test]
fn test_maximum_passes() {
    let meta = make_meta_with_schema(json!({ "type": "number", "maximum": 100.0 }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!(50.0)).is_valid());
    assert!(v.validate_input(&json!(100.0)).is_valid());
}

#[test]
fn test_maximum_fails() {
    let meta = make_meta_with_schema(json!({ "type": "number", "maximum": 100.0 }));
    let v = ActionValidator::new(&meta);
    assert!(!v.validate_input(&json!(100.1)).is_valid());
}

#[test]
fn test_min_max_range() {
    let meta = make_meta_with_schema(json!({ "minimum": -1.0, "maximum": 1.0 }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!(0.5)).is_valid());
    assert!(!v.validate_input(&json!(2.0)).is_valid());
    assert!(!v.validate_input(&json!(-2.0)).is_valid());
}

// ── string constraints ────────────────────────────────────────────────────

#[test]
fn test_max_length_passes() {
    let meta = make_meta_with_schema(json!({ "type": "string", "maxLength": 5 }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!("hello")).is_valid()); // exactly 5
    assert!(v.validate_input(&json!("hi")).is_valid());
}

#[test]
fn test_max_length_fails() {
    let meta = make_meta_with_schema(json!({ "type": "string", "maxLength": 3 }));
    let v = ActionValidator::new(&meta);
    assert!(!v.validate_input(&json!("toolong")).is_valid());
}

#[test]
fn test_min_length_passes() {
    let meta = make_meta_with_schema(json!({ "type": "string", "minLength": 3 }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!("abc")).is_valid());
}

#[test]
fn test_min_length_fails() {
    let meta = make_meta_with_schema(json!({ "type": "string", "minLength": 5 }));
    let v = ActionValidator::new(&meta);
    assert!(!v.validate_input(&json!("hi")).is_valid());
}

// ── array constraints ─────────────────────────────────────────────────────

#[test]
fn test_min_items_passes() {
    let meta = make_meta_with_schema(json!({ "type": "array", "minItems": 2 }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!([1, 2])).is_valid());
    assert!(v.validate_input(&json!([1, 2, 3])).is_valid());
}

#[test]
fn test_min_items_fails() {
    let meta = make_meta_with_schema(json!({ "type": "array", "minItems": 2 }));
    let v = ActionValidator::new(&meta);
    assert!(!v.validate_input(&json!([1])).is_valid());
    assert!(!v.validate_input(&json!([])).is_valid());
}

#[test]
fn test_max_items_fails() {
    let meta = make_meta_with_schema(json!({ "type": "array", "maxItems": 2 }));
    let v = ActionValidator::new(&meta);
    assert!(!v.validate_input(&json!([1, 2, 3])).is_valid());
}

#[test]
fn test_items_type_check() {
    let meta = make_meta_with_schema(json!({
        "type": "array",
        "items": { "type": "number" }
    }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!([1.0, 2.0])).is_valid());
    let result = v.validate_input(&json!([1.0, "bad"]));
    assert!(!result.is_valid());
    assert!(result.errors[0].path.contains("[1]"));
}

// ── enum ──────────────────────────────────────────────────────────────────

#[test]
fn test_enum_passes() {
    let meta = make_meta_with_schema(json!({ "enum": ["low", "medium", "high"] }));
    let v = ActionValidator::new(&meta);
    assert!(v.validate_input(&json!("medium")).is_valid());
}

#[test]
fn test_enum_fails() {
    let meta = make_meta_with_schema(json!({ "enum": ["low", "medium", "high"] }));
    let v = ActionValidator::new(&meta);
    assert!(!v.validate_input(&json!("extreme")).is_valid());
}
