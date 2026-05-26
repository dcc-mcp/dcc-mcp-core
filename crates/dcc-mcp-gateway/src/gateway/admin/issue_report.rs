//! GitHub issue-report payload helpers for the Admin API.

use serde_json::{Value, json};

use crate::gateway::response_codec::TOKEN_ESTIMATOR;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IssueReportMode {
    PublicSafe,
    RawDebugBundle,
}

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

fn public_label(value: &str) -> String {
    let mut safe = String::with_capacity(value.len().min(96));
    for ch in value.chars().take(96) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            safe.push(ch);
        } else {
            safe.push('-');
        }
    }
    let safe = safe.trim_matches('-').to_string();
    if safe.is_empty() {
        "unknown".to_string()
    } else {
        safe
    }
}

fn tool_family(tool: &str) -> String {
    let last_segment = tool
        .split('.')
        .rfind(|part| !part.is_empty())
        .unwrap_or(tool);
    let family = last_segment
        .rsplit_once("__")
        .map(|(_, tail)| tail)
        .unwrap_or(last_segment);
    public_label(family)
}

fn encode_url_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn public_safe_links(request_id: &str) -> Value {
    let encoded = encode_url_component(request_id);
    json!({
        "admin_trace_path": format!("/admin?panel=traces&trace={encoded}"),
        "trace_api_path": format!("/admin/api/traces/{encoded}"),
        "debug_bundle_path": format!("/admin/api/debug-bundle/{encoded}"),
        "safe_issue_report_path": format!("/admin/api/issue-report/{encoded}"),
        "raw_issue_report_path": format!("/admin/api/issue-report/{encoded}?mode=raw"),
        "stable_debug_bundle_path": format!("/v1/debug/bundles/{encoded}"),
        "stable_safe_issue_report_path": format!("/v1/debug/issue-reports/{encoded}"),
        "stable_raw_issue_report_path": format!("/v1/debug/issue-reports/{encoded}?mode=raw"),
        "openapi_spec_path": "/v1/openapi.json",
        "docs_path": "/docs",
    })
}

fn redaction_marker_detected(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("[REDACTED]"),
        Value::Array(items) => items.iter().any(redaction_marker_detected),
        Value::Object(map) => map.values().any(redaction_marker_detected),
        _ => false,
    }
}

fn find_error_text(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => {
            if let Some(text) = map
                .get("error")
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
            {
                return Some(text);
            }
            map.values().find_map(find_error_text)
        }
        Value::Array(items) => items.iter().find_map(find_error_text),
        _ => None,
    }
}

fn contains_text_marker(value: &Value, markers: &[&str]) -> bool {
    match value {
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            markers.iter().any(|marker| lower.contains(marker))
        }
        Value::Array(items) => items.iter().any(|item| contains_text_marker(item, markers)),
        Value::Object(map) => map
            .values()
            .any(|value| contains_text_marker(value, markers)),
        _ => false,
    }
}

fn failed_without_error(trace: &Value, audit: &Value) -> bool {
    trace
        .get("ok")
        .and_then(Value::as_bool)
        .is_some_and(|ok| !ok)
        || audit
            .get("success")
            .and_then(Value::as_bool)
            .is_some_and(|success| !success)
}

fn sanitized_error_kind(trace: &Value, audit: &Value, bundle: &Value) -> Value {
    let raw = trace
        .get("error")
        .or_else(|| audit.get("error"))
        .and_then(Value::as_str)
        .or_else(|| find_error_text(bundle));
    let raw = raw.or_else(|| {
        if contains_text_marker(
            bundle,
            &[
                "host_died",
                "host died",
                "connection refused",
                "unreachable",
                "disconnected",
            ],
        ) {
            Some("backend unavailable")
        } else if failed_without_error(trace, audit) {
            Some("failed")
        } else {
            None
        }
    });
    let Some(raw) = raw else {
        return json!({
            "kind": null,
            "present": false,
            "message_redacted": false,
        });
    };

    let lower = raw.to_ascii_lowercase();
    let kind = if lower.contains("timeout") || lower.contains("timed out") {
        "timeout"
    } else if lower.contains("auth") || lower.contains("permission") || lower.contains("forbidden")
    {
        "auth"
    } else if lower.contains("not found") || lower.contains("unknown") {
        "not-found"
    } else if lower.contains("host_died")
        || lower.contains("host died")
        || lower.contains("connection refused")
        || lower.contains("unreachable")
        || lower.contains("disconnected")
        || lower.contains("backend unavailable")
    {
        "backend-unavailable"
    } else if lower.contains("invalid") || lower.contains("validation") {
        "validation"
    } else {
        "error"
    };

    json!({
        "kind": kind,
        "present": true,
        "message_redacted": true,
    })
}

fn build_summary(request_id: &str, bundle: &Value) -> (Value, String) {
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
    let dcc_type = public_label(dcc_type);
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
    let tool_family = tool_family(tool);
    let title = format!("DCC-MCP request {request_id} {status}: {tool_family}");
    let error = sanitized_error_kind(&trace, &audit, bundle);
    let summary = json!({
        "title": title,
        "status": status,
        "dcc_type": dcc_type,
        "tool_family": tool_family,
        "total_ms": total_ms,
        "error": error,
        "token_accounting": token_accounting,
        "payload_tokens": payload_tokens,
        "redaction_status": {
            "mode": "public-safe",
            "raw_payloads_excluded": true,
            "payload_previews_excluded": true,
            "prompts_excluded": true,
            "scripts_excluded": true,
            "auth_material_excluded": true,
            "local_urls_excluded": true,
            "absolute_paths_excluded": true,
            "private_identifiers_excluded": true,
            "redaction_markers_detected": redaction_marker_detected(bundle),
        },
        "postmortem": {
            "previous_call_count": previous_call_count,
            "gateway_event_count": gateway_event_count,
        },
    });
    (summary, title)
}

fn public_safe_body_template(request_id: &str, summary: &Value) -> String {
    let status = summary
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let dcc_type = summary
        .get("dcc_type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let tool_family = summary
        .get("tool_family")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let error_kind = summary
        .get("error")
        .and_then(|error| error.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("none");
    let total_ms = match summary.get("total_ms") {
        Some(Value::Number(value)) => format!("{value} ms"),
        _ => "unknown".to_string(),
    };

    format!(
        "## Summary\n\nRequest `{request_id}` returned `{status}` for `{tool_family}` on `{dcc_type}`.\n\n## Public-safe diagnostics\n\n- Status: `{status}`\n- DCC type: `{dcc_type}`\n- Tool family: `{tool_family}`\n- Duration: `{total_ms}`\n- Sanitized error kind: `{error_kind}`\n\n## Evidence policy\n\nThis issue report intentionally excludes request/response payload previews, prompts, scripts, auth material, local URLs, absolute filesystem paths, and private scene or project identifiers. Use the explicit raw export only after reviewing it for public sharing."
    )
}

fn public_safe_issue_report(request_id: &str, bundle: Value) -> Value {
    let generated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let trace_id = bundle.get("trace_id").cloned().unwrap_or_else(|| {
        bundle["trace"]
            .get("trace_id")
            .cloned()
            .unwrap_or(Value::Null)
    });
    let (summary, title) = build_summary(request_id, &bundle);
    let body_template = public_safe_body_template(request_id, &summary);
    let links = public_safe_links(request_id);
    let raw_admin_path = links["raw_issue_report_path"].clone();
    let raw_stable_path = links["stable_raw_issue_report_path"].clone();

    json!({
        "schema_version": "dcc-mcp.admin.issue-report.v1",
        "report_type": "github_issue_public_safe",
        "privacy_mode": "public-safe",
        "generated_at": generated_at,
        "request_id": request_id,
        "trace_id": trace_id,
        "summary": summary,
        "github_issue": {
            "title": title,
            "body_template": body_template,
            "suggested_labels": ["bug", "admin-telemetry"],
        },
        "links": links,
        "raw_debug_bundle": {
            "available": true,
            "mode_query": "mode=raw",
            "admin_path": raw_admin_path,
            "stable_path": raw_stable_path,
            "privacy_note": "Raw exports may include payload previews, prompts, script snippets, scene paths, local URLs, auth material, or proprietary data. Review before sharing publicly.",
        },
        "privacy_note": "Public-safe mode excludes raw payload previews and local/private evidence by default. Use ?mode=raw only for reviewed local diagnostics.",
    })
}

fn raw_issue_report(request_id: &str, bundle: Value, links: Value) -> Value {
    let generated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let trace_id = bundle.get("trace_id").cloned().unwrap_or_else(|| {
        bundle["trace"]
            .get("trace_id")
            .cloned()
            .unwrap_or(Value::Null)
    });
    let (mut summary, title) = build_summary(request_id, &bundle);
    if let Some(map) = summary.as_object_mut() {
        map.insert(
            "redaction_status".to_string(),
            json!({
                "mode": "raw-local-evidence",
                "raw_payloads_excluded": false,
                "payload_previews_excluded": false,
                "local_urls_excluded": false,
                "absolute_paths_excluded": false,
                "private_identifiers_excluded": false,
                "redaction_markers_detected": redaction_marker_detected(&bundle),
            }),
        );
    }
    let tool = bundle["trace"]
        .get("tool_slug")
        .or_else(|| bundle["trace"].get("method"))
        .or_else(|| bundle["audit"].get("tool"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let dcc_type = bundle["trace"]
        .get("dcc_type")
        .or_else(|| bundle["audit"].get("dcc_type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let status = summary
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let body_template = format!(
        "## Summary\n\nRequest `{request_id}` returned `{status}` for `{tool}` on `{dcc_type}`.\n\n## Attached data\n\nThis explicit raw export includes the correlated debug bundle for private/local diagnostics.\n\n## Notes\n\nReview the JSON for secrets, local URLs, payload previews, prompts, scripts, auth material, proprietary scene paths, or private project identifiers before uploading."
    );

    json!({
        "schema_version": "dcc-mcp.admin.issue-report.v1",
        "report_type": "github_issue_debug_json",
        "privacy_mode": "raw-local-evidence",
        "generated_at": generated_at,
        "request_id": request_id,
        "trace_id": trace_id,
        "summary": summary,
        "github_issue": {
            "title": title,
            "body_template": body_template,
            "suggested_labels": ["bug", "admin-telemetry"],
        },
        "links": links,
        "privacy_note": "Raw export: review request and response payloads before uploading; this may include scene paths, prompts, tokens, local URLs, auth material, or proprietary data.",
        "debug_bundle": bundle,
    })
}

pub(super) fn issue_report_json(
    request_id: &str,
    bundle: Value,
    links: Value,
    mode: IssueReportMode,
) -> Value {
    match mode {
        IssueReportMode::PublicSafe => public_safe_issue_report(request_id, bundle),
        IssueReportMode::RawDebugBundle => raw_issue_report(request_id, bundle, links),
    }
}
