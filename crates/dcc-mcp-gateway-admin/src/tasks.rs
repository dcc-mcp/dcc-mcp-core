//! Pure task-outcome projections derived from admin audits and traces.

use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{AdminAuditRecord, AdminLinkBuilder, AgentContext, DispatchTrace};

const MAX_TASK_RELATED_IDS: usize = 32;
const MAX_TASK_ARTEFACTS: usize = 8;
const MAX_TASK_VALIDATIONS: usize = 8;

/// Aggregated task outcome exposed by the admin tasks endpoint.
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
    pub correlation: crate::ActivityCorrelation,
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
    started_at: Option<SystemTime>,
    finished_at: Option<SystemTime>,
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
    correlation: crate::ActivityCorrelation,
}

/// Build the filtered task-outcome payload returned by the admin API.
#[must_use]
pub fn task_payload(
    traces: Vec<DispatchTrace>,
    audits: Vec<AdminAuditRecord>,
    limit: usize,
    links: &AdminLinkBuilder,
) -> Value {
    let tasks = task_outcomes(traces, audits, limit, links);
    json!({ "total": tasks.len(), "tasks": tasks })
}

fn task_outcomes(
    traces: Vec<DispatchTrace>,
    audits: Vec<AdminAuditRecord>,
    limit: usize,
    links: &AdminLinkBuilder,
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

    let mut rows = builders
        .into_values()
        .filter(|builder| !builder.request_ids.is_empty())
        .map(|builder| builder.finish(links))
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| std::cmp::Reverse(row.sort_ms));
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
        if let Some(context) = trace.agent_context.as_ref() {
            self.note_agent_context(context);
        }
        let title = public_tool_label(trace.tool_slug.as_deref(), &trace.method);
        self.note_title(title.clone());
        if !trace.ok {
            self.failure_reason.get_or_insert_with(|| {
                "Request failed; inspect the linked trace for details.".to_string()
            });
        }
        self.note_artifact(&title, &trace.request_id);
        self.note_validation(
            &title,
            if trace.ok { "completed" } else { "failed" },
            &trace.request_id,
        );
    }

    fn note_audit(&mut self, audit: &AdminAuditRecord) {
        let finished_at = audit
            .duration_ms
            .map(|duration| audit.timestamp + std::time::Duration::from_millis(duration))
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
        set_if_missing(&mut self.correlation.agent_id, audit.agent_id.as_deref());
        set_if_missing(&mut self.correlation.actor_id, audit.actor_id.as_deref());
        set_if_missing(
            &mut self.correlation.actor_name,
            audit.actor_name.as_deref(),
        );
        set_if_missing(
            &mut self.correlation.client_platform,
            audit.client_platform.as_deref(),
        );
        set_if_missing(&mut self.correlation.source_ip, audit.source_ip.as_deref());
        set_if_missing(
            &mut self.correlation.parent_request_id,
            audit.parent_request_id.as_deref(),
        );
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
        self.note_artifact(&audit.action, &audit.request_id);
        self.note_validation(
            &audit.action,
            if audit.success { "completed" } else { "failed" },
            &audit.request_id,
        );
    }

    fn note_timing(&mut self, started_at: SystemTime, finished_at: SystemTime, duration_ms: u64) {
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
        self.failed |= failed;
    }

    fn note_agent_context(&mut self, context: &AgentContext) {
        if let Some(task_id) = explicit_task_id_from_context(context) {
            self.workflow_ids.insert(task_id);
        }
        prefer_text(&mut self.goal, context.task.as_deref());
        prefer_text(&mut self.goal, context.user_intent_summary.as_deref());
        prefer_text(&mut self.summary, context.user_intent_summary.as_deref());
        prefer_text(
            &mut self.final_result,
            context.agent_reply_summary.as_deref(),
        );
        set_if_missing(&mut self.correlation.agent_id, context.agent_id.as_deref());
        set_if_missing(&mut self.correlation.actor_id, context.actor_id.as_deref());
        set_if_missing(
            &mut self.correlation.actor_name,
            context.actor_name.as_deref(),
        );
        set_if_missing(
            &mut self.correlation.client_platform,
            context.client_platform.as_deref(),
        );
        set_if_missing(
            &mut self.correlation.source_ip,
            context.source_ip.as_deref(),
        );
        set_if_missing(
            &mut self.correlation.parent_request_id,
            context.parent_request_id.as_deref(),
        );
    }
}

impl TaskBuilder {
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
            if self.artifacts.is_empty() {
                None
            } else {
                Some(format!(
                    "Produced {} deliverable(s): {}",
                    self.artifacts.len(),
                    self.artifacts
                        .iter()
                        .map(|artifact| artifact.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
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
    if let Some(context) = trace.agent_context.as_ref() {
        if let Some(task_id) = explicit_task_id_from_context(context) {
            return ("agent_task".to_string(), task_id);
        }
        if let Some(session_id) = context
            .session_id
            .as_deref()
            .or(trace.session_id.as_deref())
        {
            if let Some(turn_id) = context.turn_id.as_deref() {
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

fn explicit_task_id_from_context(context: &AgentContext) -> Option<String> {
    let metadata = context.metadata.as_object()?;
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

fn set_if_missing(slot: &mut Option<String>, candidate: Option<&str>) {
    if let Some(candidate) = candidate {
        slot.get_or_insert_with(|| candidate.to_string());
    }
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
            let trimmed = part.trim_matches(|character: char| {
                matches!(
                    character,
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

fn timestamp_ms(timestamp: SystemTime) -> u64 {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
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

    #[test]
    fn public_text_redacts_urls_and_absolute_paths() {
        assert_eq!(
            sanitize_public_text("saved C:\\shots\\frame.png at https://example.invalid"),
            "saved [path-redacted] at [url-redacted]"
        );
    }

    #[test]
    fn task_classifiers_are_backend_neutral() {
        assert_eq!(artifact_kind("capture viewport"), Some("screenshot"));
        assert_eq!(artifact_kind("export usd"), Some("export"));
        assert!(is_validation_step("validate scene"));
        assert_eq!(
            public_tool_label(Some("maya__validate_scene"), "call"),
            "validate scene"
        );
    }
}
