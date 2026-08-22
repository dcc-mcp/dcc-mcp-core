//! Unified admin activity projection.
//!
//! The dashboard has several raw observability lanes: audit rows, dispatch
//! traces, gateway events, and eventually workflow/job updates.  This module
//! gives both humans and agents a single timeline-shaped interface over those
//! lanes without changing the hot-path writers.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::time::UNIX_EPOCH;

#[cfg(feature = "admin")]
use dcc_mcp_gateway_admin::task_payload;
use dcc_mcp_gateway_admin::{
    GatewayActivityInput, activity_payload, audit_activity_event, gateway_activity_event_json,
    trace_activity_event,
};
use serde_json::{Value, json};

pub use dcc_mcp_gateway_admin::{
    ActivityCorrelation, ActivityEvent, TaskArtifact, TaskRelated, TaskSnapshot, TaskValidation,
};

#[cfg(feature = "admin")]
use crate::gateway::admin::links::AdminLinkBuilder;
use crate::gateway::admin::state::{AdminAuditRecord, AdminState};
use crate::gateway::admin::trace::DispatchTrace;
use crate::gateway::event_log::ContendEvent;

const POSTMORTEM_PREVIOUS_CALL_LIMIT: usize = 5;
const POSTMORTEM_EVENT_LIMIT: usize = 10;
pub async fn build_activity_payload(state: &AdminState, limit: usize) -> Value {
    let fetch_limit = limit.saturating_mul(2).max(200);
    let audits = collect_audits(state, fetch_limit).await;
    let traces = collect_traces(state, fetch_limit).await;
    let gateway_events = state
        .gateway
        .event_log
        .recent_events(limit.min(500))
        .iter()
        .map(gateway_activity_input)
        .collect::<Vec<_>>();
    activity_payload(&audits, &traces, &gateway_events, limit)
}

#[cfg(feature = "admin")]
pub(crate) async fn build_tasks_payload(
    state: &AdminState,
    limit: usize,
    links: AdminLinkBuilder,
) -> Value {
    let fetch_limit = limit.saturating_mul(4).max(500);
    let traces = collect_traces(state, fetch_limit).await;
    let audits = collect_audits(state, fetch_limit).await;
    task_payload(traces, audits, limit, &links)
}

pub async fn build_debug_bundle(state: &AdminState, lookup_id: &str) -> Option<Value> {
    let audits = collect_audits(state, 1_000).await;
    let all_traces = collect_traces(state, 1_000).await;
    let mut matching_traces: Vec<DispatchTrace> = all_traces
        .iter()
        .filter(|trace| trace.request_id == lookup_id || trace.trace_id == lookup_id)
        .cloned()
        .collect();
    if matching_traces.is_empty()
        && let Some(trace) = find_trace(state, lookup_id).await
    {
        matching_traces.push(trace);
    }
    matching_traces.sort_by_key(|trace| Reverse(trace.started_at));
    let trace_id = matching_traces.first().map(|trace| trace.trace_id.clone());
    if let Some(trace_id) = trace_id.as_deref() {
        let extra_traces: Vec<DispatchTrace> = all_traces
            .iter()
            .filter(|trace| trace.trace_id == trace_id)
            .filter(|trace| {
                !matching_traces
                    .iter()
                    .any(|existing| existing.request_id == trace.request_id)
            })
            .cloned()
            .collect();
        for trace in extra_traces {
            matching_traces.push(trace);
        }
    }
    let request_ids: HashSet<String> = matching_traces
        .iter()
        .map(|trace| trace.request_id.clone())
        .collect();
    let matching_audits: Vec<AdminAuditRecord> = audits
        .into_iter()
        .filter(|record| {
            record.request_id == lookup_id
                || request_ids.contains(&record.request_id)
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
        .or_else(|| matching_traces.first());
    let primary_audit = matching_audits
        .iter()
        .find(|record| record.request_id == lookup_id)
        .or_else(|| matching_audits.first());
    let primary_request_id = primary_trace
        .map(|trace| trace.request_id.clone())
        .or_else(|| primary_audit.map(|record| record.request_id.clone()))
        .unwrap_or_else(|| lookup_id.to_string());
    let mut request_ids: Vec<String> = request_ids.into_iter().collect();
    if !request_ids.iter().any(|id| id == &primary_request_id) {
        request_ids.push(primary_request_id.clone());
    }
    request_ids.sort();

    let gateway_events = related_gateway_events(state, &request_ids, primary_trace);
    let related_activity: Vec<Value> = gateway_events
        .clone()
        .into_iter()
        .map(gateway_event_json)
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
    let postmortem = build_postmortem(state, primary_trace, gateway_events).await;
    let primary_trace_value = primary_trace.cloned();
    let hints = debug_hints(primary_trace);
    let root_cause = primary_audit
        .and_then(|record| record.error.clone())
        .or_else(|| {
            primary_trace
                .filter(|trace| !trace.ok)
                .and_then(|_| hints.first().cloned())
        });
    Some(json!({
        "lookup_id": lookup_id,
        "request_id": primary_request_id,
        "trace_id": trace_id,
        "request_ids": request_ids,
        "root_cause": root_cause,
        "audit": primary_audit.map(audit_activity_event),
        "audits": matching_audits.iter().map(audit_activity_event).collect::<Vec<_>>(),
        "trace": primary_trace_value,
        "traces": matching_traces,
        "related_activity": related_activity,
        "postmortem": postmortem,
        "hints": hints,
    }))
}

pub async fn collect_audits(state: &AdminState, limit: usize) -> Vec<AdminAuditRecord> {
    let mut by_id: HashMap<String, AdminAuditRecord> = HashMap::new();
    if let Some(lane) = &state.admin_sqlite_lane {
        for rec in lane
            .reader()
            .list_audits_recent(limit.saturating_mul(4).max(500))
        {
            by_id.insert(rec.request_id.clone(), rec);
        }
    }
    if let Some(log) = &state.audit_log {
        for rec in log.lock().iter().rev().take(limit) {
            by_id.insert(rec.request_id.clone(), rec.clone());
        }
    }
    let mut rows: Vec<_> = by_id.into_values().collect();
    rows.sort_by_key(|row| Reverse(row.timestamp));
    rows.truncate(limit);
    rows
}

pub async fn collect_traces(state: &AdminState, limit: usize) -> Vec<DispatchTrace> {
    let mut by_id: HashMap<String, DispatchTrace> = HashMap::new();
    if let Some(lane) = &state.admin_sqlite_lane {
        for trace in lane
            .reader()
            .list_traces_since(None, limit.saturating_mul(4).max(500))
        {
            by_id.insert(trace.request_id.clone(), trace);
        }
    }
    if let Some(log) = &state.trace_log {
        for trace in log.recent(limit) {
            by_id.insert(trace.request_id.clone(), trace);
        }
    }
    let mut rows: Vec<_> = by_id.into_values().collect();
    rows.sort_by_key(|row| Reverse(row.started_at));
    rows.truncate(limit);
    rows
}

async fn find_trace(state: &AdminState, request_id: &str) -> Option<DispatchTrace> {
    if let Some(trace) = state.trace_log.as_ref().and_then(|log| log.get(request_id)) {
        return Some(trace);
    }
    state
        .admin_sqlite_lane
        .as_ref()
        .and_then(|lane| lane.reader().get_trace(request_id))
}

fn gateway_event_json(event: ContendEvent) -> Value {
    gateway_activity_event_json(&gateway_activity_input(&event))
}

fn gateway_activity_input(event: &ContendEvent) -> GatewayActivityInput {
    GatewayActivityInput {
        timestamp: event.timestamp.clone(),
        status: event.event.as_label().to_string(),
        reason: event.reason.clone(),
        dcc_type: event.dcc_type.clone(),
        instance_id: event.instance_id.clone(),
    }
}

fn related_gateway_events(
    state: &AdminState,
    request_ids: &[String],
    trace: Option<&DispatchTrace>,
) -> Vec<ContendEvent> {
    state
        .gateway
        .event_log
        .recent_events(500)
        .into_iter()
        .filter(|event| gateway_event_matches(event, request_ids, trace))
        .take(POSTMORTEM_EVENT_LIMIT)
        .collect()
}

fn gateway_event_matches(
    event: &ContendEvent,
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
        && event.event.as_label() == "host_died"
}

async fn build_postmortem(
    state: &AdminState,
    trace: Option<&DispatchTrace>,
    gateway_events: Vec<ContendEvent>,
) -> Value {
    let Some(trace) = trace else {
        return json!({
            "previous_calls": [],
            "gateway_events": gateway_events.into_iter().map(gateway_event_json).collect::<Vec<_>>(),
        });
    };

    let previous_calls: Vec<Value> = collect_traces(state, 1_000)
        .await
        .into_iter()
        .filter(|candidate| candidate.request_id != trace.request_id)
        .filter(|candidate| candidate.started_at <= trace.started_at)
        .filter(|candidate| trace_matches_postmortem_scope(candidate, trace))
        .take(POSTMORTEM_PREVIOUS_CALL_LIMIT)
        .map(postmortem_trace_row)
        .collect();

    json!({
        "target": postmortem_trace_row(trace.clone()),
        "previous_calls": previous_calls,
        "gateway_events": gateway_events.into_iter().map(gateway_event_json).collect::<Vec<_>>(),
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
        .filter(|c| c.is_ascii_hexdigit())
        .flat_map(|c| c.to_lowercase())
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

fn rfc3339(t: std::time::SystemTime) -> String {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .map(|_| {
            chrono::DateTime::<chrono::Utc>::from(t)
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        })
        .unwrap_or_default()
}
