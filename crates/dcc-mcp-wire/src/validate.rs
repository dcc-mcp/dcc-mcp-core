//! Argument shape validation and schema checks.

use crate::{WireError, WireResult};
use serde_json::Value;

/// Validate that `arguments` is a JSON object (after normalisation).
pub fn validate_arguments_shape(arguments: &Value) -> WireResult<()> {
    match arguments {
        Value::Object(_) => Ok(()),
        other => Err(WireError::ArgumentsNotObject {
            kind: value_kind(other),
        }),
    }
}

/// Validate `arguments` against a declared JSON Schema.
pub fn validate_arguments(arguments: &Value, input_schema: Option<&Value>) -> WireResult<()> {
    let Some(schema) = input_schema else {
        return Ok(());
    };

    let Value::Object(schema_map) = schema else {
        return Ok(());
    };

    let Some(Value::String(type_)) = schema_map.get("type") else {
        return Ok(());
    };

    if type_ != "object" {
        return Ok(());
    }

    let Some(Value::Array(required)) = schema_map.get("required") else {
        return Ok(());
    };

    let Value::Object(args_map) = arguments else {
        return Ok(());
    };

    for req in required {
        let Value::String(field) = req else { continue };
        if !args_map.contains_key(field) {
            return Err(WireError::SchemaValidationFailed {
                reason: format!("missing required field: {field}"),
            });
        }
    }

    Ok(())
}

/// Validate a `tools/call` request envelope.
pub fn validate_call_tool_params(
    name: Option<&str>,
    arguments: Option<Value>,
    input_schema: Option<&Value>,
) -> WireResult<Value> {
    let Some(name) = name else {
        return Err(WireError::MissingField {
            field: "name".to_string(),
        });
    };
    if name.is_empty() {
        return Err(WireError::InvalidToolSlug {
            reason: "tool name must not be empty".to_string(),
        });
    }

    let normalised = crate::normalize::normalize_arguments(arguments)?;
    validate_arguments_shape(&normalised)?;
    validate_arguments(&normalised, input_schema)?;
    Ok(normalised)
}

/// Validate a `call_batch` request envelope.
pub fn validate_call_batch_params(calls: &[Value]) -> WireResult<Vec<(String, Value)>> {
    let mut results = Vec::new();
    for (index, call) in calls.iter().enumerate() {
        let obj = match call {
            Value::Object(map) => map,
            _ => {
                return Err(WireError::BatchItemInvalid {
                    index,
                    reason: "each batch item must be a JSON object".to_string(),
                });
            }
        };

        let name = obj
            .get("name")
            .or_else(|| obj.get("tool_slug"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| WireError::BatchItemInvalid {
                index,
                reason: "missing `name` or `tool_slug` field".to_string(),
            })?;

        let arguments = obj.get("arguments").or_else(|| obj.get("params")).cloned();

        let normalised = crate::normalize::normalize_arguments(arguments).map_err(|e| {
            WireError::BatchItemInvalid {
                index,
                reason: e.to_string(),
            }
        })?;
        validate_arguments_shape(&normalised).map_err(|e| WireError::BatchItemInvalid {
            index,
            reason: e.to_string(),
        })?;

        results.push((name.to_string(), normalised));
    }
    Ok(results)
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
