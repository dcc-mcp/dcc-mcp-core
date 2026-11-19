//! Human-readable projection for `dcc-mcp-cli capabilities`.
//! Structured formats return the backend summary unchanged, including
//! `evidence_ref`, `declaration_only`, and routing metadata. Human output only
//! compresses that same summary into a one-screen planning answer.

use serde_json::Value;

use crate::application::capabilities_result::backend_summary;

const GROUPS: [(&str, &str); 4] = [
    ("available", "AVAILABLE"),
    ("declared", "DECLARED"),
    ("unavailable", "UNAVAILABLE"),
    ("not_run", "NOT_RUN"),
];

#[must_use]
pub(crate) fn render_human(value: &Value) -> String {
    let owned_summary = backend_summary(value);
    let summary = owned_summary.as_ref().unwrap_or(value);
    let status = if value.get("ok").and_then(Value::as_bool) == Some(false)
        || value.get("success").and_then(Value::as_bool) == Some(false)
    {
        "FAILED"
    } else {
        "OK"
    };
    let mut lines = vec![format!(
        "status={status} host={} adapter={} probed_at={}",
        field(summary, "host_version"),
        field(summary, "adapter_version"),
        field(summary, "probed_at")
    )];
    let tool = value.get("backend_tool").and_then(Value::as_str);
    let dcc_type = value.get("dcc_type").and_then(Value::as_str).unwrap_or("-");
    let instance = value
        .get("instance_short")
        .or_else(|| value.get("instance_id"))
        .and_then(Value::as_str)
        .unwrap_or("-");
    if let Some(tool) = tool {
        lines.push(format!("tool={tool} host={dcc_type}/{instance}"));
    }

    for (key, label) in GROUPS {
        let entries = summary
            .get(key)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        lines.push(format!("{label} ({})", entries.len()));
        if entries.is_empty() {
            lines.push("  -".to_string());
            continue;
        }
        for entry in entries {
            let mut line = format!(
                "  {} [{}]",
                field(entry, "key"),
                field(entry, "probe_result")
            );
            if entry.get("declaration_only").and_then(Value::as_bool) == Some(true) {
                line.push_str(" declaration_only");
            }
            if let Some(reason) = entry.get("reason").and_then(Value::as_str) {
                line.push_str(" reason=");
                line.push_str(reason);
            }
            lines.push(line);
        }
    }

    lines.join("\n")
}

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .unwrap_or("-")
}

#[cfg(test)]
#[path = "capabilities_output_tests.rs"]
mod tests;
