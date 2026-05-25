//! Agent session/workflow projection for the Admin UI.
//!
//! This is a read-only view over the existing trace, audit, and search
//! telemetry stores. It deliberately keeps only bounded caller metadata and
//! correlation IDs; hidden reasoning, raw prompts, and arbitrary request bodies
//! stay in the existing trace/debug-bundle paths.

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};

use super::activity::{collect_audits, collect_traces};
use super::links::AdminLinkBuilder;
use super::state::{AdminAuditRecord, AdminState};
use super::trace::{AgentContext, DispatchTrace};
use crate::gateway::search_telemetry::{
    SearchFollowupTelemetry, SearchTelemetryHit, SearchTelemetryRecord,
};

const MAX_WORKFLOW_STEPS: usize = 64;
const MAX_WORKFLOW_IDS: usize = 32;
const MAX_AGENT_TAGS: usize = 16;

#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkflowAgent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkflowCorrelation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trace_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkflowSearchSignal {
    pub search_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_rank: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_score: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zero_results: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_success_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowStep {
    pub step_id: String,
    pub kind: String,
    pub title: String,
    pub timestamp: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<WorkflowSearchSignal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Value>,
    #[serde(skip)]
    sort_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkflowDiscoverySummary {
    pub search_count: usize,
    pub zero_result_count: usize,
    pub selected_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_selected_rank: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_first_success_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowView {
    pub workflow_id: String,
    pub group_kind: String,
    pub title: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub step_count: usize,
    pub failed_steps: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<WorkflowAgent>,
    pub correlation: WorkflowCorrelation,
    pub discovery: WorkflowDiscoverySummary,
    pub steps: Vec<WorkflowStep>,
    pub links: Value,
    #[serde(skip)]
    sort_ms: u64,
}

#[derive(Default)]
struct WorkflowBuilder {
    workflow_id: String,
    group_kind: String,
    steps: Vec<WorkflowStep>,
    agent: Option<WorkflowAgent>,
    request_ids: BTreeSet<String>,
    trace_ids: BTreeSet<String>,
    session_ids: BTreeSet<String>,
    agent_id: Option<String>,
}

pub(super) async fn build_workflows_payload(
    state: &AdminState,
    limit: usize,
    links: AdminLinkBuilder,
) -> Value {
    let fetch_limit = limit.saturating_mul(4).max(500);
    let traces = collect_traces(state, fetch_limit).await;
    let audits = collect_audits(state, fetch_limit).await;
    let search_snapshot = state.gateway.search_telemetry.snapshot(fetch_limit);
    let workflows = build_workflows(traces, audits, search_snapshot.recent, limit, links.clone());
    let failed = workflows
        .iter()
        .filter(|workflow| workflow.status == "failed")
        .count();
    let warnings = workflows
        .iter()
        .filter(|workflow| workflow.status == "warning")
        .count();
    let zero_result_workflows = workflows
        .iter()
        .filter(|workflow| workflow.discovery.zero_result_count > 0)
        .count();
    json!({
        "total": workflows.len(),
        "summary": {
            "failed": failed,
            "warning": warnings,
            "zero_result_workflows": zero_result_workflows,
        },
        "links": links.workflow_links(),
        "workflows": workflows,
    })
}

fn build_workflows(
    traces: Vec<DispatchTrace>,
    audits: Vec<AdminAuditRecord>,
    searches: Vec<SearchTelemetryRecord>,
    limit: usize,
    links: AdminLinkBuilder,
) -> Vec<WorkflowView> {
    let trace_by_request: HashMap<String, DispatchTrace> = traces
        .iter()
        .map(|trace| (trace.request_id.clone(), trace.clone()))
        .collect();
    let audit_by_request: HashMap<String, AdminAuditRecord> = audits
        .iter()
        .map(|audit| (audit.request_id.clone(), audit.clone()))
        .collect();
    let parent_by_request = parent_index(&traces, &audits);
    let mut represented_traces = BTreeSet::new();
    let mut builders: HashMap<String, WorkflowBuilder> = HashMap::new();

    for search in searches {
        let (key, kind, id) = search_group_key(&search);
        let builder = builders
            .entry(key)
            .or_insert_with(|| WorkflowBuilder::new(kind, id));
        builder.push_step(search_step(&search));
        builder.note_search(&search);

        for followup in &search.followups {
            let step = followup_step(&search, followup, &trace_by_request, &links);
            let (f_key, f_kind, f_id) = followup
                .request_id
                .as_deref()
                .and_then(|request_id| trace_by_request.get(request_id))
                .map(|trace| trace_group_key(trace, &parent_by_request))
                .unwrap_or_else(|| followup_group_key(&search, followup));
            if let Some(request_id) = followup.request_id.as_deref() {
                represented_traces.insert(request_id.to_string());
            }
            let followup_builder = builders
                .entry(f_key)
                .or_insert_with(|| WorkflowBuilder::new(f_kind, f_id));
            if let Some(trace) = followup
                .request_id
                .as_deref()
                .and_then(|request_id| trace_by_request.get(request_id))
            {
                followup_builder.note_trace(trace);
            }
            followup_builder.push_step(step);
            followup_builder.note_search(&search);
        }
    }

    for trace in traces {
        if represented_traces.contains(&trace.request_id) {
            continue;
        }
        let (key, kind, id) = trace_group_key(&trace, &parent_by_request);
        let builder = builders
            .entry(key)
            .or_insert_with(|| WorkflowBuilder::new(kind, id));
        builder.note_trace(&trace);
        builder.push_step(trace_step(&trace, "call", None, &links));
    }

    for audit in audits {
        if trace_by_request.contains_key(&audit.request_id) {
            continue;
        }
        let (key, kind, id) = audit_group_key(&audit, &parent_by_request);
        let builder = builders
            .entry(key)
            .or_insert_with(|| WorkflowBuilder::new(kind, id));
        builder.note_audit(&audit);
        builder.push_step(audit_step(&audit, &links));
    }

    let mut rows: Vec<WorkflowView> = builders
        .into_values()
        .filter(|builder| !builder.steps.is_empty())
        .map(|builder| builder.finish(&audit_by_request, &links))
        .collect();
    rows.sort_by_key(|row| Reverse(row.sort_ms));
    rows.truncate(limit);
    rows
}

impl WorkflowBuilder {
    fn new(group_kind: String, workflow_id: String) -> Self {
        Self {
            workflow_id,
            group_kind,
            ..Self::default()
        }
    }

    fn push_step(&mut self, step: WorkflowStep) {
        if let Some(request_id) = step.request_id.as_deref() {
            self.request_ids.insert(request_id.to_string());
        }
        if let Some(trace_id) = step.trace_id.as_deref() {
            self.trace_ids.insert(trace_id.to_string());
        }
        if let Some(session_id) = step.session_id.as_deref() {
            self.session_ids.insert(session_id.to_string());
        }
        self.steps.push(step);
    }

    fn note_trace(&mut self, trace: &DispatchTrace) {
        if self.agent.is_none() {
            self.agent = trace.agent_context.as_ref().map(agent_from_context);
        }
        if self.agent_id.is_none() {
            self.agent_id = trace
                .agent_context
                .as_ref()
                .and_then(|ctx| ctx.agent_id.clone());
        }
    }

    fn note_audit(&mut self, audit: &AdminAuditRecord) {
        if self.agent.is_none()
            && (audit.agent_id.is_some()
                || audit.agent_name.is_some()
                || audit.agent_model.is_some())
        {
            self.agent = Some(WorkflowAgent {
                agent_id: audit.agent_id.clone(),
                agent_name: audit.agent_name.clone(),
                model: audit.agent_model.clone(),
                ..WorkflowAgent::default()
            });
        }
        if self.agent_id.is_none() {
            self.agent_id = audit.agent_id.clone();
        }
    }

    fn note_search(&mut self, search: &SearchTelemetryRecord) {
        if let Some(request_id) = search.request_id.as_deref() {
            self.request_ids.insert(request_id.to_string());
        }
        if let Some(trace_id) = search.trace_id.as_deref() {
            self.trace_ids.insert(trace_id.to_string());
        }
        if let Some(session_id) = search.session_id.as_deref() {
            self.session_ids.insert(session_id.to_string());
        }
    }

    fn finish(
        mut self,
        audit_by_request: &HashMap<String, AdminAuditRecord>,
        links: &AdminLinkBuilder,
    ) -> WorkflowView {
        self.steps.sort_by_key(|step| step.sort_ms);
        self.steps.truncate(MAX_WORKFLOW_STEPS);
        if self.agent.is_none() || self.agent_id.is_none() {
            let audit_agent = self
                .steps
                .iter()
                .filter_map(|step| step.request_id.as_deref())
                .filter_map(|request_id| audit_by_request.get(request_id))
                .find(|audit| {
                    audit.agent_id.is_some()
                        || audit.agent_name.is_some()
                        || audit.agent_model.is_some()
                });
            if let Some(audit) = audit_agent {
                if self.agent.is_none() {
                    self.agent = Some(agent_from_audit(audit));
                }
                if self.agent_id.is_none() {
                    self.agent_id = audit.agent_id.clone();
                }
            }
        }

        let started_ms = self.steps.first().map(|step| step.sort_ms).unwrap_or(0);
        let finished_ms = self.steps.last().map(|step| step.sort_ms).unwrap_or(0);
        let failed_steps = self
            .steps
            .iter()
            .filter(|step| step.kind != "search" && step.success == Some(false))
            .count();
        let zero_result_steps = self
            .steps
            .iter()
            .filter(|step| {
                step.search
                    .as_ref()
                    .and_then(|search| search.zero_results)
                    .unwrap_or(false)
            })
            .count();
        let status = if failed_steps > 0 {
            "failed"
        } else if zero_result_steps > 0 {
            "warning"
        } else {
            "completed"
        }
        .to_string();
        let title = workflow_title(&self);
        let request_ids = limit_set(self.request_ids);
        let trace_ids = limit_set(self.trace_ids);
        let session_ids = limit_set(self.session_ids);
        let correlation = WorkflowCorrelation {
            session_id: session_ids.first().cloned(),
            trace_id: trace_ids.first().cloned(),
            agent_id: self.agent_id.clone(),
            request_ids,
            trace_ids,
            session_ids,
        };
        let discovery = discovery_summary(&self.steps);
        let workflow_links = self
            .steps
            .iter()
            .find_map(|step| step.request_id.as_deref())
            .map(|request_id| links.request_links(request_id))
            .unwrap_or_else(|| links.workflow_links());

        WorkflowView {
            workflow_id: self.workflow_id,
            group_kind: self.group_kind,
            title,
            status,
            started_at: rfc3339_ms(started_ms),
            finished_at: rfc3339_ms(finished_ms),
            duration_ms: (finished_ms >= started_ms).then_some(finished_ms - started_ms),
            step_count: self.steps.len(),
            failed_steps,
            agent: self.agent,
            correlation,
            discovery,
            steps: self.steps,
            links: workflow_links,
            sort_ms: finished_ms,
        }
    }
}

fn parent_index(
    traces: &[DispatchTrace],
    audits: &[AdminAuditRecord],
) -> HashMap<String, Option<String>> {
    let mut map = HashMap::new();
    for trace in traces {
        map.insert(trace.request_id.clone(), trace.parent_request_id.clone());
    }
    for audit in audits {
        map.entry(audit.request_id.clone())
            .or_insert_with(|| audit.parent_request_id.clone());
    }
    map
}

fn trace_group_key(
    trace: &DispatchTrace,
    parent_by_request: &HashMap<String, Option<String>>,
) -> (String, String, String) {
    if let Some(session_id) = trace
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return keyed("session", session_id.to_string());
    }
    if let Some(workflow_id) = trace
        .agent_context
        .as_ref()
        .and_then(workflow_id_from_context)
    {
        return keyed("workflow", workflow_id);
    }
    if !trace.trace_id.is_empty() {
        return keyed("trace", trace.trace_id.clone());
    }
    keyed(
        "request",
        root_request_id(&trace.request_id, parent_by_request),
    )
}

fn audit_group_key(
    audit: &AdminAuditRecord,
    parent_by_request: &HashMap<String, Option<String>>,
) -> (String, String, String) {
    if let Some(session_id) = audit
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return keyed("session", session_id.to_string());
    }
    if let Some(trace_id) = audit.trace_id.as_deref().filter(|value| !value.is_empty()) {
        return keyed("trace", trace_id.to_string());
    }
    keyed(
        "request",
        root_request_id(&audit.request_id, parent_by_request),
    )
}

fn search_group_key(search: &SearchTelemetryRecord) -> (String, String, String) {
    if let Some(session_id) = search
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return keyed("session", session_id.to_string());
    }
    if let Some(trace_id) = search.trace_id.as_deref().filter(|value| !value.is_empty()) {
        return keyed("trace", trace_id.to_string());
    }
    if let Some(request_id) = search
        .request_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return keyed("request", request_id.to_string());
    }
    keyed("search", search.search_id.clone())
}

fn followup_group_key(
    search: &SearchTelemetryRecord,
    followup: &SearchFollowupTelemetry,
) -> (String, String, String) {
    if let Some(session_id) = search
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return keyed("session", session_id.to_string());
    }
    if let Some(trace_id) = followup
        .trace_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return keyed("trace", trace_id.to_string());
    }
    search_group_key(search)
}

fn keyed(kind: &str, id: String) -> (String, String, String) {
    (format!("{kind}:{id}"), kind.to_string(), id)
}

fn root_request_id(
    request_id: &str,
    parent_by_request: &HashMap<String, Option<String>>,
) -> String {
    let mut current = request_id.to_string();
    for _ in 0..32 {
        let Some(Some(parent)) = parent_by_request.get(&current) else {
            return current;
        };
        if parent == &current || parent.is_empty() {
            return current;
        }
        if !parent_by_request.contains_key(parent) {
            return current;
        }
        current = parent.clone();
    }
    current
}

fn search_step(search: &SearchTelemetryRecord) -> WorkflowStep {
    let status = if search.zero_results {
        "zero_results"
    } else {
        "ok"
    };
    WorkflowStep {
        step_id: format!("search:{}", search.search_id),
        kind: "search".to_string(),
        title: search
            .query_preview
            .as_ref()
            .map(|query| format!("search {query}"))
            .unwrap_or_else(|| "search tools".to_string()),
        timestamp: rfc3339_ms(search.timestamp_ms),
        status: status.to_string(),
        success: Some(!search.zero_results),
        request_id: search.request_id.clone(),
        trace_id: search.trace_id.clone(),
        parent_request_id: None,
        session_id: search.session_id.clone(),
        dcc_type: search.dcc_type.clone(),
        instance_id: search.instance_id.clone(),
        tool: None,
        transport: Some(search.transport.clone()),
        duration_ms: None,
        search: Some(WorkflowSearchSignal {
            search_id: search.search_id.clone(),
            zero_results: Some(search.zero_results),
            result_count: Some(search.total),
            first_success_ms: search.first_success_ms,
            ..WorkflowSearchSignal::default()
        }),
        links: None,
        sort_ms: search.timestamp_ms,
    }
}

fn followup_step(
    search: &SearchTelemetryRecord,
    followup: &SearchFollowupTelemetry,
    trace_by_request: &HashMap<String, DispatchTrace>,
    links: &AdminLinkBuilder,
) -> WorkflowStep {
    let selected_hit = selected_hit(search, followup);
    let signal = WorkflowSearchSignal {
        search_id: search.search_id.clone(),
        selected_rank: followup.selected_rank,
        selected_score: selected_hit.map(|hit| hit.score),
        match_reasons: selected_hit
            .map(|hit| hit.match_reasons.clone())
            .unwrap_or_default(),
        zero_results: Some(search.zero_results),
        result_count: Some(search.total),
        first_success_ms: search.first_success_ms,
    };
    if let Some(trace) = followup
        .request_id
        .as_deref()
        .and_then(|request_id| trace_by_request.get(request_id))
    {
        return trace_step(trace, &followup.kind, Some(signal), links);
    }

    let request_id = followup.request_id.clone();
    WorkflowStep {
        step_id: format!(
            "{}:{}",
            followup.kind,
            request_id
                .clone()
                .unwrap_or_else(|| format!("{}:{}", search.search_id, followup.timestamp_ms))
        ),
        kind: followup.kind.clone(),
        title: operation_title(
            &followup.kind,
            followup.tool_slug.as_deref(),
            followup.skill_name.as_deref(),
        ),
        timestamp: rfc3339_ms(followup.timestamp_ms),
        status: if followup.success { "ok" } else { "err" }.to_string(),
        success: Some(followup.success),
        request_id: request_id.clone(),
        trace_id: followup
            .trace_id
            .clone()
            .or_else(|| search.trace_id.clone()),
        parent_request_id: None,
        session_id: search.session_id.clone(),
        dcc_type: search.dcc_type.clone(),
        instance_id: search.instance_id.clone(),
        tool: followup
            .tool_slug
            .clone()
            .or_else(|| followup.skill_name.clone()),
        transport: Some(search.transport.clone()),
        duration_ms: followup.elapsed_ms,
        search: Some(signal),
        links: request_id.map(|request_id| links.request_links(&request_id)),
        sort_ms: followup.timestamp_ms,
    }
}

fn trace_step(
    trace: &DispatchTrace,
    kind: &str,
    search: Option<WorkflowSearchSignal>,
    links: &AdminLinkBuilder,
) -> WorkflowStep {
    let sort_ms = timestamp_ms(trace.started_at);
    WorkflowStep {
        step_id: format!("{kind}:{}", trace.request_id),
        kind: kind.to_string(),
        title: trace
            .tool_slug
            .clone()
            .unwrap_or_else(|| trace.method.clone()),
        timestamp: rfc3339_ms(sort_ms),
        status: if trace.ok { "ok" } else { "err" }.to_string(),
        success: Some(trace.ok),
        request_id: Some(trace.request_id.clone()),
        trace_id: Some(trace.trace_id.clone()),
        parent_request_id: trace.parent_request_id.clone(),
        session_id: trace.session_id.clone(),
        dcc_type: trace.dcc_type.clone(),
        instance_id: trace.instance_id.clone(),
        tool: trace.tool_slug.clone(),
        transport: trace.transport.clone(),
        duration_ms: Some(trace.total_ms),
        search,
        links: Some(links.request_links(&trace.request_id)),
        sort_ms,
    }
}

fn audit_step(audit: &AdminAuditRecord, links: &AdminLinkBuilder) -> WorkflowStep {
    let sort_ms = timestamp_ms(audit.timestamp);
    WorkflowStep {
        step_id: format!("audit:{}", audit.request_id),
        kind: "call".to_string(),
        title: audit.action.clone(),
        timestamp: rfc3339_ms(sort_ms),
        status: if audit.success { "ok" } else { "err" }.to_string(),
        success: Some(audit.success),
        request_id: Some(audit.request_id.clone()),
        trace_id: audit.trace_id.clone(),
        parent_request_id: audit.parent_request_id.clone(),
        session_id: audit.session_id.clone(),
        dcc_type: audit.dcc_type.clone(),
        instance_id: audit.instance_id.clone(),
        tool: Some(audit.action.clone()),
        transport: audit.transport.clone(),
        duration_ms: audit.duration_ms,
        search: None,
        links: Some(links.request_links(&audit.request_id)),
        sort_ms,
    }
}

fn selected_hit<'a>(
    search: &'a SearchTelemetryRecord,
    followup: &SearchFollowupTelemetry,
) -> Option<&'a SearchTelemetryHit> {
    if let Some(tool_slug) = followup.tool_slug.as_deref()
        && let Some(hit) = search.hits.iter().find(|hit| hit.tool_slug == tool_slug)
    {
        return Some(hit);
    }
    let skill_name = followup.skill_name.as_deref()?;
    search.hits.iter().find(|hit| {
        hit.skill_name
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(skill_name))
    })
}

fn operation_title(kind: &str, tool_slug: Option<&str>, skill_name: Option<&str>) -> String {
    match (tool_slug, skill_name) {
        (Some(tool), _) => tool.to_string(),
        (None, Some(skill)) => format!("{kind} {skill}"),
        (None, None) => kind.to_string(),
    }
}

fn workflow_title(builder: &WorkflowBuilder) -> String {
    let agent = builder
        .agent
        .as_ref()
        .and_then(|agent| {
            agent
                .agent_name
                .as_deref()
                .or(agent.agent_id.as_deref())
                .or(agent.agent_kind.as_deref())
                .or(agent.model.as_deref())
        })
        .map(str::to_string);
    let first_tool = builder
        .steps
        .iter()
        .find_map(|step| step.tool.as_deref().or(Some(step.title.as_str())))
        .map(str::to_string)
        .unwrap_or_else(|| builder.workflow_id.clone());
    match agent {
        Some(agent) => format!("{agent}: {first_tool}"),
        None => first_tool,
    }
}

fn discovery_summary(steps: &[WorkflowStep]) -> WorkflowDiscoverySummary {
    let mut search_ids = BTreeSet::new();
    let mut zero_result_count = 0usize;
    let mut selected_ranks = Vec::new();
    let mut first_success_values = Vec::new();
    for step in steps {
        let Some(search) = step.search.as_ref() else {
            continue;
        };
        search_ids.insert(search.search_id.clone());
        if search.zero_results == Some(true) && step.kind == "search" {
            zero_result_count += 1;
        }
        if let Some(rank) = search.selected_rank {
            selected_ranks.push(rank);
        }
        if let Some(ms) = search.first_success_ms {
            first_success_values.push(ms);
        }
    }
    WorkflowDiscoverySummary {
        search_count: search_ids.len(),
        zero_result_count,
        selected_count: selected_ranks.len(),
        best_selected_rank: selected_ranks.into_iter().min(),
        time_to_first_success_ms: first_success_values.into_iter().min(),
        search_ids: search_ids.into_iter().take(MAX_WORKFLOW_IDS).collect(),
    }
}

fn agent_from_context(ctx: &AgentContext) -> WorkflowAgent {
    WorkflowAgent {
        agent_id: ctx.agent_id.clone(),
        agent_name: ctx.agent_name.clone(),
        agent_kind: ctx.agent_kind.clone(),
        model: ctx.model.clone(),
        task: ctx.task.clone(),
        turn_index: ctx.turn_index,
        tags: ctx.tags.iter().take(MAX_AGENT_TAGS).cloned().collect(),
    }
}

fn agent_from_audit(audit: &AdminAuditRecord) -> WorkflowAgent {
    WorkflowAgent {
        agent_id: audit.agent_id.clone(),
        agent_name: audit.agent_name.clone(),
        model: audit.agent_model.clone(),
        ..WorkflowAgent::default()
    }
}

fn workflow_id_from_context(ctx: &AgentContext) -> Option<String> {
    let metadata = ctx.metadata.as_object()?;
    for key in [
        "workflow_id",
        "workflowId",
        "session_workflow_id",
        "task_id",
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

fn limit_set(values: BTreeSet<String>) -> Vec<String> {
    values.into_iter().take(MAX_WORKFLOW_IDS).collect()
}

fn timestamp_ms(time: std::time::SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

fn rfc3339_ms(ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from(UNIX_EPOCH + Duration::from_millis(ms))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
