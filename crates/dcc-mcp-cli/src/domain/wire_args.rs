//! Client-side argument normalisation shared with the server.
//!
//! Both the gateway REST path (`/v1/call`) and the MCP `tools/call` path used
//! to forward `arguments` / `meta` verbatim, while the server normalises them
//! through `dcc-mcp-wire`. That split meant a payload accepted by one side
//! could be rejected by the other. These helpers make the CLI apply the same
//! normalisation before a request leaves the process (B2, ADR-037).

use dcc_mcp_wire::{WireError, normalize_arguments, normalize_meta};
use serde_json::{Map, Value};

/// Normalised call payload: `(arguments, meta)`.
///
/// `arguments` is always a JSON object; `meta` is `None` when the caller
/// supplied nothing meaningful.
pub type NormalizedArgs = (Value, Option<Map<String, Value>>);

/// Normalise `arguments` and `meta` with the server's own wire rules.
///
/// # Errors
///
/// Returns [`WireError`] when `arguments` is present but is neither a JSON
/// object nor a string decoding to one, and likewise for `meta`.
pub fn normalize_call_args(
    arguments: Value,
    meta: Option<Value>,
) -> Result<NormalizedArgs, WireError> {
    let arguments = normalize_arguments(Some(arguments))?;
    let meta = normalize_meta(meta)?;
    Ok((arguments, meta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_null_arguments_to_empty_object() {
        let (arguments, meta) = normalize_call_args(Value::Null, None).unwrap();
        assert_eq!(arguments, json!({}));
        assert!(meta.is_none());
    }

    #[test]
    fn normalizes_empty_string_arguments_to_empty_object() {
        let (arguments, _) = normalize_call_args(Value::String("  ".into()), None).unwrap();
        assert_eq!(arguments, json!({}));
    }

    #[test]
    fn unwraps_json_string_arguments() {
        let (arguments, _) =
            normalize_call_args(Value::String(r#"{"radius": 2}"#.into()), None).unwrap();
        assert_eq!(arguments, json!({"radius": 2}));
    }

    #[test]
    fn passes_object_arguments_through() {
        let (arguments, _) = normalize_call_args(json!({"radius": 2}), None).unwrap();
        assert_eq!(arguments, json!({"radius": 2}));
    }

    #[test]
    fn rejects_non_object_arguments() {
        let error = normalize_call_args(json!([1, 2]), None).unwrap_err();
        assert_eq!(error.kind(), "arguments-not-object");
    }

    #[test]
    fn rejects_non_json_string_arguments() {
        let error = normalize_call_args(Value::String("not json".into()), None).unwrap_err();
        assert_eq!(error.kind(), "arguments-string-not-json");
    }

    #[test]
    fn normalizes_meta_object() {
        let (_, meta) = normalize_call_args(json!({}), Some(json!({"agent_context": 1}))).unwrap();
        assert_eq!(
            meta,
            Some(json!({"agent_context": 1}).as_object().cloned().unwrap())
        );
    }

    #[test]
    fn drops_empty_meta() {
        let (_, meta) = normalize_call_args(json!({}), Some(Value::Null)).unwrap();
        assert!(meta.is_none());
    }

    #[test]
    fn unwraps_json_string_meta() {
        let (_, meta) =
            normalize_call_args(json!({}), Some(Value::String(r#"{"k": 1}"#.into()))).unwrap();
        assert!(meta.is_some());
    }
}
