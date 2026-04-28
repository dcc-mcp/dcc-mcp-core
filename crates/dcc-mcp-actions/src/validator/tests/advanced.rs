//! Advanced validator tests: from_schema, boolean schemas, boundaries,
//! nested arrays, multiple errors, and ValidationResult helpers.

use super::*;
use serde_json::json;

// ── ValidationResult helpers ──────────────────────────────────────────────

#[test]
fn test_validation_result_into_result_ok() {
    let r = ValidationResult { errors: vec![] };
    assert!(r.into_result().is_ok());
}

#[test]
fn test_validation_result_into_result_err() {
    let r = ValidationResult {
        errors: vec![ValidationError {
            path: "params.x".into(),
            message: "bad".into(),
        }],
    };
    let err = r.into_result().unwrap_err();
    assert!(err.contains("params.x"));
}

// ── from_schema ───────────────────────────────────────────────────────────

#[test]
fn test_from_schema_direct() {
    let schema = json!({ "type": "string", "minLength": 1 });
    let v = ActionValidator::from_schema(schema);
    assert!(v.validate_input(&json!("hello")).is_valid());
    assert!(!v.validate_input(&json!("")).is_valid());
}

// ── boolean schema (JSON Schema `true`/`false`) ───────────────────────────

#[test]
fn test_boolean_schema_true_accepts_anything() {
    // `true` schema: any value is valid
    let v = ActionValidator::from_schema(json!(true));
    assert!(v.validate_input(&json!(null)).is_valid());
    assert!(v.validate_input(&json!(42)).is_valid());
    assert!(v.validate_input(&json!("hello")).is_valid());
    assert!(v.validate_input(&json!({"x": 1})).is_valid());
}

#[test]
fn test_boolean_schema_false_accepts_anything() {
    // Our validator skips validation for non-object schemas (incl. `false`)
    // so no errors are generated — matches lenient "unknown schema → skip" policy.
    let v = ActionValidator::from_schema(json!(false));
    assert!(v.validate_input(&json!(null)).is_valid());
}

// ── deeply nested array items ─────────────────────────────────────────────

#[test]
fn test_nested_array_items_type_check() {
    // Array of arrays of numbers
    let v = ActionValidator::from_schema(json!({
        "type": "array",
        "items": {
            "type": "array",
            "items": { "type": "number" }
        }
    }));
    assert!(v.validate_input(&json!([[1.0, 2.0], [3.0]])).is_valid());
    let result = v.validate_input(&json!([[1.0, "bad"], [3.0]]));
    assert!(!result.is_valid());
    // Path should include both array indices
    assert!(result.errors[0].path.contains("[0]"));
    assert!(result.errors[0].path.contains("[1]"));
}

// ── enum inside property ──────────────────────────────────────────────────

#[test]
fn test_enum_inside_property() {
    let v = ActionValidator::from_schema(json!({
        "type": "object",
        "properties": {
            "mode": { "enum": ["read", "write", "append"] }
        }
    }));
    assert!(v.validate_input(&json!({"mode": "read"})).is_valid());
    let result = v.validate_input(&json!({"mode": "invalid"}));
    assert!(!result.is_valid());
    assert!(result.errors[0].path.contains("mode"));
}

// ── multiple errors accumulate ────────────────────────────────────────────

#[test]
fn test_multiple_errors_accumulate() {
    let v = ActionValidator::from_schema(json!({
        "type": "object",
        "required": ["x", "y"],
        "properties": {
            "x": { "type": "number", "minimum": 0.0 },
            "y": { "type": "string", "maxLength": 5 }
        }
    }));
    // x wrong type, y too long
    let result = v.validate_input(&json!({"x": "bad", "y": "toolong"}));
    assert!(!result.is_valid());
    // Should have at least 2 errors (type mismatch for x + length for y)
    assert!(result.errors.len() >= 2);
}

// ── array with both minItems and items schema ─────────────────────────────

#[test]
fn test_array_min_items_and_items_schema() {
    let v = ActionValidator::from_schema(json!({
        "type": "array",
        "minItems": 2,
        "items": { "type": "number" }
    }));
    assert!(v.validate_input(&json!([1.0, 2.0])).is_valid());
    // Too short
    assert!(!v.validate_input(&json!([1.0])).is_valid());
    // Correct length but wrong item type
    let result = v.validate_input(&json!([1.0, "oops"]));
    assert!(!result.is_valid());
}

// ── string boundary at exact limits ──────────────────────────────────────

#[test]
fn test_string_exact_min_max_length() {
    let v = ActionValidator::from_schema(json!({
        "type": "string",
        "minLength": 3,
        "maxLength": 5
    }));
    assert!(v.validate_input(&json!("abc")).is_valid()); // exactly 3
    assert!(v.validate_input(&json!("abcde")).is_valid()); // exactly 5
    assert!(!v.validate_input(&json!("ab")).is_valid()); // 2 < min
    assert!(!v.validate_input(&json!("abcdef")).is_valid()); // 6 > max
}

// ── numeric boundary at exact limits ─────────────────────────────────────

#[test]
fn test_numeric_exact_min_max() {
    let v = ActionValidator::from_schema(json!({
        "type": "number",
        "minimum": 0.0,
        "maximum": 1.0
    }));
    assert!(v.validate_input(&json!(0.0)).is_valid()); // exactly min
    assert!(v.validate_input(&json!(1.0)).is_valid()); // exactly max
    assert!(!v.validate_input(&json!(-0.001)).is_valid());
    assert!(!v.validate_input(&json!(1.001)).is_valid());
}

// ── error message format ──────────────────────────────────────────────────

#[test]
fn test_validation_error_display() {
    let e = ValidationError {
        path: "params.radius".into(),
        message: "must be >= 0".into(),
    };
    assert_eq!(e.to_string(), "params.radius: must be >= 0");
}
