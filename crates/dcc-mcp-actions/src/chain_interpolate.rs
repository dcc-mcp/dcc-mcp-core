//! Context merge and `{key}` placeholder interpolation for [`ActionChain`](super::ActionChain).

use serde_json::{Map, Value};

// ── Context helpers ───────────────────────────────────────────────────────────

/// Merge top-level keys from `src` into the context value.
///
/// If the context is not a JSON object, it is replaced by an object
/// containing only the merged keys.
pub(crate) fn merge_into_context(context: &mut Value, src: &Map<String, Value>) {
    if let Value::Object(map) = context {
        for (k, v) in src {
            map.insert(k.clone(), v.clone());
        }
    } else {
        let mut map = Map::new();
        for (k, v) in src {
            map.insert(k.clone(), v.clone());
        }
        *context = Value::Object(map);
    }
}

// ── Placeholder interpolation ─────────────────────────────────────────────────

/// Recursively replace `"{key}"` string segments with values from `context`.
///
/// Only simple `{word}` patterns are supported (consistent with the workflow
/// skill's `_interpolate` function). If the entire string matches a single
/// placeholder and the context value is not a string, the original JSON type
/// is returned (e.g. `"{count}"` → `Value::Number(42)`).
pub(crate) fn interpolate(value: &Value, context: &Value) -> Value {
    match value {
        Value::String(s) => interpolate_string(s, context),
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), interpolate(v, context));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| interpolate(v, context)).collect()),
        other => other.clone(),
    }
}

/// Interpolate `{key}` placeholders inside a string value.
///
/// If the entire string is a single `{key}` token, the context value is
/// returned directly (preserving its JSON type). Otherwise, string replacement
/// is performed using the `to_string()` representation of context values.
pub(crate) fn interpolate_string(s: &str, context: &Value) -> Value {
    // Fast path: entire string is a single placeholder
    if s.starts_with('{') && s.ends_with('}') && s.len() > 2 {
        let key = &s[1..s.len() - 1];
        // Only treat as placeholder if key has no inner braces
        if !key.contains('{') && !key.contains('}') {
            if let Some(v) = context.get(key) {
                return v.clone();
            }
        }
    }

    // General case: replace all {key} occurrences with string representations
    let mut result = String::with_capacity(s.len());
    let mut remaining = s;
    while let Some(open) = remaining.find('{') {
        result.push_str(&remaining[..open]);
        remaining = &remaining[open + 1..];
        if let Some(close) = remaining.find('}') {
            let key = &remaining[..close];
            remaining = &remaining[close + 1..];
            if !key.is_empty() && !key.contains('{') {
                if let Some(v) = context.get(key) {
                    result.push_str(&value_to_string(v));
                } else {
                    // Key not found: leave placeholder intact
                    result.push('{');
                    result.push_str(key);
                    result.push('}');
                }
            } else {
                result.push('{');
                result.push_str(key);
                result.push('}');
            }
        } else {
            // No closing brace found; emit the rest as-is
            result.push('{');
            result.push_str(remaining);
            remaining = "";
        }
    }
    result.push_str(remaining);
    Value::String(result)
}

pub(crate) fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

// ── Convenience builder helpers ───────────────────────────────────────────────

/// Helper for building a context map for [`ActionChain::run`](super::ActionChain::run).
///
/// ```rust
/// use dcc_mcp_actions::chain::context;
/// let ctx = context([("export_path", "/tmp/out.fbx"), ("frame", "42")]);
/// ```
pub fn context<I, K, V>(entries: I) -> Value
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<Value>,
{
    let mut map = Map::new();
    for (k, v) in entries {
        map.insert(k.into(), v.into());
    }
    Value::Object(map)
}
