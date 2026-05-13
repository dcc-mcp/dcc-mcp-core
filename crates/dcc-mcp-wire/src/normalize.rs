//! Outer argument object normalisation.
//!
//! Provides `normalize_arguments` and `normalize_meta` so that
//! HTTP servers, gateway, and transports never re-implement serde quirks.

use crate::{WireError, WireResult};
use serde_json::{Map, Value, json};

/// Normalise `arguments` to a JSON object.
///
/// - `None` / `Null` / empty-trimmed-string → `{}`
/// - `Object` → returned unchanged
/// - `String` → parsed; must decode to `Object`
/// - any other primitive → `ArgumentsNotObject`
pub fn normalize_arguments(arguments: Option<Value>) -> WireResult<Value> {
    match arguments {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::Object(_)) => Ok(arguments.unwrap()),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(json!({}));
            }
            let parsed: Value =
                serde_json::from_str(trimmed).map_err(|e| WireError::ArgumentsStringNotJson {
                    reason: e.to_string(),
                })?;
            if let Value::Object(_) = parsed {
                Ok(parsed)
            } else {
                Err(WireError::ArgumentsDecodedNotObject {
                    kind: value_kind(&parsed),
                })
            }
        }
        Some(other) => Err(WireError::ArgumentsNotObject {
            kind: value_kind(&other),
        }),
    }
}

/// Normalise `_meta` field to `Option<Map<String, Value>>`.
///
/// - `None` / `Null` / empty-trimmed-string → `None`
/// - `Object` → `Some(map)`
/// - `String` → parsed; must decode to `Object`
pub fn normalize_meta(meta: Option<Value>) -> WireResult<Option<Map<String, Value>>> {
    match meta {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            let parsed: Value =
                serde_json::from_str(trimmed).map_err(|e| WireError::ArgumentsStringNotJson {
                    reason: format!("_meta: {}", e),
                })?;
            match parsed {
                Value::Object(map) => Ok(Some(map)),
                other => Err(WireError::ArgumentsDecodedNotObject {
                    kind: value_kind(&other),
                }),
            }
        }
        Some(other) => Err(WireError::ArgumentsNotObject {
            kind: value_kind(&other),
        }),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn accepts_none_and_null() {
        assert_eq!(normalize_arguments(None).unwrap(), json!({}));
        assert_eq!(normalize_arguments(Some(Value::Null)).unwrap(), json!({}));
    }

    #[test]
    fn accepts_object() {
        let obj = json!({"code": "print(1)"});
        assert_eq!(normalize_arguments(Some(obj.clone())).unwrap(), obj);
    }

    #[test]
    fn parses_json_string() {
        let s = r#"{"code":"print(1)"}"#.to_string();
        let out = normalize_arguments(Some(Value::String(s))).unwrap();
        assert_eq!(out, json!({"code": "print(1)"}));
    }

    #[test]
    fn rejects_non_json_string() {
        let err = normalize_arguments(Some(Value::String("not json".into()))).unwrap_err();
        assert_eq!(err.kind(), "arguments-string-not-json");
    }

    #[test]
    fn rejects_array_string() {
        let err = normalize_arguments(Some(Value::String("[1]".into()))).unwrap_err();
        assert_eq!(err.kind(), "arguments-decoded-not-object");
    }

    #[test]
    fn rejects_array_root() {
        let err = normalize_arguments(Some(json!([1, 2]))).unwrap_err();
        assert_eq!(err.kind(), "arguments-not-object");
    }

    #[test]
    fn rejects_number() {
        let err = normalize_arguments(Some(json!(42))).unwrap_err();
        assert_eq!(err.kind(), "arguments-not-object");
    }

    #[test]
    fn accepts_empty_string() {
        let result = normalize_arguments(Some(Value::String("  ".into()))).unwrap();
        assert_eq!(result, json!({}));
    }

    #[test]
    fn normalize_meta_accepts_object() {
        let mut map = Map::new();
        map.insert("key".to_string(), json!(1));
        let result = normalize_meta(Some(Value::Object(map))).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn normalize_meta_rejects_non_object_string() {
        let err = normalize_meta(Some(Value::String("42".into()))).unwrap_err();
        assert_eq!(err.kind(), "arguments-decoded-not-object");
    }
}
