//! Result projection shared by the CLI capability command and its tests.
//! Successful results keep the backend payload verbatim so machine consumers
//! retain `evidence_ref`, `declaration_only`, and routing metadata. A backend
//! failure is wrapped with the resolved instance identity and the tool name
//! so operators can see which adapter answered with the error.

use serde_json::{Value, json};

use super::local_control::call_result_succeeded;

#[must_use]
pub fn backend_summary(result: &Value) -> Option<&Value> {
    for pointer in [
        "/result/structuredContent/context",
        "/structuredContent/context",
        "/context",
        "/output",
    ] {
        if let Some(summary) = result.pointer(pointer) {
            return Some(summary);
        }
    }
    None
}

#[must_use]
pub fn project_result(result: Value, backend_ok: bool) -> Value {
    if backend_ok {
        return result;
    }
    let mut original = result.clone();
    original["ok"] = json!(call_result_succeeded(&result));
    let mut projected = json!({
        "ok": false,
        "error": original,
        "backend_tool": "dcc_host_capabilities",
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

#[cfg(test)]
#[path = "capabilities_result_tests.rs"]
mod tests;
