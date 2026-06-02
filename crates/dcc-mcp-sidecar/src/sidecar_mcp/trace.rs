use axum::http::HeaderMap;
use serde_json::{Value, json};

pub(super) fn trace_context_from_headers(headers: &HeaderMap, request_id: &str) -> Option<Value> {
    let traceparent = header_str(headers, "traceparent");
    let trace_id = traceparent
        .as_deref()
        .and_then(parse_traceparent_trace_id)
        .or_else(|| header_str(headers, "x-trace-id"));
    let trace_id = trace_id?;
    let parent_span_id = traceparent
        .as_deref()
        .and_then(parse_traceparent_parent_span_id);
    let trace_flags = traceparent.as_deref().and_then(parse_traceparent_flags);
    Some(json!({
        "trace_id": trace_id,
        "request_id": request_id,
        "parent_span_id": parent_span_id,
        "parent_request_id": header_str(headers, "x-dcc-mcp-parent-request-id"),
        "trace_flags": trace_flags,
        "trace_state": header_str(headers, "tracestate"),
    }))
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_traceparent_trace_id(value: &str) -> Option<String> {
    let trace_id = value.trim().split('-').nth(1)?;
    (trace_id.len() == 32).then(|| trace_id.to_ascii_lowercase())
}

fn parse_traceparent_parent_span_id(value: &str) -> Option<String> {
    let span_id = value.trim().split('-').nth(2)?;
    (span_id.len() == 16).then(|| span_id.to_ascii_lowercase())
}

fn parse_traceparent_flags(value: &str) -> Option<String> {
    let flags = value.trim().split('-').nth(3)?;
    (flags.len() == 2).then(|| flags.to_ascii_lowercase())
}
