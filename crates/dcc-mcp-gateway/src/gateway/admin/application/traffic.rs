//! Gateway adapters for metadata-only admin traffic projections.

use dcc_mcp_gateway_admin::{TrafficProjectionSnapshot, traffic_jsonl_export, traffic_payload};
use serde_json::Value;

use super::governance::governance_capture_decision;
use crate::gateway::traffic::{TrafficCapture, TrafficCaptureSnapshot};

pub(in crate::gateway::admin) fn build_traffic_payload(
    capture: &TrafficCapture,
    limit: usize,
    links: Value,
) -> Value {
    let frames = capture
        .recent_frames(limit)
        .into_iter()
        .map(|frame| frame.to_value())
        .collect();
    let snapshot = traffic_projection_snapshot(&capture.governance_snapshot());
    traffic_payload(frames, &snapshot, links)
}

pub(in crate::gateway::admin) fn build_traffic_export_body(
    capture: &TrafficCapture,
    limit: usize,
) -> String {
    let frames = capture
        .recent_frames(limit)
        .into_iter()
        .map(|frame| frame.to_value())
        .collect();
    traffic_jsonl_export(frames)
}

fn traffic_projection_snapshot(snapshot: &TrafficCaptureSnapshot) -> TrafficProjectionSnapshot {
    let admin_live = snapshot
        .sinks
        .iter()
        .find(|sink| sink.kind.eq_ignore_ascii_case("admin_live"));
    TrafficProjectionSnapshot {
        enabled: snapshot.enabled,
        sink_count: snapshot.sink_count,
        subscriber_enabled: snapshot.subscriber_enabled,
        live_sink_enabled: admin_live.is_some(),
        admin_live_capacity: admin_live.and_then(|sink| sink.ring_buffer_capacity),
        recent_decisions: snapshot
            .recent_decisions
            .iter()
            .map(governance_capture_decision)
            .collect(),
    }
}
