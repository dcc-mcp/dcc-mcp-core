//! Unified admin activity projection.
//!
//! The dashboard has several raw observability lanes: audit rows, dispatch
//! traces, gateway events, and eventually workflow/job updates.  This module
//! gives both humans and agents a single timeline-shaped interface over those
//! lanes without changing the hot-path writers.

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::UNIX_EPOCH;

use dcc_mcp_gateway_admin::{
    GatewayActivityInput, activity_payload, audit_activity_event, gateway_activity_event_json,
    trace_activity_event,
};
use serde::Serialize;
use serde_json::{Value, json};

pub use dcc_mcp_gateway_admin::{ActivityCorrelation, ActivityEvent};

#[cfg(feature = "admin")]
use crate::gateway::admin::links::AdminLinkBuilder;
use crate::gateway::admin::state::{AdminAuditRecord, AdminState};
use crate::gateway::admin::trace::AgentContext;
use crate::gateway::admin::trace::DispatchTrace;
use crate::gateway::event_log::ContendEvent;

const POSTMORTEM_PREVIOUS_CALL_LIMIT: usize = 5;
const POSTMORTEM_EVENT_LIMIT: usize = 10;
const MAX_TASK_RELATED_IDS: usize = 32;
const MAX_TASK_ARTEFACTS: usize = 8;
const MAX_TASK_VALIDATIONS: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct TaskSnapshot {
    pub task_id: String,
    pub task_type: String,
    pub status: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub app_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<TaskArtifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation_checks: Vec<TaskValidation>,
    pub related: TaskRelated,
    pub correlation: ActivityCorrelation,
    pub links: Value,
    #[serde(skip)]
    sort_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskRelated {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workflow_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskArtifact {
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskValidation {
    pub title: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Debug, Default)]
struct TaskBuilder {
    task_id: String,
    task_type: String,
    goal: Option<String>,
    summary: Option<String>,
    final_result: Option<String>,
    failure_reason: Option<String>,
    started_at: Option<std::time::SystemTime>,
    finished_at: Option<std::time::SystemTime>,
    duration_ms: u64,
    failed: bool,
    warning: bool,
    title_candidates: Vec<String>,
    request_ids: BTreeSet<String>,
    trace_ids: BTreeSet<String>,
    session_ids: BTreeSet<String>,
    workflow_ids: BTreeSet<String>,
    app_types: BTreeSet<String>,
    artifacts: Vec<TaskArtifact>,
    validation_checks: Vec<TaskValidation>,
    primary_request_id: Option<String>,
    correlation: ActivityCorrelation,
}

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
    let tasks = build_task_outcomes(traces, audits, limit, links.clone());
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

#[cfg(feature = "admin")]
fn build_task_outcomes(
    traces: Vec<DispatchTrace>,
    audits: Vec<AdminAuditRecord>,
    limit: usize,
    links: AdminLinkBuilder,
) -> Vec<TaskSnapshot> {
    let trace_by_request: HashMap<String, DispatchTrace> = traces
        .iter()
        .map(|trace| (trace.request_id.clone(), trace.clone()))
        .collect();
    let mut builders: HashMap<String, TaskBuilder> = HashMap::new();

    for trace in traces {
        let (task_type, task_id) = trace_task_key(&trace);
        builders
            .entry(task_id.clone())
            .or_insert_with(|| TaskBuilder::new(task_type, task_id))
            .note_trace(&trace);
    }

    for audit in audits {
        let (task_type, task_id) = trace_by_request
            .get(&audit.request_id)
            .map(trace_task_key)
            .unwrap_or_else(|| audit_task_key(&audit));
        builders
            .entry(task_id.clone())
            .or_insert_with(|| TaskBuilder::new(task_type, task_id))
            .note_audit(&audit);
    }

    let mut rows: Vec<TaskSnapshot> = builders
        .into_values()
        .filter(|builder| !builder.request_ids.is_empty())
        .map(|builder| builder.finish(&links))
        .collect();
    rows.sort_by_key(|row| Reverse(row.sort_ms));
    rows.truncate(limit);
    rows
}

impl TaskBuilder {
    fn new(task_type: String, task_id: String) -> Self {
        let mut workflow_ids = BTreeSet::new();
        workflow_ids.insert(task_id.clone());
        Self {
            task_id,
            task_type,
            workflow_ids,
            ..Self::default()
        }
    }

    fn note_trace(&mut self, trace: &DispatchTrace) {
        let finished_at = trace.started_at + std::time::Duration::from_millis(trace.total_ms);
        self.note_timing(trace.started_at, finished_at, trace.total_ms);
        self.note_request(
            &trace.request_id,
            Some(&trace.trace_id),
            trace.session_id.as_deref(),
            trace.dcc_type.as_deref(),
            !trace.ok,
        );
        if let Some(ctx) = trace.agent_context.as_ref() {
            self.note_agent_context(ctx);
        }
        self.note_title(public_tool_label(trace.tool_slug.as_deref(), &trace.method));
        if !trace.ok {
            self.failure_reason.get_or_insert_with(|| {
                "Request failed; inspect the linked trace for details.".to_string()
            });
        }
        let title = public_tool_label(trace.tool_slug.as_deref(), &trace.method);
        self.note_artifact(&title, trace.request_id.as_str());
        self.note_validation(
            &title,
            if trace.ok { "completed" } else { "failed" },
            trace.request_id.as_str(),
        );
    }

    fn note_audit(&mut self, audit: &AdminAuditRecord) {
        let finished_at = audit
            .duration_ms
            .map(|ms| audit.timestamp + std::time::Duration::from_millis(ms))
            .unwrap_or(audit.timestamp);
        self.note_timing(
            audit.timestamp,
            finished_at,
            audit.duration_ms.unwrap_or_default(),
        );
        self.note_request(
            &audit.request_id,
            audit.trace_id.as_deref(),
            audit.session_id.as_deref(),
            audit.dcc_type.as_deref(),
            !audit.success,
        );
        if let Some(agent_id) = audit.agent_id.as_deref() {
            self.correlation
                .agent_id
                .get_or_insert_with(|| agent_id.to_string());
        }
        if let Some(actor_id) = audit.actor_id.as_deref() {
            self.correlation
                .actor_id
                .get_or_insert_with(|| actor_id.to_string());
        }
        if let Some(actor_name) = audit.actor_name.as_deref() {
            self.correlation
                .actor_name
                .get_or_insert_with(|| actor_name.to_string());
        }
        if let Some(client_platform) = audit.client_platform.as_deref() {
            self.correlation
                .client_platform
                .get_or_insert_with(|| client_platform.to_string());
        }
        if let Some(source_ip) = audit.source_ip.as_deref() {
            self.correlation
                .source_ip
                .get_or_insert_with(|| source_ip.to_string());
        }
        if let Some(parent_request_id) = audit.parent_request_id.as_deref() {
            self.correlation
                .parent_request_id
                .get_or_insert_with(|| parent_request_id.to_string());
        }
        self.note_title(public_tool_label(
            Some(&audit.action),
            audit.method.as_deref().unwrap_or("call"),
        ));
        if !audit.success {
            self.failure_reason = audit
                .error
                .as_deref()
                .map(sanitize_public_text)
                .or_else(|| {
                    Some("Request failed; inspect the linked trace for details.".to_string())
                });
        }
        self.note_artifact(&audit.action, audit.request_id.as_str());
        self.note_validation(
            &audit.action,
            if audit.success { "completed" } else { "failed" },
            audit.request_id.as_str(),
        );
    }

    fn note_timing(
        &mut self,
        started_at: std::time::SystemTime,
        finished_at: std::time::SystemTime,
        duration_ms: u64,
    ) {
        if self.started_at.is_none_or(|current| started_at < current) {
            self.started_at = Some(started_at);
        }
        if self.finished_at.is_none_or(|current| finished_at > current) {
            self.finished_at = Some(finished_at);
        }
        self.duration_ms = self.duration_ms.saturating_add(duration_ms);
    }

    fn note_request(
        &mut self,
        request_id: &str,
        trace_id: Option<&str>,
        session_id: Option<&str>,
        dcc_type: Option<&str>,
        failed: bool,
    ) {
        self.request_ids.insert(request_id.to_string());
        if let Some(trace_id) = trace_id {
            self.trace_ids.insert(trace_id.to_string());
            self.correlation
                .trace_id
                .get_or_insert_with(|| trace_id.to_string());
        }
        if let Some(session_id) = session_id {
            self.session_ids.insert(session_id.to_string());
            self.correlation
                .session_id
                .get_or_insert_with(|| session_id.to_string());
            self.workflow_ids.insert(session_id.to_string());
        }
        if let Some(dcc_type) = dcc_type {
            self.app_types.insert(dcc_type.to_string());
            self.correlation
                .dcc_type
                .get_or_insert_with(|| dcc_type.to_string());
        }
        self.correlation
            .workflow_id
            .get_or_insert_with(|| self.task_id.clone());
        if self.primary_request_id.is_none() || failed {
            self.primary_request_id = Some(request_id.to_string());
            self.correlation.request_id = Some(request_id.to_string());
        }
        if failed {
            self.failed = true;
        }
    }

    fn note_agent_context(&mut self, ctx: &AgentContext) {
        if let Some(task_id) = explicit_task_id_from_context(ctx) {
            self.workflow_ids.insert(task_id);
        }
        prefer_text(&mut self.goal, ctx.task.as_deref());
        prefer_text(&mut self.goal, ctx.user_intent_summary.as_deref());
        prefer_text(&mut self.summary, ctx.user_intent_summary.as_deref());
        prefer_text(&mut self.final_result, ctx.agent_reply_summary.as_deref());
        if let Some(agent_id) = ctx.agent_id.as_deref() {
            self.correlation
                .agent_id
                .get_or_insert_with(|| agent_id.to_string());
        }
        if let Some(actor_id) = ctx.actor_id.as_deref() {
            self.correlation
                .actor_id
                .get_or_insert_with(|| actor_id.to_string());
        }
        if let Some(actor_name) = ctx.actor_name.as_deref() {
            self.correlation
                .actor_name
                .get_or_insert_with(|| actor_name.to_string());
        }
        if let Some(client_platform) = ctx.client_platform.as_deref() {
            self.correlation
                .client_platform
                .get_or_insert_with(|| client_platform.to_string());
        }
        if let Some(source_ip) = ctx.source_ip.as_deref() {
            self.correlation
                .source_ip
                .get_or_insert_with(|| source_ip.to_string());
        }
        if let Some(parent_request_id) = ctx.parent_request_id.as_deref() {
            self.correlation
                .parent_request_id
                .get_or_insert_with(|| parent_request_id.to_string());
        }
    }

    fn note_title(&mut self, title: String) {
        let title = sanitize_public_text(&title);
        if !title.is_empty()
            && !self
                .title_candidates
                .iter()
                .any(|existing| existing == &title)
        {
            self.title_candidates.push(title);
        }
    }

    fn note_artifact(&mut self, title: &str, request_id: &str) {
        let Some(kind) = artifact_kind(title) else {
            return;
        };
        if self.artifacts.len() >= MAX_TASK_ARTEFACTS {
            return;
        }
        let name = sanitize_public_text(&public_tool_label(Some(title), "tool"));
        if self.artifacts.iter().any(|artifact| artifact.name == name) {
            return;
        }
        self.artifacts.push(TaskArtifact {
            name,
            kind: kind.to_string(),
            request_id: Some(request_id.to_string()),
        });
    }

    fn note_validation(&mut self, title: &str, status: &str, request_id: &str) {
        if !is_validation_step(title) || self.validation_checks.len() >= MAX_TASK_VALIDATIONS {
            return;
        }
        let title = sanitize_public_text(&public_tool_label(Some(title), "validation"));
        if self
            .validation_checks
            .iter()
            .any(|validation| validation.title == title)
        {
            return;
        }
        self.validation_checks.push(TaskValidation {
            title,
            status: status.to_string(),
            request_id: Some(request_id.to_string()),
        });
    }

    #[cfg(feature = "admin")]
    fn finish(self, links: &AdminLinkBuilder) -> TaskSnapshot {
        let started_at = self.started_at.unwrap_or(UNIX_EPOCH);
        let finished_at = self.finished_at;
        let duration_ms = finished_at
            .and_then(|finish| finish.duration_since(started_at).ok())
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .or_else(|| (self.duration_ms > 0).then_some(self.duration_ms));
        let title = self
            .goal
            .clone()
            .or_else(|| self.title_candidates.first().cloned())
            .unwrap_or_else(|| "Gateway task outcome".to_string());
        let status = if self.failed {
            "failed"
        } else if self.warning {
            "warning"
        } else {
            "completed"
        }
        .to_string();
        let final_result = self.final_result.clone().or_else(|| {
            if !self.artifacts.is_empty() {
                Some(format!(
                    "Produced {} deliverable(s): {}",
                    self.artifacts.len(),
                    self.artifacts
                        .iter()
                        .map(|artifact| artifact.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            } else {
                None
            }
        });
        let sort_ms = timestamp_ms(finished_at.unwrap_or(started_at));
        let primary_request = self.primary_request_id.as_deref();
        TaskSnapshot {
            task_id: self.task_id,
            task_type: self.task_type,
            status,
            title,
            goal: self.goal,
            summary: self.summary,
            final_result,
            failure_reason: self.failure_reason,
            started_at: rfc3339(started_at),
            finished_at: finished_at.map(rfc3339),
            duration_ms,
            app_types: limit_set(self.app_types),
            artifacts: self.artifacts,
            validation_checks: self.validation_checks,
            related: TaskRelated {
                workflow_ids: limit_set(self.workflow_ids),
                request_ids: limit_set(self.request_ids),
                trace_ids: limit_set(self.trace_ids),
                session_ids: limit_set(self.session_ids),
            },
            correlation: self.correlation,
            links: task_links(links, primary_request),
            sort_ms,
        }
    }
}

fn trace_task_key(trace: &DispatchTrace) -> (String, String) {
    if let Some(ctx) = trace.agent_context.as_ref() {
        if let Some(task_id) = explicit_task_id_from_context(ctx) {
            return ("agent_task".to_string(), task_id);
        }
        if let Some(session_id) = ctx.session_id.as_deref().or(trace.session_id.as_deref()) {
            if let Some(turn_id) = ctx.turn_id.as_deref() {
                return ("agent_turn".to_string(), format!("{session_id}:{turn_id}"));
            }
            return ("session_task".to_string(), session_id.to_string());
        }
    }
    if let Some(session_id) = trace.session_id.as_deref() {
        return ("session_task".to_string(), session_id.to_string());
    }
    if !trace.trace_id.is_empty() {
        return ("trace_task".to_string(), trace.trace_id.clone());
    }
    ("request_task".to_string(), trace.request_id.clone())
}

fn audit_task_key(audit: &AdminAuditRecord) -> (String, String) {
    if let Some(session_id) = audit.session_id.as_deref() {
        return ("session_task".to_string(), session_id.to_string());
    }
    if let Some(trace_id) = audit.trace_id.as_deref() {
        return ("trace_task".to_string(), trace_id.to_string());
    }
    if let Some(parent_request_id) = audit.parent_request_id.as_deref() {
        return ("request_chain".to_string(), parent_request_id.to_string());
    }
    ("request_task".to_string(), audit.request_id.clone())
}

fn explicit_task_id_from_context(ctx: &AgentContext) -> Option<String> {
    let metadata = ctx.metadata.as_object()?;
    for key in [
        "task_id",
        "taskId",
        "workflow_id",
        "workflowId",
        "goal_id",
        "goalId",
    ] {
        if let Some(value) = metadata
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

fn prefer_text(slot: &mut Option<String>, candidate: Option<&str>) {
    if slot.is_some() {
        return;
    }
    let Some(candidate) = candidate.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    *slot = Some(sanitize_public_text(candidate));
}

fn public_tool_label(tool_slug: Option<&str>, method: &str) -> String {
    let raw = tool_slug.unwrap_or(method);
    raw.rsplit("__")
        .next()
        .unwrap_or(raw)
        .rsplit('.')
        .next()
        .unwrap_or(raw)
        .replace(['_', '-'], " ")
}

fn artifact_kind(title: &str) -> Option<&'static str> {
    let lower = title.to_ascii_lowercase();
    if lower.contains("screenshot") || lower.contains("capture") {
        Some("screenshot")
    } else if lower.contains("render") || lower.contains("preview") {
        Some("render")
    } else if lower.contains("export") {
        Some("export")
    } else if lower.contains("save") {
        Some("save")
    } else if lower.contains("artifact") {
        Some("artifact")
    } else {
        None
    }
}

fn is_validation_step(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    ["validate", "validation", "verify", "check", "test", "lint"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn sanitize_public_text(value: &str) -> String {
    value
        .split_whitespace()
        .map(|part| {
            let trimmed = part.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
                )
            });
            if looks_like_url(trimmed) {
                "[url-redacted]".to_string()
            } else if looks_like_absolute_path(trimmed) {
                "[path-redacted]".to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn looks_like_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() >= 3
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
        && bytes[0].is_ascii_alphabetic()
    {
        return true;
    }
    value.starts_with("\\\\")
        || value.starts_with("/Users/")
        || value.starts_with("/home/")
        || value.starts_with("/mnt/")
        || value.starts_with("/studio/")
}

#[cfg(feature = "admin")]
fn task_links(links: &AdminLinkBuilder, primary_request_id: Option<&str>) -> Value {
    let mut payload = json!({
        "admin_tasks_url": links.panel_url("tasks"),
        "admin_workflows_url": links.panel_url("workflows"),
        "admin_calls_url": links.panel_url("calls"),
        "admin_traces_url": links.panel_url("traces"),
    });
    if let Some(request_id) = primary_request_id
        && let Some(map) = payload.as_object_mut()
    {
        map.insert(
            "primary_request".to_string(),
            links.request_links(request_id),
        );
    }
    payload
}

fn limit_set(values: BTreeSet<String>) -> Vec<String> {
    values.into_iter().take(MAX_TASK_RELATED_IDS).collect()
}

fn timestamp_ms(t: std::time::SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
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
