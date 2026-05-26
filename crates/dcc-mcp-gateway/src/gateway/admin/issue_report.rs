//! GitHub issue-report payload helpers for the Admin API.

use serde_json::{Value, json};

use crate::gateway::response_codec::TOKEN_ESTIMATOR;

pub(super) fn issue_report_filename(request_id: &str) -> String {
    let mut safe = String::with_capacity(request_id.len());
    for ch in request_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            safe.push(ch);
        } else {
            safe.push('-');
        }
    }
    if safe.is_empty() {
        safe.push_str("request");
    }
    format!("dcc-mcp-issue-report-{safe}.json")
}

fn trace_payload_token_summary(trace: &Value) -> Value {
    let input_tokens = trace
        .get("input")
        .and_then(|payload| payload.get("estimated_tokens"))
        .and_then(Value::as_u64);
    let output_tokens = trace
        .get("output")
        .and_then(|payload| payload.get("estimated_tokens"))
        .and_then(Value::as_u64);
    let total_tokens = match (input_tokens, output_tokens) {
        (Some(input), Some(output)) => Some(input.saturating_add(output)),
        (Some(input), None) => Some(input),
        (None, Some(output)) => Some(output),
        (None, None) => None,
    };
    json!({
        "estimator": TOKEN_ESTIMATOR,
        "input": input_tokens,
        "output": output_tokens,
        "total": total_tokens,
    })
}

pub(super) fn issue_report_json(request_id: &str, bundle: Value, links: Value) -> Value {
    let trace = bundle.get("trace").cloned().unwrap_or(Value::Null);
    let audit = bundle.get("audit").cloned().unwrap_or(Value::Null);
    let tool = trace
        .get("tool_slug")
        .or_else(|| trace.get("method"))
        .or_else(|| audit.get("tool"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let dcc_type = trace
        .get("dcc_type")
        .or_else(|| audit.get("dcc_type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let status = trace
        .get("ok")
        .and_then(Value::as_bool)
        .or_else(|| audit.get("success").and_then(Value::as_bool))
        .map(|ok| if ok { "ok" } else { "failed" })
        .unwrap_or("unknown");
    let total_ms = trace
        .get("total_ms")
        .or_else(|| audit.get("duration_ms"))
        .cloned()
        .unwrap_or(Value::Null);
    let token_accounting = trace
        .get("token_accounting")
        .or_else(|| audit.get("token_accounting"))
        .cloned()
        .unwrap_or(Value::Null);
    let payload_tokens = trace_payload_token_summary(&trace);
    let trace_id = bundle
        .get("trace_id")
        .cloned()
        .unwrap_or_else(|| trace.get("trace_id").cloned().unwrap_or(Value::Null));
    let postmortem = bundle.get("postmortem").cloned().unwrap_or(Value::Null);
    let previous_call_count = postmortem
        .get("previous_calls")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let gateway_event_count = postmortem
        .get("gateway_events")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let generated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let title = format!("DCC-MCP request {request_id} {status}: {tool}");
    let body_template = format!(
        "## Summary\n\nRequest `{request_id}` returned `{status}` for `{tool}` on `{dcc_type}`.\n\n## Attached data\n\nUpload this JSON export to the issue so maintainers can inspect trace spans, audit metadata, payload previews, postmortem context, and links.\n\n## Notes\n\nReview the JSON for secrets or proprietary scene paths before uploading."
    );

    json!({
        "schema_version": "dcc-mcp.admin.issue-report.v1",
        "report_type": "github_issue_debug_json",
        "generated_at": generated_at,
        "request_id": request_id,
        "trace_id": trace_id,
        "summary": {
            "title": title,
            "status": status,
            "tool": tool,
            "dcc_type": dcc_type,
            "total_ms": total_ms,
            "token_accounting": token_accounting,
            "payload_tokens": payload_tokens,
            "postmortem": {
                "previous_call_count": previous_call_count,
                "gateway_event_count": gateway_event_count,
            },
        },
        "github_issue": {
            "title": title,
            "body_template": body_template,
            "suggested_labels": ["bug", "admin-telemetry"],
        },
        "links": links,
        "privacy_note": "Review request and response payloads before uploading; this export may include scene paths, prompts, tokens, or proprietary data.",
        "debug_bundle": bundle,
    })
}
