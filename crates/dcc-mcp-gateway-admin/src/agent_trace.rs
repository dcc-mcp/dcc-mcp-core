//! Public-safe agent trace packet projections.

use serde_json::{Value, json};

use crate::TOKEN_ESTIMATOR;

/// Build a compact agent trace packet from a correlated debug bundle.
///
/// The caller owns request routing and link construction. This projection
/// intentionally omits request and response payloads, prompts, scripts, and
/// scene data from the returned packet.
#[must_use]
pub fn agent_trace_packet(lookup_id: &str, bundle: &Value, links: Value) -> Value {
    let trace = bundle.get("trace").cloned().unwrap_or(Value::Null);
    let request_id = bundle
        .get("request_id")
        .and_then(Value::as_str)
        .unwrap_or(lookup_id);
    let ok = trace.get("ok").and_then(Value::as_bool);
    let span_count = trace
        .get("spans")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let postmortem = bundle.get("postmortem").unwrap_or(&Value::Null);

    json!({
        "schema_version": "dcc-mcp.admin.agent-trace-packet.v1",
        "lookup_id": lookup_id,
        "trace_id": bundle.get("trace_id").cloned().unwrap_or(Value::Null),
        "request_id": request_id,
        "request_ids": bundle.get("request_ids").cloned().unwrap_or_else(|| json!([])),
        "status": ok.map(|ok| if ok { "ok" } else { "err" }).unwrap_or("unknown"),
        "tool": trace
            .get("tool_slug")
            .or_else(|| trace.get("method"))
            .cloned()
            .unwrap_or(Value::Null),
        "dcc_type": trace.get("dcc_type").cloned().unwrap_or(Value::Null),
        "transport": trace.get("transport").cloned().unwrap_or(Value::Null),
        "total_ms": trace.get("total_ms").cloned().unwrap_or(Value::Null),
        "span_count": span_count,
        "payload_tokens": payload_token_packet(&trace),
        "response_token_accounting": trace
            .get("token_accounting")
            .cloned()
            .unwrap_or(Value::Null),
        "postmortem": {
            "previous_call_count": postmortem
                .get("previous_calls")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0),
            "gateway_event_count": postmortem
                .get("gateway_events")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0),
        },
        "links": links,
        "privacy_note": "Agent trace packets omit request/response payload previews, prompts, scripts, and scene data. Use debug_bundle_url only for reviewed local diagnostics.",
    })
}

fn payload_token_packet(trace: &Value) -> Value {
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
        "token_estimator": TOKEN_ESTIMATOR,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_tokens,
        "missing_payload_tokens": input_tokens.is_none() && output_tokens.is_none(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_projects_counts_and_omits_payloads() {
        let bundle = json!({
            "trace_id": "trace-1",
            "request_id": "req-1",
            "request_ids": ["req-1"],
            "trace": {
                "ok": false,
                "tool_slug": "maya__scene_save",
                "dcc_type": "maya",
                "transport": "http",
                "total_ms": 42,
                "spans": [{"name": "dispatch"}, {"name": "backend"}],
                "input": {"estimated_tokens": 3, "preview": "private input"},
                "output": {"estimated_tokens": 5, "preview": "private output"},
                "token_accounting": {"saved_tokens": 2},
            },
            "postmortem": {
                "previous_calls": [{}, {}],
                "gateway_events": [{}],
            },
        });
        let links = json!({"trace_url": "/v1/debug/traces/req-1"});

        let packet = agent_trace_packet("req-1", &bundle, links.clone());

        assert_eq!(packet["status"], "err");
        assert_eq!(packet["span_count"], 2);
        assert_eq!(packet["payload_tokens"]["total_tokens"], 8);
        assert_eq!(packet["postmortem"]["previous_call_count"], 2);
        assert_eq!(packet["postmortem"]["gateway_event_count"], 1);
        assert_eq!(packet["links"], links);
        assert!(packet.get("input").is_none());
        assert!(packet.get("output").is_none());
        assert!(!packet.to_string().contains("private"));
    }

    #[test]
    fn packet_defaults_missing_trace_fields() {
        let packet = agent_trace_packet("missing", &json!({}), json!({}));

        assert_eq!(packet["request_id"], "missing");
        assert_eq!(packet["status"], "unknown");
        assert_eq!(packet["span_count"], 0);
        assert_eq!(packet["payload_tokens"]["missing_payload_tokens"], true);
    }
}
