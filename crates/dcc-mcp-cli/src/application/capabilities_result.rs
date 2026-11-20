//! Result projection shared by the CLI capability command and its tests.
//! A result is successful only when the backend accepted the call and exposed
//! a valid summary object. Successful results keep the backend payload verbatim
//! so machine consumers retain `evidence_ref`, `declaration_only`, and routing
//! metadata. A failure is wrapped with the resolved instance identity and the
//! tool name so operators can see which adapter answered with the error.

use serde_json::{Value, json};

use super::local_control::call_result_succeeded;
use crate::domain::capabilities::DESCRIBE_CAPABILITIES_TOOL;

#[must_use]
pub fn backend_summary(result: &Value) -> Option<Value> {
    for pointer in ["/result/content", "/content"] {
        let Some(text) = result
            .pointer(pointer)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Ok(payload) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if let Some(summary) = backend_summary_ref(&payload) {
            return Some(summary.clone());
        }
    }
    backend_summary_ref(result).cloned()
}

#[must_use]
pub fn backend_summary_ref(result: &Value) -> Option<&Value> {
    for pointer in [
        "/result/structuredContent/context",
        "/structuredContent/context",
        "/context",
        "/output/context",
        "/result/output/context",
    ] {
        if let Some(summary) = result.pointer(pointer) {
            return Some(summary);
        }
    }
    None
}

#[must_use]
pub fn project_result(result: Value) -> Value {
    if command_succeeded(&result) {
        return result;
    }
    let mut original = result.clone();
    original["ok"] = json!(call_result_succeeded(&result));
    let backend_tool = result
        .get("backend_tool")
        .and_then(Value::as_str)
        .unwrap_or(DESCRIBE_CAPABILITIES_TOOL);
    let mut projected = json!({
        "ok": false,
        "error": error_body(&result, &original),
        "backend_tool": backend_tool,
    });
    for (field, key) in [
        ("dcc_type", "dcc_type"),
        ("instance_id", "instance_id"),
        ("instance_short", "instance_short"),
    ] {
        let value = result
            .get(key)
            .or_else(|| (field == "dcc_type").then(|| result.get("dcc")).flatten());
        projected[field] = value.cloned().unwrap_or(Value::Null);
    }
    projected
}

#[must_use]
pub fn command_succeeded(result: &Value) -> bool {
    call_result_succeeded(result)
        && backend_summary(result).is_some_and(|summary| summary.is_object())
}

fn error_body(result: &Value, original: &Value) -> Value {
    if call_result_succeeded(result) {
        return json!({
            "code": "invalid_capabilities_response",
            "message": "Backend succeeded without a valid capabilities summary",
            "response": original,
        });
    }
    original.clone()
}

#[cfg(test)]
#[path = "capabilities_result_tests.rs"]
mod tests;
