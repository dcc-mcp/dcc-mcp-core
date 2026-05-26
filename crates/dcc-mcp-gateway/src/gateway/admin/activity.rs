//! Unified admin activity projection.
//!
//! The dashboard has several raw observability lanes: audit rows, dispatch
//! traces, gateway events, and eventually workflow/job updates.  This module
//! gives both humans and agents a single timeline-shaped interface over those
//! lanes without changing the hot-path writers.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use serde_json::{Value, json};

use super::state::{AdminAuditRecord, AdminState};
use super::trace::DispatchTrace;
use crate::gateway::event_log::ContendEvent;

const POSTMORTEM_PREVIOUS_CALL_LIMIT: usize = 5;
const POSTMORTEM_EVENT_LIMIT: usize = 10;

#[derive(Debug, Clone, Serialize)]
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
    pub token_accounting: Option<super::trace::TokenTelemetry>,
    pub correlation: ActivityCorrelation,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskSnapshot {
    pub task_id: String,
    pub task_type: String,
    pub status: String,
    pub title: String,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub correlation: ActivityCorrelation,
}

pub async fn build_activity_payload(state: &AdminState, limit: usize) -> Value {
    let events = collect_activity_events(state, limit).await;
    json!({ "total": events.len(), "events": events })
}

pub async fn build_tasks_payload(state: &AdminState, limit: usize) -> Value {
    let mut tasks = Vec::new();
    for trace in collect_traces(state, limit).await {
        tasks.push(TaskSnapshot {
            task_id: trace.request_id.clone(),
            task_type: "tool_call".to_string(),
            status: if trace.ok { "completed" } else { "failed" }.to_string(),
            title: trace
                .tool_slug
                .clone()
                .unwrap_or_else(|| trace.method.clone()),
            started_at: rfc3339(trace.started_at),
            duration_ms: Some(trace.total_ms),
            correlation: trace_correlation(&trace),
        });
    }
    json!({ "total": tasks.len(), "tasks": tasks })
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
                .map(audit_event)
                .filter_map(|event| serde_json::to_value(event).ok()),
        )
        .chain(
            matching_traces
                .iter()
                .map(trace_event)
                .filter_map(|event| serde_json::to_value(event).ok()),
        )
        .collect();
    let postmortem = build_postmortem(state, primary_trace, gateway_events).await;
    let primary_trace_value = primary_trace.cloned();
    let hints = debug_hints(primary_trace);
    Some(json!({
        "lookup_id": lookup_id,
        "request_id": primary_request_id,
        "trace_id": trace_id,
        "request_ids": request_ids,
        "audit": primary_audit.map(audit_event),
        "audits": matching_audits.iter().map(audit_event).collect::<Vec<_>>(),
        "trace": primary_trace_value,
        "traces": matching_traces,
        "related_activity": related_activity,
        "postmortem": postmortem,
        "hints": hints,
    }))
}

async fn collect_activity_events(state: &AdminState, limit: usize) -> Vec<ActivityEvent> {
    let mut events = Vec::new();
    for record in collect_audits(state, limit.saturating_mul(2).max(200)).await {
        events.push(audit_event(&record));
    }
    for trace in collect_traces(state, limit.saturating_mul(2).max(200)).await {
        events.push(trace_event(&trace));
    }
    for event in state.gateway.event_log.recent_events(limit.min(500)) {
        events.push(gateway_event(&event));
    }
    events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    events.truncate(limit);
    events
}

pub(super) async fn collect_audits(state: &AdminState, limit: usize) -> Vec<AdminAuditRecord> {
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

pub(super) async fn collect_traces(state: &AdminState, limit: usize) -> Vec<DispatchTrace> {
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

fn audit_event(record: &AdminAuditRecord) -> ActivityEvent {
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

fn trace_event(trace: &DispatchTrace) -> ActivityEvent {
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

fn gateway_event(event: &ContendEvent) -> ActivityEvent {
    let label = event.event.as_label();
    ActivityEvent {
        event_id: format!(
            "gateway:{}:{}:{}",
            event.timestamp, event.dcc_type, event.instance_id
        ),
        timestamp: event.timestamp.clone(),
        kind: "gateway_event".to_string(),
        severity: "info".to_string(),
        status: label.to_string(),
        message: event.reason.clone().unwrap_or_else(|| {
            format!(
                "{label} dcc_type={} instance={}",
                event.dcc_type, event.instance_id
            )
        }),
        tool: None,
        duration_ms: None,
        token_accounting: None,
        correlation: ActivityCorrelation {
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            request_id: None,
            session_id: None,
            instance_id: Some(event.instance_id.clone()),
            dcc_type: Some(event.dcc_type.clone()),
            workflow_id: None,
            job_id: None,
            agent_id: None,
            actor_id: None,
            actor_name: None,
            client_platform: None,
            source_ip: None,
            parent_request_id: None,
        },
    }
}

fn gateway_event_json(event: ContendEvent) -> Value {
    serde_json::to_value(gateway_event(&event)).unwrap_or_else(|_| json!({}))
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
            .and_then(|ctx| ctx.agent_id.clone()),
        actor_id: trace
            .agent_context
            .as_ref()
            .and_then(|ctx| ctx.actor_id.clone()),
        actor_name: trace
            .agent_context
            .as_ref()
            .and_then(|ctx| ctx.actor_name.clone()),
        client_platform: trace
            .agent_context
            .as_ref()
            .and_then(|ctx| ctx.client_platform.clone()),
        source_ip: trace
            .agent_context
            .as_ref()
            .and_then(|ctx| ctx.source_ip.clone()),
        parent_request_id: trace
            .agent_context
            .as_ref()
            .and_then(|ctx| ctx.parent_request_id.clone()),
    }
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
