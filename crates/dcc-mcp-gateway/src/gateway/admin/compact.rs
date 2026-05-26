//! Compact projections for agent-facing admin/debug responses.

use serde_json::{Value, json};

#[derive(Default)]
struct PayloadPreviewStats {
    preview_count: usize,
    truncated_count: usize,
    redacted_marker_count: usize,
}

pub(crate) fn compact_debug_bundle_payload(bundle: &Value) -> Value {
    let postmortem = bundle.get("postmortem").unwrap_or(&Value::Null);
    let trace = bundle.get("trace").unwrap_or(&Value::Null);
    json!({
        "schema_version": "dcc-mcp.admin.debug-summary.v1",
        "lookup_id": field(bundle, &["lookup_id"]),
        "request_id": field(bundle, &["request_id"]),
        "trace_id": field(bundle, &["trace_id"]),
        "request_ids": field(bundle, &["request_ids"]),
        "status": status_for_bundle(bundle),
        "root_cause": sanitized_root_cause(first_present(&[
            field(bundle, &["root_cause"]),
            first_hint(bundle),
        ])),
        "tool": first_present(&[
            field(trace, &["tool_slug"]),
            field(trace, &["method"]),
            field(postmortem, &["target", "tool"]),
        ]),
        "dcc_type": first_present(&[
            field(trace, &["dcc_type"]),
            field(postmortem, &["target", "dcc_type"]),
        ]),
        "total_ms": first_present(&[
            field(trace, &["total_ms"]),
            field(postmortem, &["target", "total_ms"]),
        ]),
        "token_accounting": first_present(&[
            field(trace, &["token_accounting"]),
            field(bundle, &["audit", "token_accounting"]),
        ]),
        "redaction": payload_preview_summary(bundle),
        "postmortem": {
            "previous_call_count": array_len(postmortem, &["previous_calls"]),
            "gateway_event_count": array_len(postmortem, &["gateway_events"]),
        },
        "links": field(bundle, &["links"]),
        "hints": field(bundle, &["hints"]),
    })
}

pub(crate) fn compact_trace_detail_payload(trace: &Value) -> Value {
    json!({
        "schema_version": "dcc-mcp.admin.trace-summary.v1",
        "request_id": field(trace, &["request_id"]),
        "trace_id": field(trace, &["trace_id"]),
        "parent_request_id": field(trace, &["parent_request_id"]),
        "status": status_for_trace(trace),
        "tool": first_present(&[
            field(trace, &["tool_slug"]),
            field(trace, &["tool"]),
            field(trace, &["method"]),
        ]),
        "dcc_type": field(trace, &["dcc_type"]),
        "instance_id": field(trace, &["instance_id"]),
        "transport": field(trace, &["transport"]),
        "total_ms": field(trace, &["total_ms"]),
        "span_count": array_len(trace, &["spans"]),
        "slowest_span": slowest_span(trace),
        "payload_tokens": {
            "input_tokens": field(trace, &["input_tokens"]),
            "output_tokens": field(trace, &["output_tokens"]),
            "total_tokens": first_present(&[
                field(trace, &["total_tokens"]),
                field(trace, &["estimated_total_tokens"]),
            ]),
            "token_estimator": field(trace, &["payload_token_estimator"]),
        },
        "response_token_accounting": field(trace, &["token_accounting"]),
        "redaction": payload_preview_summary(trace),
        "links": field(trace, &["links"]),
    })
}

pub(crate) fn compact_trace_list_payload(payload: &Value) -> Value {
    let traces: Vec<Value> = payload
        .get("traces")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(compact_trace_detail_payload).collect())
        .unwrap_or_default();
    json!({
        "total": field(payload, &["total"]),
        "traces": traces,
        "links": field(payload, &["links"]),
    })
}

pub(crate) fn compact_trace_context_payload(payload: &Value) -> Value {
    json!({
        "lookup_id": field(payload, &["lookup_id"]),
        "request_id": field(payload, &["request_id"]),
        "trace_id": field(payload, &["trace_id"]),
        "request_ids": field(payload, &["request_ids"]),
        "trace": compact_trace_detail_payload(payload.get("trace").unwrap_or(&Value::Null)),
        "trace_count": array_len(payload, &["traces"]),
        "links": field(payload, &["links"]),
    })
}

fn field(value: &Value, path: &[&str]) -> Value {
    let mut current = value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return Value::Null;
        };
        current = next;
    }
    current.clone()
}

fn first_present(values: &[Value]) -> Value {
    values
        .iter()
        .find(|value| !value.is_null())
        .cloned()
        .unwrap_or(Value::Null)
}

fn sanitized_root_cause(value: Value) -> Value {
    let Some(text) = value.as_str() else {
        return value;
    };
    let lower = text.to_ascii_lowercase();
    let kind = if lower.contains("timeout") || lower.contains("timed out") {
        "timeout"
    } else if lower.contains("auth") || lower.contains("permission") || lower.contains("forbidden")
    {
        "auth"
    } else if lower.contains("not found") || lower.contains("unknown") {
        "not found"
    } else if lower.contains("host_died")
        || lower.contains("host died")
        || lower.contains("connection refused")
        || lower.contains("unreachable")
        || lower.contains("disconnected")
        || lower.contains("backend unavailable")
    {
        "host died"
    } else if lower.contains("invalid") || lower.contains("validation") {
        "validation"
    } else if text.trim().is_empty() {
        return Value::Null;
    } else {
        "error"
    };
    json!(kind)
}

fn first_hint(bundle: &Value) -> Value {
    bundle
        .get("hints")
        .and_then(Value::as_array)
        .and_then(|hints| hints.first())
        .cloned()
        .unwrap_or(Value::Null)
}

fn array_len(value: &Value, path: &[&str]) -> usize {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn status_for_bundle(bundle: &Value) -> Value {
    first_present(&[
        field(bundle, &["audit", "status"]),
        status_for_trace(bundle.get("trace").unwrap_or(&Value::Null)),
        field(bundle, &["postmortem", "target", "status"]),
    ])
}

fn status_for_trace(trace: &Value) -> Value {
    if let Some(status) = trace.get("status").filter(|value| !value.is_null()) {
        return status.clone();
    }
    match trace.get("ok").and_then(Value::as_bool) {
        Some(true) => json!("ok"),
        Some(false) => json!("err"),
        None => Value::Null,
    }
}

fn slowest_span(trace: &Value) -> Value {
    let Some(spans) = trace.get("spans").and_then(Value::as_array) else {
        return Value::Null;
    };
    spans
        .iter()
        .max_by_key(|span| span.get("duration_ns").and_then(Value::as_u64).unwrap_or(0))
        .map(|span| {
            json!({
                "name": field(span, &["name"]),
                "duration_ms": span
                    .get("duration_ns")
                    .and_then(Value::as_u64)
                    .map(|ns| ns / 1_000_000)
                    .unwrap_or(0),
                "ok": field(span, &["ok"]),
            })
        })
        .unwrap_or(Value::Null)
}

fn payload_preview_summary(value: &Value) -> Value {
    let mut stats = PayloadPreviewStats::default();
    visit_payloads(value, &mut stats);
    json!({
        "payload_previews_omitted": true,
        "payload_preview_count": stats.preview_count,
        "truncated_payload_count": stats.truncated_count,
        "redacted_marker_count": stats.redacted_marker_count,
    })
}

fn visit_payloads(value: &Value, stats: &mut PayloadPreviewStats) {
    match value {
        Value::Object(map) => {
            if map.contains_key("mime_type") && map.contains_key("original_size") {
                stats.preview_count += 1;
                if map.get("truncated").and_then(Value::as_bool) == Some(true) {
                    stats.truncated_count += 1;
                }
                if map
                    .get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|content| {
                        let lower = content.to_ascii_lowercase();
                        lower.contains("redacted") || content.contains("[REDACTED]")
                    })
                {
                    stats.redacted_marker_count += 1;
                }
                return;
            }
            for child in map.values() {
                visit_payloads(child, stats);
            }
        }
        Value::Array(items) => {
            for child in items {
                visit_payloads(child, stats);
            }
        }
        _ => {}
    }
}
