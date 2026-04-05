//! Input validation for sandbox action parameters.
//!
//! The [`InputValidator`] examines raw JSON parameter maps and applies
//! configurable rules before they reach the DCC action handler.

use serde_json::Value;

use crate::error::SandboxError;

// ── Validation rules ──────────────────────────────────────────────────────────

/// A single named validation rule.
#[derive(Debug, Clone)]
pub enum ValidationRule {
    /// The field must be present and not JSON `null`.
    Required,
    /// The field (when present) must be a JSON string.
    IsString,
    /// The field (when present) must be a JSON number.
    IsNumber,
    /// The field (when present) must be a JSON boolean.
    IsBoolean,
    /// A string field must have at most this many characters.
    MaxLength(usize),
    /// A string field must have at least this many characters.
    MinLength(usize),
    /// A numeric field's value must be ≤ max.
    MaxValue(f64),
    /// A numeric field's value must be ≥ min.
    MinValue(f64),
    /// The string field must not contain any of these substrings
    /// (injection guard).
    ForbiddenSubstrings(Vec<String>),
}

impl ValidationRule {
    /// Apply this rule to `value` for the given `field` name.
    ///
    /// `value` is `None` when the field is absent from the input map.
    pub fn check(&self, field: &str, value: Option<&Value>) -> Result<(), SandboxError> {
        match self {
            ValidationRule::Required => match value {
                None | Some(Value::Null) => Err(SandboxError::ValidationFailed {
                    field: field.to_owned(),
                    reason: "field is required".to_owned(),
                }),
                _ => Ok(()),
            },
            ValidationRule::IsString => {
                if let Some(v) = value {
                    if !v.is_string() {
                        return Err(SandboxError::ValidationFailed {
                            field: field.to_owned(),
                            reason: format!("expected string, got {}", json_type_name(v)),
                        });
                    }
                }
                Ok(())
            }
            ValidationRule::IsNumber => {
                if let Some(v) = value {
                    if !v.is_number() {
                        return Err(SandboxError::ValidationFailed {
                            field: field.to_owned(),
                            reason: format!("expected number, got {}", json_type_name(v)),
                        });
                    }
                }
                Ok(())
            }
            ValidationRule::IsBoolean => {
                if let Some(v) = value {
                    if !v.is_boolean() {
                        return Err(SandboxError::ValidationFailed {
                            field: field.to_owned(),
                            reason: format!("expected boolean, got {}", json_type_name(v)),
                        });
                    }
                }
                Ok(())
            }
            ValidationRule::MaxLength(max) => {
                if let Some(Value::String(s)) = value {
                    if s.len() > *max {
                        return Err(SandboxError::ValidationFailed {
                            field: field.to_owned(),
                            reason: format!("string length {} exceeds maximum {}", s.len(), max),
                        });
                    }
                }
                Ok(())
            }
            ValidationRule::MinLength(min) => {
                if let Some(Value::String(s)) = value {
                    if s.len() < *min {
                        return Err(SandboxError::ValidationFailed {
                            field: field.to_owned(),
                            reason: format!("string length {} is below minimum {}", s.len(), min),
                        });
                    }
                }
                Ok(())
            }
            ValidationRule::MaxValue(max) => {
                if let Some(v) = value {
                    if let Some(n) = v.as_f64() {
                        if n > *max {
                            return Err(SandboxError::ValidationFailed {
                                field: field.to_owned(),
                                reason: format!("value {n} exceeds maximum {max}"),
                            });
                        }
                    }
                }
                Ok(())
            }
            ValidationRule::MinValue(min) => {
                if let Some(v) = value {
                    if let Some(n) = v.as_f64() {
                        if n < *min {
                            return Err(SandboxError::ValidationFailed {
                                field: field.to_owned(),
                                reason: format!("value {n} is below minimum {min}"),
                            });
                        }
                    }
                }
                Ok(())
            }
            ValidationRule::ForbiddenSubstrings(patterns) => {
                if let Some(Value::String(s)) = value {
                    for pattern in patterns {
                        if s.contains(pattern.as_str()) {
                            return Err(SandboxError::ValidationFailed {
                                field: field.to_owned(),
                                reason: format!("contains forbidden substring '{pattern}'"),
                            });
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ── FieldSchema ───────────────────────────────────────────────────────────────

/// Ordered list of validation rules for a single field.
#[derive(Debug, Clone, Default)]
pub struct FieldSchema {
    rules: Vec<ValidationRule>,
}

impl FieldSchema {
    /// Create an empty schema (no rules — everything passes).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a rule.
    pub fn rule(mut self, rule: ValidationRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Validate `value` against all rules.
    pub fn validate(&self, field: &str, value: Option<&Value>) -> Result<(), SandboxError> {
        for rule in &self.rules {
            rule.check(field, value)?;
        }
        Ok(())
    }
}

// ── InputValidator ────────────────────────────────────────────────────────────

/// Validates a JSON parameter map against registered field schemas.
///
/// Build once, call [`InputValidator::validate`] for every incoming request.
#[derive(Debug, Default)]
pub struct InputValidator {
    schemas: std::collections::HashMap<String, FieldSchema>,
}

impl InputValidator {
    /// Create a validator with no schemas registered.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a schema for `field`.
    pub fn register(mut self, field: impl Into<String>, schema: FieldSchema) -> Self {
        self.schemas.insert(field.into(), schema);
        self
    }

    /// Validate all fields in `params` against the registered schemas.
    ///
    /// Fields with no registered schema are passed through unchanged.
    /// Returns on the first validation failure.
    pub fn validate(&self, params: &serde_json::Map<String, Value>) -> Result<(), SandboxError> {
        for (field, schema) in &self.schemas {
            let value = params.get(field);
            schema.validate(field, value)?;
        }
        Ok(())
    }

    /// Validate a raw JSON `Value` (must be an Object).
    pub fn validate_value(&self, value: &Value) -> Result<(), SandboxError> {
        match value {
            Value::Object(map) => self.validate(map),
            _ => Err(SandboxError::ValidationFailed {
                field: "<root>".to_owned(),
                reason: "expected JSON object for parameters".to_owned(),
            }),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    mod test_validation_rules {
        use super::*;

        #[test]
        fn required_fails_on_missing_field() {
            let r = ValidationRule::Required;
            assert!(matches!(
                r.check("x", None),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn required_fails_on_null() {
            let r = ValidationRule::Required;
            assert!(matches!(
                r.check("x", Some(&Value::Null)),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn required_passes_with_value() {
            let r = ValidationRule::Required;
            assert!(r.check("x", Some(&json!("hello"))).is_ok());
        }

        #[test]
        fn is_string_fails_on_number() {
            let r = ValidationRule::IsString;
            assert!(matches!(
                r.check("x", Some(&json!(42))),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn is_string_passes_on_string() {
            let r = ValidationRule::IsString;
            assert!(r.check("x", Some(&json!("hello"))).is_ok());
        }

        #[test]
        fn is_string_skips_absent_field() {
            let r = ValidationRule::IsString;
            assert!(r.check("x", None).is_ok());
        }

        #[test]
        fn max_length_blocks_long_string() {
            let r = ValidationRule::MaxLength(5);
            assert!(matches!(
                r.check("x", Some(&json!("toolong"))),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn max_length_passes_exact_length() {
            let r = ValidationRule::MaxLength(5);
            assert!(r.check("x", Some(&json!("hello"))).is_ok());
        }

        #[test]
        fn min_value_blocks_low_number() {
            let r = ValidationRule::MinValue(0.0);
            assert!(matches!(
                r.check("x", Some(&json!(-1.0))),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn max_value_blocks_high_number() {
            let r = ValidationRule::MaxValue(100.0);
            assert!(matches!(
                r.check("x", Some(&json!(101.0))),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn forbidden_substrings_blocks_injection() {
            let r = ValidationRule::ForbiddenSubstrings(vec![
                "__import__".to_string(),
                "exec(".to_string(),
            ]);
            let malicious = json!("__import__('os').system('rm -rf /')");
            assert!(matches!(
                r.check("script", Some(&malicious)),
                Err(SandboxError::ValidationFailed { .. })
            ));
            assert!(r.check("script", Some(&json!("safe code"))).is_ok());
        }
    }

    mod test_input_validator {
        use super::*;

        fn build_validator() -> InputValidator {
            InputValidator::new()
                .register(
                    "name",
                    FieldSchema::new()
                        .rule(ValidationRule::Required)
                        .rule(ValidationRule::IsString)
                        .rule(ValidationRule::MaxLength(50)),
                )
                .register(
                    "count",
                    FieldSchema::new()
                        .rule(ValidationRule::IsNumber)
                        .rule(ValidationRule::MinValue(0.0))
                        .rule(ValidationRule::MaxValue(1000.0)),
                )
        }

        #[test]
        fn valid_input_passes() {
            let v = build_validator();
            let params = json!({"name": "sphere", "count": 42});
            assert!(v.validate_value(&params).is_ok());
        }

        #[test]
        fn missing_required_field_fails() {
            let v = build_validator();
            let params = json!({"count": 10});
            assert!(matches!(
                v.validate_value(&params),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn count_below_min_fails() {
            let v = build_validator();
            let params = json!({"name": "sphere", "count": -1});
            assert!(matches!(
                v.validate_value(&params),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn count_above_max_fails() {
            let v = build_validator();
            let params = json!({"name": "sphere", "count": 9999});
            assert!(matches!(
                v.validate_value(&params),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn non_object_root_fails() {
            let v = build_validator();
            assert!(matches!(
                v.validate_value(&json!("not an object")),
                Err(SandboxError::ValidationFailed { .. })
            ));
        }

        #[test]
        fn extra_fields_not_in_schema_pass_through() {
            let v = build_validator();
            let params = json!({"name": "sphere", "count": 1, "extra": true});
            assert!(v.validate_value(&params).is_ok());
        }
    }
}
