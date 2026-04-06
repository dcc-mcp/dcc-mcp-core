//! Action parameter validation against JSON Schema.
//!
//! Provides [`ActionValidator`] which validates a `serde_json::Value` payload
//! against a JSON Schema stored in [`crate::registry::ActionMeta`].
//!
//! ## Supported schema keywords
//!
//! | Keyword | Supported |
//! |---------|-----------|
//! | `type` (string, number, boolean, array, object, null, integer) | yes |
//! | `required` | yes |
//! | `properties` (nested type + enum check) | yes |
//! | `enum` (top-level) | yes |
//! | `minLength` / `maxLength` | yes |
//! | `minimum` / `maximum` | yes |
//! | `minItems` / `maxItems` | yes |
//! | `additionalProperties: false` | yes |

use serde_json::Value;
use std::fmt;

use crate::registry::ActionMeta;

// ── ValidationError ───────────────────────────────────────────────────────────

/// A single validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// JSON path to the failing field (e.g. `"params.radius"`).
    pub path: String,
    /// Human-readable error message.
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

// ── ValidationResult ──────────────────────────────────────────────────────────

/// The outcome of a validation run.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// All validation errors.  Empty means the input is valid.
    pub errors: Vec<ValidationError>,
}

impl ValidationResult {
    /// Return `true` if there are no errors.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Convenience: convert to `Ok(())` / `Err(String)`.
    pub fn into_result(self) -> Result<(), String> {
        if self.is_valid() {
            Ok(())
        } else {
            let msgs: Vec<String> = self.errors.iter().map(|e| e.to_string()).collect();
            Err(msgs.join("; "))
        }
    }
}

// ── ActionValidator ───────────────────────────────────────────────────────────

/// Validates an Action's parameters against its JSON Schema.
///
/// # Example
///
/// ```no_run
/// use dcc_mcp_actions::registry::ActionMeta;
/// use dcc_mcp_actions::validator::ActionValidator;
/// use serde_json::json;
///
/// let meta = ActionMeta {
///     name: "create_sphere".into(),
///     dcc: "maya".into(),
///     input_schema: json!({
///         "type": "object",
///         "required": ["radius"],
///         "properties": {
///             "radius": { "type": "number", "minimum": 0.0 },
///             "name":   { "type": "string", "maxLength": 64 }
///         }
///     }),
///     ..Default::default()
/// };
///
/// let validator = ActionValidator::new(&meta);
/// assert!(validator.validate_input(&json!({"radius": 1.0})).is_valid());
/// assert!(!validator.validate_input(&json!({"radius": -1.0})).is_valid());
/// assert!(!validator.validate_input(&json!({})).is_valid()); // missing required
/// ```
#[derive(Debug, Clone)]
pub struct ActionValidator {
    schema: Value,
}

impl ActionValidator {
    /// Create a validator from action metadata.
    #[must_use]
    pub fn new(meta: &ActionMeta) -> Self {
        Self {
            schema: meta.input_schema.clone(),
        }
    }

    /// Create a validator directly from a JSON Schema value.
    #[must_use]
    pub fn from_schema(schema: Value) -> Self {
        Self { schema }
    }

    /// Validate `params` against the action's `input_schema`.
    ///
    /// Returns a [`ValidationResult`] that may contain zero or more errors.
    /// An empty error list means the input is valid.
    #[must_use]
    pub fn validate_input(&self, params: &Value) -> ValidationResult {
        let mut errors = Vec::new();
        validate_value("params", params, &self.schema, &mut errors);
        ValidationResult { errors }
    }
}

// ── Core recursive validator ──────────────────────────────────────────────────

pub(crate) fn validate_value(
    path: &str,
    value: &Value,
    schema: &Value,
    errors: &mut Vec<ValidationError>,
) {
    // If schema is not an object (e.g. `true` / `false`), skip deep validation.
    let schema_obj = match schema.as_object() {
        Some(o) => o,
        None => return,
    };

    // ── type check ──
    if let Some(type_val) = schema_obj.get("type") {
        let expected = match type_val {
            Value::String(s) => vec![s.as_str()],
            Value::Array(arr) => arr.iter().filter_map(|v| v.as_str()).collect(),
            _ => vec![],
        };
        if !expected.is_empty() && !type_matches(value, &expected) {
            let got = json_type_name(value);
            errors.push(ValidationError {
                path: path.to_string(),
                message: format!("expected type {:?}, got {got}", expected),
            });
            // Don't continue checking sub-constraints on a type mismatch.
            return;
        }
    }

    // ── enum check ──
    if let Some(Value::Array(variants)) = schema_obj.get("enum") {
        if !variants.contains(value) {
            errors.push(ValidationError {
                path: path.to_string(),
                message: format!(
                    "value must be one of: {}",
                    variants
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
    }

    // ── string constraints ──
    if let Value::String(s) = value {
        if let Some(max) = schema_obj.get("maxLength").and_then(|v| v.as_u64()) {
            if s.chars().count() as u64 > max {
                errors.push(ValidationError {
                    path: path.to_string(),
                    message: format!(
                        "string length {} exceeds maxLength {max}",
                        s.chars().count()
                    ),
                });
            }
        }
        if let Some(min) = schema_obj.get("minLength").and_then(|v| v.as_u64()) {
            if (s.chars().count() as u64) < min {
                errors.push(ValidationError {
                    path: path.to_string(),
                    message: format!(
                        "string length {} is less than minLength {min}",
                        s.chars().count()
                    ),
                });
            }
        }
    }

    // ── numeric constraints ──
    if let Some(n) = value.as_f64() {
        if let Some(min) = schema_obj.get("minimum").and_then(|v| v.as_f64()) {
            if n < min {
                errors.push(ValidationError {
                    path: path.to_string(),
                    message: format!("{n} is less than minimum {min}"),
                });
            }
        }
        if let Some(max) = schema_obj.get("maximum").and_then(|v| v.as_f64()) {
            if n > max {
                errors.push(ValidationError {
                    path: path.to_string(),
                    message: format!("{n} exceeds maximum {max}"),
                });
            }
        }
    }

    // ── array constraints ──
    if let Value::Array(arr) = value {
        if let Some(min) = schema_obj.get("minItems").and_then(|v| v.as_u64()) {
            if (arr.len() as u64) < min {
                errors.push(ValidationError {
                    path: path.to_string(),
                    message: format!("array length {} is less than minItems {min}", arr.len()),
                });
            }
        }
        if let Some(max) = schema_obj.get("maxItems").and_then(|v| v.as_u64()) {
            if arr.len() as u64 > max {
                errors.push(ValidationError {
                    path: path.to_string(),
                    message: format!("array length {} exceeds maxItems {max}", arr.len()),
                });
            }
        }
        // items schema
        if let Some(items_schema) = schema_obj.get("items") {
            for (i, item) in arr.iter().enumerate() {
                validate_value(&format!("{path}[{i}]"), item, items_schema, errors);
            }
        }
    }

    // ── object constraints ──
    if let Value::Object(obj) = value {
        // required fields
        if let Some(Value::Array(required)) = schema_obj.get("required") {
            for req in required {
                if let Some(field) = req.as_str() {
                    if !obj.contains_key(field) {
                        errors.push(ValidationError {
                            path: path.to_string(),
                            message: format!("missing required field '{field}'"),
                        });
                    }
                }
            }
        }

        // properties
        if let Some(Value::Object(props)) = schema_obj.get("properties") {
            for (key, val) in obj {
                if let Some(prop_schema) = props.get(key) {
                    validate_value(&format!("{path}.{key}"), val, prop_schema, errors);
                }
            }
        }

        // additionalProperties: false
        if let Some(Value::Bool(false)) = schema_obj.get("additionalProperties") {
            let known_props: std::collections::HashSet<&str> = schema_obj
                .get("properties")
                .and_then(|p| p.as_object())
                .map_or_else(std::collections::HashSet::new, |props| {
                    props.keys().map(String::as_str).collect()
                });
            for key in obj.keys() {
                if !known_props.contains(key.as_str()) {
                    errors.push(ValidationError {
                        path: path.to_string(),
                        message: format!("unexpected additional property '{key}'"),
                    });
                }
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub(crate) fn type_matches(value: &Value, expected: &[&str]) -> bool {
    expected.iter().any(|t| match *t {
        "string" => value.is_string(),
        "number" => value.is_f64() || value.is_i64() || value.is_u64(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        _ => true,
    })
}

pub(crate) fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) => {
            if n.is_f64() {
                "number"
            } else {
                "integer"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
