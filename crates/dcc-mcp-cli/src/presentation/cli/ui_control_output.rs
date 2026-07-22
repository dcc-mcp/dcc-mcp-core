use serde_json::{Map, Value};

pub(super) fn compact_ui_control_result(tool_name: &str, value: &Value) -> Value {
    let payload = [value.get("result"), value.get("output"), Some(value)]
        .into_iter()
        .flatten()
        .find_map(|candidate| {
            crate::application::local_control::call_result_payload(candidate).or_else(|| {
                (candidate.get("context").is_some()
                    || candidate.get("message").is_some()
                    || candidate.get("job_id").is_some())
                .then(|| candidate.clone())
            })
        })
        .unwrap_or_else(|| value.clone());
    let mut compact = Map::new();
    compact.insert(
        "success".to_string(),
        Value::Bool(crate::application::local_control::call_result_succeeded(
            value,
        )),
    );
    compact.insert("tool".to_string(), Value::String(tool_name.to_string()));

    for key in [
        "tool_slug",
        "backend_tool",
        "dcc_type",
        "instance_id",
        "instance_short",
        "source",
    ] {
        copy_object_field(&mut compact, value, key);
    }
    for key in ["message", "prompt", "error", "possible_solutions"] {
        copy_non_null_field(&mut compact, &payload, key);
    }
    for key in ["job_id", "status", "parent_job_id", "progress_token"] {
        copy_non_null_field(&mut compact, &payload, key);
    }
    if let Some(context) = payload.get("context").and_then(Value::as_object) {
        for (key, field) in context {
            if !matches!(key.as_str(), "policy" | "audit") {
                compact.insert(key.clone(), field.clone());
            }
        }
    }
    if let Some(snapshot) = compact.get_mut("snapshot").and_then(Value::as_object_mut) {
        snapshot.remove("root");
    }
    Value::Object(compact)
}

fn copy_object_field(target: &mut Map<String, Value>, source: &Value, key: &str) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_string(), value.clone());
    }
}

fn copy_non_null_field(target: &mut Map<String, Value>, source: &Value, key: &str) {
    if let Some(value) = source.get(key).filter(|value| !value.is_null()) {
        target.insert(key.to_string(), value.clone());
    }
}
