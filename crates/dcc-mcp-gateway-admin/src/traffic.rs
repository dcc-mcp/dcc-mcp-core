//! Metadata-only traffic projections for the admin dashboard.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::GovernanceCaptureDecision;

const SCHEMA_VERSION: &str = "dcc-mcp.admin.traffic.v1";

/// Backend-neutral traffic capture state consumed by admin projections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficProjectionSnapshot {
    pub enabled: bool,
    pub sink_count: usize,
    pub subscriber_enabled: bool,
    pub live_sink_enabled: bool,
    pub admin_live_capacity: Option<usize>,
    pub recent_decisions: Vec<GovernanceCaptureDecision>,
}

/// Build the metadata-only traffic payload returned by the admin API.
#[must_use]
pub fn traffic_payload(
    frames: Vec<Value>,
    snapshot: &TrafficProjectionSnapshot,
    links: Value,
) -> Value {
    let frames: Vec<Value> = frames.into_iter().map(sanitize_frame).collect();
    let capture_status = capture_status(snapshot, frames.len());

    json!({
        "schema_version": SCHEMA_VERSION,
        "total": frames.len(),
        "frames": frames,
        "capture_status": capture_status,
        "links": links,
    })
}

/// Encode metadata-only traffic frames as newline-delimited JSON.
#[must_use]
pub fn traffic_jsonl_export(frames: Vec<Value>) -> String {
    let mut body = String::new();
    for frame in frames.into_iter().map(sanitize_frame) {
        if let Ok(line) = serde_json::to_string(&frame) {
            body.push_str(&line);
            body.push('\n');
        }
    }
    body
}

fn sanitize_frame(mut frame: Value) -> Value {
    if let Some(attributes) = frame
        .as_object_mut()
        .and_then(|map| map.get_mut("attributes"))
    {
        sanitize_attributes(attributes);
    }
    frame
}

fn sanitize_attributes(attributes: &mut Value) {
    let Some(map) = attributes.as_object_mut() else {
        return;
    };

    if let Some(body) = map.get_mut("body").and_then(Value::as_object_mut) {
        body.remove("data");
        body.insert("payload_omitted".to_string(), Value::Bool(true));
        body.insert(
            "omission_reason".to_string(),
            Value::String("admin-traffic-metadata-only".to_string()),
        );
    }
}

fn capture_status(snapshot: &TrafficProjectionSnapshot, retained_frames: usize) -> Value {
    let captured_decision_count = decision_count(&snapshot.recent_decisions, "captured");
    let skipped_decision_count = decision_count(&snapshot.recent_decisions, "skipped");
    let skip_reasons = skip_reasons(&snapshot.recent_decisions);
    let redacted_paths = redacted_paths(&snapshot.recent_decisions);
    let state = capture_state(
        snapshot.enabled,
        snapshot.live_sink_enabled,
        retained_frames,
        skipped_decision_count,
    );

    json!({
        "state": state,
        "message": capture_message(state),
        "capture_enabled": snapshot.enabled,
        "live_sink_enabled": snapshot.live_sink_enabled,
        "sink_count": snapshot.sink_count,
        "subscriber_enabled": snapshot.subscriber_enabled,
        "retained_frames": retained_frames,
        "recent_decision_count": snapshot.recent_decisions.len(),
        "captured_decision_count": captured_decision_count,
        "skipped_decision_count": skipped_decision_count,
        "skip_reasons": skip_reasons,
        "redacted_path_count": redacted_paths.len(),
        "redacted_paths": redacted_paths,
        "safe_to_share": true,
        "payload_policy": "metadata-only",
        "retention": {
            "admin_live_configured": snapshot.live_sink_enabled,
            "ring_buffer_capacity": snapshot.admin_live_capacity,
        },
    })
}

fn capture_state(
    capture_enabled: bool,
    live_sink_enabled: bool,
    retained_frames: usize,
    skipped_decision_count: usize,
) -> &'static str {
    if retained_frames > 0 {
        "captured"
    } else if !capture_enabled {
        "capture_disabled"
    } else if !live_sink_enabled {
        "capture_unavailable"
    } else if skipped_decision_count > 0 {
        "capture_filtered"
    } else {
        "no_traffic"
    }
}

fn capture_message(state: &str) -> &'static str {
    match state {
        "captured" => "Sanitized traffic metadata is retained in the admin live ring.",
        "capture_disabled" => {
            "Traffic capture is disabled; the panel is showing zero retained frames by configuration."
        }
        "capture_unavailable" => {
            "Traffic capture is enabled, but no admin_live sink is configured for this panel."
        }
        "capture_filtered" => {
            "Recent gateway traffic was skipped by capture filters or redaction policy before live retention."
        }
        _ => {
            "Admin live capture is ready, but no matching traffic has been observed in the retained range."
        }
    }
}

fn decision_count(decisions: &[GovernanceCaptureDecision], outcome: &str) -> usize {
    decisions
        .iter()
        .filter(|decision| decision.outcome == outcome)
        .count()
}

fn skip_reasons(decisions: &[GovernanceCaptureDecision]) -> Vec<String> {
    decisions
        .iter()
        .filter_map(|decision| decision.reason.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn redacted_paths(decisions: &[GovernanceCaptureDecision]) -> Vec<String> {
    decisions
        .iter()
        .flat_map(|decision| decision.redacted_paths.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;

    fn decision(outcome: &str) -> GovernanceCaptureDecision {
        GovernanceCaptureDecision {
            timestamp: UNIX_EPOCH,
            request_id: Some("request-1".to_string()),
            trace_id: None,
            session_id: None,
            transport: "mcp".to_string(),
            mcp_method: Some("tools/call".to_string()),
            outcome: outcome.to_string(),
            reason: (outcome == "skipped").then(|| "method-filter".to_string()),
            redacted_paths: vec!["$.arguments.token".to_string()],
        }
    }

    fn snapshot() -> TrafficProjectionSnapshot {
        TrafficProjectionSnapshot {
            enabled: true,
            sink_count: 1,
            subscriber_enabled: false,
            live_sink_enabled: true,
            admin_live_capacity: Some(32),
            recent_decisions: vec![decision("captured")],
        }
    }

    #[test]
    fn payload_removes_body_data_and_reports_capture_state() {
        let payload = traffic_payload(
            vec![json!({
                "attributes": {"body": {"data": "secret", "size": 6}},
                "event": "traffic"
            })],
            &snapshot(),
            json!({"self": "/admin/api/traffic"}),
        );

        assert_eq!(payload["capture_status"]["state"], "captured");
        assert_eq!(payload["capture_status"]["safe_to_share"], true);
        assert_eq!(
            payload["frames"][0]["attributes"]["body"]["data"],
            Value::Null
        );
        assert_eq!(
            payload["frames"][0]["attributes"]["body"]["payload_omitted"],
            true
        );
    }

    #[test]
    fn filtered_capture_and_jsonl_export_share_sanitization() {
        let mut snapshot = snapshot();
        snapshot.recent_decisions = vec![decision("skipped")];
        let payload = traffic_payload(vec![], &snapshot, Value::Null);
        assert_eq!(payload["capture_status"]["state"], "capture_filtered");

        let export = traffic_jsonl_export(vec![json!({
            "attributes": {"body": {"data": {"token": "secret"}}}
        })]);
        assert!(!export.contains("secret"));
        assert!(export.contains("admin-traffic-metadata-only"));
        assert_eq!(export.lines().count(), 1);
    }
}
