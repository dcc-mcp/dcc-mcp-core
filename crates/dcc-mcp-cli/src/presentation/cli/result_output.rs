//! CLI-only views; the MCP response and persisted acceptance evidence stay intact.

use serde_json::{Map, Value};

/// Remove only redundant, unannotated JSON text mirrors of structuredContent.
pub(super) fn canonical_result(value: &mut Value) {
    if let Some(results) = value.get_mut("results").and_then(Value::as_array_mut) {
        for row in results {
            canonical_result(row);
        }
    }
    let Some(result) = value.get_mut("result") else {
        return;
    };
    let Some(structured) = result.get("structuredContent").cloned() else {
        return;
    };
    if let Some(content) = result.get_mut("content").and_then(Value::as_array_mut) {
        content.retain(|block| {
            let mirror = block.as_object().is_some_and(|object| {
                object.len() == 2
                    && block.get("type").and_then(Value::as_str) == Some("text")
                    && block
                        .get("text")
                        .and_then(Value::as_str)
                        .and_then(|text| serde_json::from_str::<Value>(text).ok())
                        .is_some_and(|payload| payload == structured)
            });
            !mirror
        });
    }
}

pub(super) fn parse_pointer(pointer: &str) -> Result<String, String> {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(
            "--select expects an RFC 6901 JSON pointer (for example /result/structuredContent)"
                .into(),
        );
    }
    let mut chars = pointer.chars();
    while let Some(ch) = chars.next() {
        if ch == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return Err("JSON pointer escapes must be ~0 or ~1".into());
        }
    }
    Ok(pointer.to_string())
}

/// Missing fields are null; selection never turns a completed mutation into a retry.
pub(super) fn select_fields(value: Value, pointers: &[String]) -> Value {
    if pointers.is_empty() {
        return value;
    }
    let selected: Map<String, Value> = pointers
        .iter()
        .map(|pointer| {
            (
                pointer.clone(),
                value.pointer(pointer).cloned().unwrap_or(Value::Null),
            )
        })
        .collect();
    let mut output = serde_json::json!({"selected": selected});
    for key in ["success", "request_id", "tool_slug"] {
        if let Some(field) = value.get(key) {
            output[key] = field.clone();
        }
    }
    output
}

#[cfg(test)]
#[path = "result_output_tests.rs"]
mod tests;
