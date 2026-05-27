//! Admin traffic projection.
//!
//! The capture layer may retain high-sensitivity payloads for explicit private
//! replay sinks. The admin API presents a metadata-only projection so operators
//! can understand capture state without leaking tool arguments.

use std::collections::BTreeSet;

use dcc_mcp_actions::events::EventEnvelope;
use serde_json::{Value, json};

use crate::gateway::traffic::{TrafficCapture, TrafficCaptureDecision, TrafficCaptureSnapshot};

const SCHEMA_VERSION: &str = "dcc-mcp.admin.traffic.v1";

pub(super) fn build_traffic_payload(capture: &TrafficCapture, limit: usize, links: Value) -> Value {
    let frames: Vec<Value> = capture
        .recent_frames(limit)
        .into_iter()
        .map(sanitized_frame_value)
        .collect();
    let snapshot = capture.governance_snapshot();
    let capture_status = capture_status(&snapshot, frames.len());

    json!({
        "schema_version": SCHEMA_VERSION,
        "total": frames.len(),
        "frames": frames,
        "capture_status": capture_status,
        "links": links,
    })
}

pub(super) fn build_traffic_export_body(capture: &TrafficCapture, limit: usize) -> String {
    let mut body = String::new();
    for frame in capture.recent_frames(limit) {
        if let Ok(line) = serde_json::to_string(&sanitized_frame_value(frame)) {
            body.push_str(&line);
            body.push('\n');
        }
    }
    body
}

fn sanitized_frame_value(frame: EventEnvelope) -> Value {
    let mut value = frame.to_value();
    if let Some(attributes) = value
        .as_object_mut()
        .and_then(|map| map.get_mut("attributes"))
    {
        sanitize_attributes(attributes);
    }
    value
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

fn capture_status(snapshot: &TrafficCaptureSnapshot, retained_frames: usize) -> Value {
    let live_sink_enabled = snapshot
        .sinks
        .iter()
        .any(|sink| sink.kind.eq_ignore_ascii_case("admin_live"));
    let captured_decision_count = decision_count(&snapshot.recent_decisions, "captured");
    let skipped_decision_count = decision_count(&snapshot.recent_decisions, "skipped");
    let skip_reasons = skip_reasons(&snapshot.recent_decisions);
    let redacted_paths = redacted_paths(&snapshot.recent_decisions);
    let state = capture_state(
        snapshot.enabled,
        live_sink_enabled,
        retained_frames,
        skipped_decision_count,
    );

    json!({
        "state": state,
        "message": capture_message(state),
        "capture_enabled": snapshot.enabled,
        "live_sink_enabled": live_sink_enabled,
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
            "admin_live_configured": live_sink_enabled,
            "ring_buffer_capacity": admin_live_capacity(snapshot),
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

fn decision_count(decisions: &[TrafficCaptureDecision], outcome: &str) -> usize {
    decisions
        .iter()
        .filter(|decision| decision.outcome == outcome)
        .count()
}

fn skip_reasons(decisions: &[TrafficCaptureDecision]) -> Vec<String> {
    decisions
        .iter()
        .filter_map(|decision| decision.reason.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn redacted_paths(decisions: &[TrafficCaptureDecision]) -> Vec<String> {
    decisions
        .iter()
        .flat_map(|decision| decision.redacted_paths.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn admin_live_capacity(snapshot: &TrafficCaptureSnapshot) -> Option<usize> {
    snapshot
        .sinks
        .iter()
        .find(|sink| sink.kind.eq_ignore_ascii_case("admin_live"))
        .and_then(|sink| sink.ring_buffer_capacity)
}
