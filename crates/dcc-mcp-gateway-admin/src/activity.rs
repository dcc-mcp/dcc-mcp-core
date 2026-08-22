//! Pure activity timeline projections for the admin dashboard.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{AdminAuditRecord, DispatchTrace, TokenTelemetry};

/// Correlation fields shared by audit, trace, and gateway activity rows.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ActivityCorrelation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
}

/// One backend-neutral row in the unified activity timeline.
#[derive(Debug, Clone, Serialize)]
pub struct ActivityEvent {
    pub event_id: String,
    pub timestamp: String,
    pub kind: String,
    pub severity: String,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_accounting: Option<TokenTelemetry>,
    pub correlation: ActivityCorrelation,
}

/// Gateway-owned event data accepted by the admin timeline projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayActivityInput {
    pub timestamp: String,
    pub status: String,
    pub reason: Option<String>,
    pub dcc_type: String,
    pub instance_id: String,
}

/// Build the unified activity payload from already-collected domain rows.
#[must_use]
pub fn activity_payload(
    audits: &[AdminAuditRecord],
    traces: &[DispatchTrace],
    gateway_events: &[GatewayActivityInput],
    limit: usize,
) -> Value {
    let mut events = audits
        .iter()
        .map(audit_activity_event)
        .chain(traces.iter().map(trace_activity_event))
        .chain(gateway_events.iter().map(gateway_activity_event))
        .collect::<Vec<_>>();
    events.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    events.truncate(limit);
    json!({ "total": events.len(), "events": events })
}

/// Project an audit record into one activity row.
#[must_use]
pub fn audit_activity_event(record: &AdminAuditRecord) -> ActivityEvent {
    ActivityEvent {
        event_id: format!("audit:{}", record.request_id),
        timestamp: rfc3339(record.timestamp),
        kind: "tool_call".to_string(),
        severity: if record.success { "info" } else { "error" }.to_string(),
        status: if record.success { "ok" } else { "err" }.to_string(),
        message: format!(
            "{} {}",
            record.method.as_deref().unwrap_or("call"),
            record.action
        ),
        tool: Some(record.action.clone()),
        duration_ms: record.duration_ms,
        token_accounting: record.token_accounting.clone(),
        correlation: ActivityCorrelation {
            trace_id: record.trace_id.clone(),
            span_id: record.span_id.clone(),
            parent_span_id: record.parent_span_id.clone(),
            request_id: Some(record.request_id.clone()),
            session_id: record.session_id.clone(),
            instance_id: record.instance_id.clone(),
            dcc_type: record.dcc_type.clone(),
            workflow_id: None,
            job_id: None,
            agent_id: record.agent_id.clone(),
            actor_id: record.actor_id.clone(),
            actor_name: record.actor_name.clone(),
            client_platform: record.client_platform.clone(),
            source_ip: record.source_ip.clone(),
            parent_request_id: record.parent_request_id.clone(),
        },
    }
}

/// Project a dispatch trace into one activity row.
#[must_use]
pub fn trace_activity_event(trace: &DispatchTrace) -> ActivityEvent {
    let tool = trace
        .tool_slug
        .clone()
        .unwrap_or_else(|| trace.method.clone());
    ActivityEvent {
        event_id: format!("trace:{}", trace.request_id),
        timestamp: rfc3339(trace.started_at),
        kind: "dispatch_trace".to_string(),
        severity: if trace.ok { "debug" } else { "error" }.to_string(),
        status: if trace.ok { "ok" } else { "err" }.to_string(),
        message: format!("{} completed in {}ms", tool, trace.total_ms),
        tool: Some(tool),
        duration_ms: Some(trace.total_ms),
        token_accounting: trace.token_accounting.clone(),
        correlation: trace_correlation(trace),
    }
}

/// Project a gateway event DTO into one activity row.
#[must_use]
pub fn gateway_activity_event(event: &GatewayActivityInput) -> ActivityEvent {
    ActivityEvent {
        event_id: format!(
            "gateway:{}:{}:{}",
            event.timestamp, event.dcc_type, event.instance_id
        ),
        timestamp: event.timestamp.clone(),
        kind: "gateway_event".to_string(),
        severity: "info".to_string(),
        status: event.status.clone(),
        message: event.reason.clone().unwrap_or_else(|| {
            format!(
                "{} dcc_type={} instance={}",
                event.status, event.dcc_type, event.instance_id
            )
        }),
        tool: None,
        duration_ms: None,
        token_accounting: None,
        correlation: ActivityCorrelation {
            instance_id: Some(event.instance_id.clone()),
            dcc_type: Some(event.dcc_type.clone()),
            ..ActivityCorrelation::default()
        },
    }
}

/// Serialize a gateway event row for debug-bundle composition.
#[must_use]
pub fn gateway_activity_event_json(event: &GatewayActivityInput) -> Value {
    serde_json::to_value(gateway_activity_event(event)).unwrap_or_else(|_| json!({}))
}

fn trace_correlation(trace: &DispatchTrace) -> ActivityCorrelation {
    ActivityCorrelation {
        trace_id: Some(trace.trace_id.clone()),
        span_id: trace.span_id.clone(),
        parent_span_id: trace.parent_span_id.clone(),
        request_id: Some(trace.request_id.clone()),
        session_id: trace.session_id.clone(),
        instance_id: trace.instance_id.clone(),
        dcc_type: trace.dcc_type.clone(),
        workflow_id: None,
        job_id: None,
        agent_id: trace
            .agent_context
            .as_ref()
            .and_then(|context| context.agent_id.clone()),
        actor_id: trace
            .agent_context
            .as_ref()
            .and_then(|context| context.actor_id.clone()),
        actor_name: trace
            .agent_context
            .as_ref()
            .and_then(|context| context.actor_name.clone()),
        client_platform: trace
            .agent_context
            .as_ref()
            .and_then(|context| context.client_platform.clone()),
        source_ip: trace
            .agent_context
            .as_ref()
            .and_then(|context| context.source_ip.clone()),
        parent_request_id: trace
            .agent_context
            .as_ref()
            .and_then(|context| context.parent_request_id.clone()),
    }
}

fn rfc3339(timestamp: SystemTime) -> String {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|_| {
            chrono::DateTime::<chrono::Utc>::from(timestamp)
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gateway_event(timestamp: &str, instance_id: &str) -> GatewayActivityInput {
        GatewayActivityInput {
            timestamp: timestamp.to_string(),
            status: "host_died".to_string(),
            reason: None,
            dcc_type: "maya".to_string(),
            instance_id: instance_id.to_string(),
        }
    }

    #[test]
    fn gateway_projection_preserves_operator_context_without_gateway_types() {
        let event = gateway_activity_event(&gateway_event("2026-08-23T01:02:03Z", "abc123"));
        assert_eq!(event.kind, "gateway_event");
        assert_eq!(event.status, "host_died");
        assert_eq!(event.correlation.dcc_type.as_deref(), Some("maya"));
        assert!(event.message.contains("instance=abc123"));
    }

    #[test]
    fn activity_payload_orders_newest_first_and_applies_limit() {
        let events = vec![
            gateway_event("2026-08-23T01:00:00Z", "older"),
            gateway_event("2026-08-23T02:00:00Z", "newer"),
        ];
        let payload = activity_payload(&[], &[], &events, 1);
        assert_eq!(payload["total"], 1);
        assert_eq!(payload["events"][0]["correlation"]["instance_id"], "newer");
    }
}
