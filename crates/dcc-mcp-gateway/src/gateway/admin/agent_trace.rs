//! Public-safe agent trace packet projection for stable debug routes.

use axum::Json;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde_json::{Value, json};

use super::links::AdminLinkBuilder;
use super::state::AdminState;
use crate::gateway::response_codec::TOKEN_ESTIMATOR;

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

/// `GET /v1/debug/agent-traces/{lookup_id}` — compact agent packet by trace id or request id.
pub async fn handle_v1_debug_agent_trace_packet(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Path(lookup_id): Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    match crate::gateway::admin::activity::build_debug_bundle(&s, &lookup_id).await {
        Some(bundle) => {
            let trace = bundle.get("trace").cloned().unwrap_or(Value::Null);
            let request_id = bundle
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or(&lookup_id);
            let ok = trace.get("ok").and_then(Value::as_bool);
            let spans = trace
                .get("spans")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            let postmortem = bundle.get("postmortem").cloned().unwrap_or(Value::Null);
            let packet = json!({
                "schema_version": "dcc-mcp.admin.agent-trace-packet.v1",
                "lookup_id": lookup_id,
                "trace_id": bundle.get("trace_id").cloned().unwrap_or(Value::Null),
                "request_id": request_id,
                "request_ids": bundle.get("request_ids").cloned().unwrap_or_else(|| json!([])),
                "status": ok.map(|ok| if ok { "ok" } else { "err" }).unwrap_or("unknown"),
                "tool": trace.get("tool_slug")
                    .or_else(|| trace.get("method"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "dcc_type": trace.get("dcc_type").cloned().unwrap_or(Value::Null),
                "transport": trace.get("transport").cloned().unwrap_or(Value::Null),
                "total_ms": trace.get("total_ms").cloned().unwrap_or(Value::Null),
                "span_count": spans,
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
                "links": links.request_links(request_id),
                "privacy_note": "Agent trace packets omit request/response payload previews, prompts, scripts, and scene data. Use debug_bundle_url only for reviewed local diagnostics.",
            });
            (StatusCode::OK, Json(packet)).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent trace packet not found", "lookup_id": lookup_id })),
        )
            .into_response(),
    }
}
