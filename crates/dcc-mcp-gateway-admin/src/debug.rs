//! Pure debug-bundle and postmortem projections for the admin dashboard.

use std::cmp::Reverse;
use std::collections::HashSet;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AdminAuditRecord, DispatchTrace, GatewayActivityInput, StatsFilter, StatsStatus,
    audit_activity_event, gateway_activity_event_json, trace_activity_event,
};

const POSTMORTEM_PREVIOUS_CALL_LIMIT: usize = 5;
const POSTMORTEM_EVENT_LIMIT: usize = 10;

/// Shared query contract for stable admin/debug list endpoints.
#[derive(Debug, Default, Deserialize)]
pub struct DebugListQuery {
    limit: Option<String>,
    range: Option<String>,
    dcc_type: Option<String>,
    skill: Option<String>,
    tool: Option<String>,
    status: Option<String>,
    instance_id: Option<String>,
    session_id: Option<String>,
    response_format: Option<String>,
    compact: Option<bool>,
}

impl DebugListQuery {
    /// Return a bounded list limit, using `default` for missing or invalid input.
    #[must_use]
    pub fn limit(&self, default: usize, max: usize) -> usize {
        self.limit
            .as_deref()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(default)
            .clamp(1, max)
    }

    /// Return the requested time range or the stable `all` default.
    #[must_use]
    pub fn range(&self) -> &str {
        self.range.as_deref().unwrap_or("all")
    }

    /// Parse optional statistics filters, rejecting unknown status values.
    pub fn stats_filter(&self) -> Result<StatsFilter, String> {
        Ok(StatsFilter {
            dcc_type: non_empty(self.dcc_type.as_deref()),
            skill: non_empty(self.skill.as_deref()),
            tool: non_empty(self.tool.as_deref()),
            status: non_empty(self.status.as_deref())
                .as_deref()
                .map(StatsStatus::from_query)
                .transpose()?,
            instance_id: non_empty(self.instance_id.as_deref()),
            session_id: non_empty(self.session_id.as_deref()),
        })
    }

    /// Build the request body consumed by backend-neutral response negotiation.
    #[must_use]
    pub fn response_format_body(&self) -> Value {
        let mut body = serde_json::Map::new();
        if let Some(format) = self.response_format.as_deref() {
            body.insert("response_format".to_string(), json!(format));
        }
        if let Some(compact) = self.compact {
            body.insert("compact".to_string(), json!(compact));
        }
        Value::Object(body)
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Build one correlated debug bundle from already-collected admin records.
///
/// The caller owns persistence and event-log access; this function owns only
/// the backend-neutral correlation and JSON projection contract.
#[must_use]
pub fn debug_bundle_payload(
    lookup_id: &str,
    audits: Vec<AdminAuditRecord>,
    all_traces: Vec<DispatchTrace>,
    gateway_events: Vec<GatewayActivityInput>,
) -> Option<Value> {
    let mut matching_traces: Vec<DispatchTrace> = all_traces
        .iter()
        .filter(|trace| trace.request_id == lookup_id || trace.trace_id == lookup_id)
        .cloned()
        .collect();
    matching_traces.sort_by_key(|trace| Reverse(trace.started_at));

    let trace_id = matching_traces.first().map(|trace| trace.trace_id.clone());
    if let Some(trace_id) = trace_id.as_deref() {
        let extra_traces = all_traces
            .iter()
            .filter(|trace| trace.trace_id == trace_id)
            .filter(|trace| {
                !matching_traces
                    .iter()
                    .any(|existing| existing.request_id == trace.request_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        matching_traces.extend(extra_traces);
    }

    let matching_request_ids: HashSet<String> = matching_traces
        .iter()
        .map(|trace| trace.request_id.clone())
        .collect();
    let matching_audits: Vec<AdminAuditRecord> = audits
        .into_iter()
        .filter(|record| {
            record.request_id == lookup_id
                || matching_request_ids.contains(&record.request_id)
                || trace_id
                    .as_deref()
                    .is_some_and(|id| record.trace_id.as_deref() == Some(id))
        })
        .collect();
    if matching_audits.is_empty() && matching_traces.is_empty() {
        return None;
    }

    let primary_trace = matching_traces
        .iter()
        .find(|trace| trace.request_id == lookup_id)
        .or_else(|| matching_traces.first())
        .cloned();
    let primary_audit = matching_audits
        .iter()
        .find(|record| record.request_id == lookup_id)
        .or_else(|| matching_audits.first())
        .cloned();
    let primary_request_id = primary_trace
        .as_ref()
        .map(|trace| trace.request_id.clone())
        .or_else(|| {
            primary_audit
                .as_ref()
                .map(|record| record.request_id.clone())
        })
        .unwrap_or_else(|| lookup_id.to_string());

    let mut request_ids: Vec<String> = matching_request_ids.into_iter().collect();
    if !request_ids.iter().any(|id| id == &primary_request_id) {
        request_ids.push(primary_request_id.clone());
    }
    request_ids.sort();

    let related_events =
        related_gateway_events(gateway_events, &request_ids, primary_trace.as_ref());
    let related_activity: Vec<Value> = related_events
        .iter()
        .map(gateway_activity_event_json)
        .chain(
            matching_audits
                .iter()
                .map(audit_activity_event)
                .filter_map(|event| serde_json::to_value(event).ok()),
        )
        .chain(
            matching_traces
                .iter()
                .map(trace_activity_event)
                .filter_map(|event| serde_json::to_value(event).ok()),
        )
        .collect();
    let postmortem = postmortem_payload(&all_traces, primary_trace.as_ref(), &related_events);
    let hints = debug_hints(primary_trace.as_ref());
    let root_cause = primary_audit
        .as_ref()
        .and_then(|record| record.error.clone())
        .or_else(|| {
            primary_trace
                .as_ref()
                .filter(|trace| !trace.ok)
                .and_then(|_| hints.first().cloned())
        });

    Some(json!({
        "lookup_id": lookup_id,
        "request_id": primary_request_id,
        "trace_id": trace_id,
        "request_ids": request_ids,
        "root_cause": root_cause,
        "audit": primary_audit.as_ref().map(audit_activity_event),
        "audits": matching_audits.iter().map(audit_activity_event).collect::<Vec<_>>(),
        "trace": primary_trace,
        "traces": matching_traces,
        "related_activity": related_activity,
        "postmortem": postmortem,
        "hints": hints,
    }))
}

fn related_gateway_events(
    gateway_events: Vec<GatewayActivityInput>,
    request_ids: &[String],
    trace: Option<&DispatchTrace>,
) -> Vec<GatewayActivityInput> {
    gateway_events
        .into_iter()
        .filter(|event| gateway_event_matches(event, request_ids, trace))
        .take(POSTMORTEM_EVENT_LIMIT)
        .collect()
}

fn gateway_event_matches(
    event: &GatewayActivityInput,
    request_ids: &[String],
    trace: Option<&DispatchTrace>,
) -> bool {
    if let Some(reason) = event.reason.as_deref()
        && request_ids
            .iter()
            .any(|request_id| reason.contains(request_id))
    {
        return true;
    }
    let Some(trace) = trace else {
        return false;
    };
    if let Some(instance_id) = trace.instance_id.as_deref()
        && instance_hint_matches(&event.instance_id, instance_id)
    {
        return true;
    }
    trace
        .dcc_type
        .as_deref()
        .is_some_and(|dcc| event.dcc_type.eq_ignore_ascii_case(dcc))
        && event.status == "host_died"
}

fn postmortem_payload(
    all_traces: &[DispatchTrace],
    trace: Option<&DispatchTrace>,
    gateway_events: &[GatewayActivityInput],
) -> Value {
    let Some(trace) = trace else {
        return json!({
            "previous_calls": [],
            "gateway_events": gateway_events
                .iter()
                .map(gateway_activity_event_json)
                .collect::<Vec<_>>(),
        });
    };

    let previous_calls: Vec<Value> = all_traces
        .iter()
        .filter(|candidate| candidate.request_id != trace.request_id)
        .filter(|candidate| candidate.started_at <= trace.started_at)
        .filter(|candidate| trace_matches_postmortem_scope(candidate, trace))
        .take(POSTMORTEM_PREVIOUS_CALL_LIMIT)
        .cloned()
        .map(postmortem_trace_row)
        .collect();

    json!({
        "target": postmortem_trace_row(trace.clone()),
        "previous_calls": previous_calls,
        "gateway_events": gateway_events
            .iter()
            .map(gateway_activity_event_json)
            .collect::<Vec<_>>(),
    })
}

fn trace_matches_postmortem_scope(candidate: &DispatchTrace, target: &DispatchTrace) -> bool {
    if candidate.trace_id == target.trace_id {
        return true;
    }
    if let (Some(a), Some(b)) = (
        candidate.instance_id.as_deref(),
        target.instance_id.as_deref(),
    ) {
        return instance_hint_matches(a, b);
    }
    if let (Some(a), Some(b)) = (
        candidate.session_id.as_deref(),
        target.session_id.as_deref(),
    ) {
        return a == b;
    }
    if let (Some(a), Some(b)) = (candidate.dcc_type.as_deref(), target.dcc_type.as_deref()) {
        return a.eq_ignore_ascii_case(b);
    }
    false
}

fn postmortem_trace_row(trace: DispatchTrace) -> Value {
    json!({
        "request_id": trace.request_id,
        "trace_id": trace.trace_id,
        "span_id": trace.span_id,
        "parent_span_id": trace.parent_span_id,
        "parent_request_id": trace.parent_request_id,
        "started_at": rfc3339(trace.started_at),
        "tool": trace.tool_slug.unwrap_or(trace.method),
        "dcc_type": trace.dcc_type,
        "instance_id": trace.instance_id,
        "session_id": trace.session_id,
        "transport": trace.transport,
        "agent_context": trace.agent_context,
        "ok": trace.ok,
        "total_ms": trace.total_ms,
        "input": trace.input,
        "output": trace.output,
    })
}

fn instance_hint_matches(a: &str, b: &str) -> bool {
    let a = normalise_instance_hint(a);
    let b = normalise_instance_hint(b);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || (a.len() >= 4 && b.starts_with(&a)) || (b.len() >= 4 && a.starts_with(&b))
}

fn normalise_instance_hint(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_hexdigit)
        .flat_map(char::to_lowercase)
        .collect()
}

fn debug_hints(trace: Option<&DispatchTrace>) -> Vec<String> {
    let Some(trace) = trace else {
        return vec!["No dispatch trace was retained for this request.".to_string()];
    };
    if trace.ok {
        return vec![
            "Request completed successfully; inspect spans for slow segments.".to_string(),
        ];
    }
    let mut hints =
        vec!["Request failed; inspect the last error span and output payload.".to_string()];
    if trace
        .spans
        .iter()
        .any(|span| span.name.contains("backend") && !span.ok)
    {
        hints.push(
            "A backend span failed; check instance reachability and sidecar/DCC logs.".to_string(),
        );
    }
    hints
}

fn rfc3339(timestamp: std::time::SystemTime) -> String {
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

    #[test]
    fn debug_list_query_normalizes_bounds_filters_and_format() {
        let query: DebugListQuery = serde_json::from_value(json!({
            "limit": "999",
            "range": "24h",
            "dcc_type": "  maya  ",
            "skill": " ",
            "status": "failure",
            "response_format": "toon",
            "compact": true
        }))
        .unwrap();

        assert_eq!(query.limit(50, 100), 100);
        assert_eq!(query.range(), "24h");
        let filter = query.stats_filter().unwrap();
        assert_eq!(filter.dcc_type.as_deref(), Some("maya"));
        assert_eq!(filter.skill, None);
        assert_eq!(filter.status, Some(StatsStatus::Failure));
        assert_eq!(
            query.response_format_body(),
            json!({"response_format": "toon", "compact": true})
        );
    }

    #[test]
    fn debug_list_query_rejects_unknown_status() {
        let query: DebugListQuery = serde_json::from_value(json!({"status": "maybe"})).unwrap();
        assert!(query.stats_filter().is_err());
    }

    fn trace(request_id: &str, trace_id: &str, started_secs: u64) -> DispatchTrace {
        serde_json::from_value(json!({
            "request_id": request_id,
            "trace_id": trace_id,
            "method": "tools/call",
            "tool_slug": "maya__scene_save",
            "instance_id": "abcd-1234",
            "dcc_type": "maya",
            "started_at": started_secs * 1_000,
            "total_ms": 10,
            "ok": true,
            "spans": [],
        }))
        .expect("trace fixture should deserialize")
    }

    #[test]
    fn bundle_correlates_trace_family_and_gateway_events() {
        let target = trace("req-2", "trace-a", 20);
        let previous = trace("req-1", "trace-a", 10);
        let unrelated = trace("req-x", "trace-x", 30);
        let events = vec![GatewayActivityInput {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            status: "host_died".to_string(),
            reason: None,
            dcc_type: "maya".to_string(),
            instance_id: "abcd".to_string(),
        }];

        let bundle = debug_bundle_payload(
            "req-2",
            Vec::new(),
            vec![target, previous, unrelated],
            events,
        )
        .expect("matching trace should produce a bundle");

        assert_eq!(bundle["request_id"], "req-2");
        assert_eq!(bundle["request_ids"], json!(["req-1", "req-2"]));
        assert_eq!(
            bundle["postmortem"]["previous_calls"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            bundle["postmortem"]["gateway_events"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn missing_records_return_none() {
        assert!(debug_bundle_payload("missing", Vec::new(), Vec::new(), Vec::new()).is_none());
    }
}
