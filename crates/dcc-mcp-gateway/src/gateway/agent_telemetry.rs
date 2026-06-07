//! Agent workflow telemetry spans for gateway discovery and execution.
//!
//! This module intentionally emits bounded semantic attributes only. It links
//! gateway-local Admin trace IDs with OTLP/OpenInference-style spans without
//! exporting raw prompts, request bodies, hidden reasoning, or secrets.

#[cfg(feature = "telemetry")]
use opentelemetry::trace::{Span as _, Status, Tracer as _};
#[cfg(feature = "telemetry")]
use opentelemetry::{KeyValue, global};
use serde_json::Value;
#[cfg(test)]
use serde_json::{Map, json};

use crate::gateway::admin::trace::{AgentContext, TraceContext};
use crate::gateway::capability::parse_slug;
use crate::gateway::search_telemetry::{
    RANKER_VERSION, SearchTelemetryHit, SearchTelemetryStore, search_id_from_meta,
    search_id_from_payload,
};

const MATCH_REASON_LIMIT: usize = 5;

#[derive(Debug, Clone)]
pub(crate) struct AgentWorkflowEvent {
    operation: &'static str,
    transport: String,
    trace_id: Option<String>,
    request_id: Option<String>,
    parent_request_id: Option<String>,
    session_id: Option<String>,
    actor_id: Option<String>,
    actor_name: Option<String>,
    actor_email_hash: Option<String>,
    agent_id: Option<String>,
    agent_name: Option<String>,
    agent_kind: Option<String>,
    agent_version: Option<String>,
    agent_model_provider: Option<String>,
    agent_model_version: Option<String>,
    agent_model: Option<String>,
    agent_reasoning_effort: Option<String>,
    agent_turn_id: Option<String>,
    agent_user_intent_summary: Option<String>,
    agent_reply_summary: Option<String>,
    agent_user_input_hash: Option<String>,
    agent_reply_hash: Option<String>,
    agent_user_input_chars: Option<u64>,
    agent_reply_chars: Option<u64>,
    agent_task: Option<String>,
    agent_tags: Vec<String>,
    client_platform: Option<String>,
    client_os: Option<String>,
    client_host: Option<String>,
    auth_subject: Option<String>,
    source_ip: Option<String>,
    forwarded_for: Vec<String>,
    dcc_type: Option<String>,
    instance_id: Option<String>,
    skill_name: Option<String>,
    tool_slug: Option<String>,
    search_id: Option<String>,
    ranker_version: Option<String>,
    selected_rank: Option<u32>,
    score: Option<u32>,
    match_reasons: Vec<String>,
    policy_outcome: Option<String>,
    policy_reason: Option<String>,
    success: Option<bool>,
    error_kind: Option<String>,
    total: Option<usize>,
    zero_results: Option<bool>,
    batch_size: Option<usize>,
}

impl AgentWorkflowEvent {
    pub(crate) fn new(operation: &'static str, transport: impl Into<String>) -> Self {
        Self {
            operation,
            transport: transport.into(),
            trace_id: None,
            request_id: None,
            parent_request_id: None,
            session_id: None,
            actor_id: None,
            actor_name: None,
            actor_email_hash: None,
            agent_id: None,
            agent_name: None,
            agent_kind: None,
            agent_version: None,
            agent_model_provider: None,
            agent_model_version: None,
            agent_model: None,
            agent_reasoning_effort: None,
            agent_turn_id: None,
            agent_user_intent_summary: None,
            agent_reply_summary: None,
            agent_user_input_hash: None,
            agent_reply_hash: None,
            agent_user_input_chars: None,
            agent_reply_chars: None,
            agent_task: None,
            agent_tags: Vec::new(),
            client_platform: None,
            client_os: None,
            client_host: None,
            auth_subject: None,
            source_ip: None,
            forwarded_for: Vec::new(),
            dcc_type: None,
            instance_id: None,
            skill_name: None,
            tool_slug: None,
            search_id: None,
            ranker_version: None,
            selected_rank: None,
            score: None,
            match_reasons: Vec::new(),
            policy_outcome: None,
            policy_reason: None,
            success: None,
            error_kind: None,
            total: None,
            zero_results: None,
            batch_size: None,
        }
    }

    pub(crate) fn with_trace_context(mut self, trace_context: Option<&TraceContext>) -> Self {
        if let Some(trace_context) = trace_context {
            self.trace_id = Some(trace_context.trace_id.clone());
            self.request_id = Some(trace_context.request_id.clone());
            self.parent_request_id = trace_context.parent_request_id.clone();
        }
        self
    }

    pub(crate) fn with_agent_context(mut self, agent_context: Option<&AgentContext>) -> Self {
        if let Some(ctx) = agent_context {
            self.actor_id = ctx.actor_id.clone();
            self.actor_name = ctx.actor_name.clone();
            self.actor_email_hash = ctx.actor_email_hash.clone();
            self.agent_id = ctx.agent_id.clone();
            self.agent_name = ctx.agent_name.clone();
            self.agent_kind = ctx.agent_kind.clone();
            self.agent_version = ctx.agent_version.clone();
            self.agent_model_provider = ctx.model_provider.clone();
            self.agent_model_version = ctx.model_version.clone();
            self.agent_model = ctx.model.clone();
            self.agent_reasoning_effort = ctx.reasoning_effort.clone();
            self.agent_turn_id = ctx.turn_id.clone();
            self.agent_user_intent_summary = ctx.user_intent_summary.clone();
            self.agent_reply_summary = ctx.agent_reply_summary.clone();
            self.agent_user_input_hash = ctx.user_input_hash.clone();
            self.agent_reply_hash = ctx.agent_reply_hash.clone();
            self.agent_user_input_chars = ctx.user_input_chars;
            self.agent_reply_chars = ctx.agent_reply_chars;
            self.agent_task = ctx.task.clone();
            self.agent_tags = ctx.tags.iter().take(16).cloned().collect();
            self.client_platform = ctx.client_platform.clone();
            self.client_os = ctx.client_os.clone();
            self.client_host = ctx.client_host.clone();
            self.auth_subject = ctx.auth_subject.clone();
            self.source_ip = ctx.source_ip.clone();
            self.forwarded_for = ctx.forwarded_for.iter().take(16).cloned().collect();
            if self.session_id.is_none() {
                self.session_id = ctx.session_id.clone();
            }
            if self.parent_request_id.is_none() {
                self.parent_request_id = ctx.parent_request_id.clone();
            }
            if self.trace_id.is_none() {
                self.trace_id = ctx.trace_id.clone();
            }
        }
        self
    }

    pub(crate) fn with_session_id(mut self, session_id: Option<&str>) -> Self {
        if let Some(session_id) = session_id.filter(|value| !value.is_empty()) {
            self.session_id = Some(session_id.to_string());
        }
        self
    }

    pub(crate) fn with_route(
        mut self,
        tool_slug: Option<&str>,
        skill_name: Option<&str>,
        dcc_type: Option<&str>,
        instance_id: Option<&str>,
    ) -> Self {
        if let Some(slug) = tool_slug.filter(|value| !value.is_empty()) {
            self.tool_slug = Some(slug.to_string());
            if let Some((parsed_dcc, parsed_instance, _)) = parse_slug(slug) {
                if self.dcc_type.is_none() {
                    self.dcc_type = Some(parsed_dcc.to_string());
                }
                if self.instance_id.is_none() {
                    self.instance_id = Some(parsed_instance.to_string());
                }
            }
        }
        if self.skill_name.is_none() {
            self.skill_name = skill_name
                .filter(|value| !value.is_empty())
                .map(str::to_string);
        }
        if self.dcc_type.is_none() {
            self.dcc_type = dcc_type
                .filter(|value| !value.is_empty())
                .map(str::to_string);
        }
        if self.instance_id.is_none() {
            self.instance_id = instance_id
                .filter(|value| !value.is_empty())
                .map(str::to_string);
        }
        self
    }

    pub(crate) fn with_search_id(mut self, search_id: Option<&str>) -> Self {
        self.search_id = search_id
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self
    }

    pub(crate) fn with_ranker_version(mut self, ranker_version: Option<&str>) -> Self {
        self.ranker_version = ranker_version
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| Some(RANKER_VERSION.to_string()));
        self
    }

    pub(crate) fn with_search_result(mut self, value: &Value) -> Self {
        if self.search_id.is_none() {
            self.search_id = value
                .get("search_id")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if self.ranker_version.is_none() {
            self.ranker_version = value
                .get("ranker_version")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        let total = value
            .get("total")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .or_else(|| {
                value
                    .get("hits")
                    .and_then(Value::as_array)
                    .map(|items| items.len())
            });
        self.total = total;
        self.zero_results = total.map(|value| value == 0);
        self
    }

    pub(crate) fn with_selected_hit(mut self, hit: Option<&SearchTelemetryHit>) -> Self {
        if let Some(hit) = hit {
            if self.tool_slug.is_none() && !hit.tool_slug.is_empty() {
                self.tool_slug = Some(hit.tool_slug.clone());
            }
            if self.skill_name.is_none() {
                self.skill_name = hit.skill_name.clone();
            }
            if self.dcc_type.is_none() && !hit.dcc_type.is_empty() {
                self.dcc_type = Some(hit.dcc_type.clone());
            }
            self.selected_rank = Some(hit.rank);
            self.score = Some(hit.score);
            self.match_reasons = hit
                .match_reasons
                .iter()
                .take(MATCH_REASON_LIMIT)
                .cloned()
                .collect();
        }
        self
    }

    pub(crate) fn with_batch_size(mut self, batch_size: Option<usize>) -> Self {
        self.batch_size = batch_size;
        self
    }

    pub(crate) fn with_outcome(mut self, success: bool, error_kind: Option<&str>) -> Self {
        self.success = Some(success);
        self.error_kind = error_kind
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self.policy_outcome = Some(policy_outcome(error_kind).to_string());
        self
    }

    pub(crate) fn with_policy_reason(mut self, policy_reason: Option<&str>) -> Self {
        self.policy_reason = policy_reason
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self
    }

    #[cfg(test)]
    pub(crate) fn attributes(&self) -> Map<String, Value> {
        let mut attrs = Map::new();
        insert_str(
            &mut attrs,
            "openinference.span.kind",
            self.openinference_kind(),
        );
        insert_str(&mut attrs, "dcc_mcp.workflow.operation", self.operation);
        insert_str(&mut attrs, "dcc_mcp.transport", &self.transport);
        insert_opt(&mut attrs, "dcc_mcp.trace_id", self.trace_id.as_deref());
        insert_opt(&mut attrs, "dcc_mcp.request_id", self.request_id.as_deref());
        insert_opt(
            &mut attrs,
            "dcc_mcp.parent_request_id",
            self.parent_request_id.as_deref(),
        );
        insert_opt(&mut attrs, "dcc_mcp.session_id", self.session_id.as_deref());
        insert_opt(&mut attrs, "dcc_mcp.actor.id", self.actor_id.as_deref());
        insert_opt(&mut attrs, "dcc_mcp.actor.name", self.actor_name.as_deref());
        insert_opt(
            &mut attrs,
            "dcc_mcp.actor.email_hash",
            self.actor_email_hash.as_deref(),
        );
        insert_opt(&mut attrs, "dcc_mcp.agent.id", self.agent_id.as_deref());
        insert_opt(&mut attrs, "dcc_mcp.agent.name", self.agent_name.as_deref());
        insert_opt(&mut attrs, "dcc_mcp.agent.kind", self.agent_kind.as_deref());
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.version",
            self.agent_version.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.model_provider",
            self.agent_model_provider.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.model_version",
            self.agent_model_version.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.model",
            self.agent_model.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.reasoning_effort",
            self.agent_reasoning_effort.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.turn_id",
            self.agent_turn_id.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.user_intent_summary",
            self.agent_user_intent_summary.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.reply_summary",
            self.agent_reply_summary.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.user_input_hash",
            self.agent_user_input_hash.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.agent.reply_hash",
            self.agent_reply_hash.as_deref(),
        );
        insert_u64(
            &mut attrs,
            "dcc_mcp.agent.user_input_chars",
            self.agent_user_input_chars,
        );
        insert_u64(
            &mut attrs,
            "dcc_mcp.agent.reply_chars",
            self.agent_reply_chars,
        );
        insert_opt(&mut attrs, "dcc_mcp.agent.task", self.agent_task.as_deref());
        if !self.agent_tags.is_empty() {
            attrs.insert("dcc_mcp.agent.tags".to_string(), json!(self.agent_tags));
        }
        insert_opt(
            &mut attrs,
            "dcc_mcp.client.platform",
            self.client_platform.as_deref(),
        );
        insert_opt(&mut attrs, "dcc_mcp.client.os", self.client_os.as_deref());
        insert_opt(
            &mut attrs,
            "dcc_mcp.client.host",
            self.client_host.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.auth.subject",
            self.auth_subject.as_deref(),
        );
        insert_opt(&mut attrs, "dcc_mcp.source.ip", self.source_ip.as_deref());
        if !self.forwarded_for.is_empty() {
            attrs.insert(
                "dcc_mcp.forwarded_for".to_string(),
                json!(self.forwarded_for),
            );
        }
        insert_opt(&mut attrs, "dcc_mcp.dcc.type", self.dcc_type.as_deref());
        insert_opt(
            &mut attrs,
            "dcc_mcp.instance.id",
            self.instance_id.as_deref(),
        );
        insert_opt(&mut attrs, "dcc_mcp.skill.name", self.skill_name.as_deref());
        insert_opt(&mut attrs, "dcc_mcp.tool.slug", self.tool_slug.as_deref());
        insert_opt(&mut attrs, "dcc_mcp.search.id", self.search_id.as_deref());
        insert_opt(
            &mut attrs,
            "dcc_mcp.search.ranker_version",
            self.ranker_version.as_deref(),
        );
        insert_num(
            &mut attrs,
            "dcc_mcp.search.selected_rank",
            self.selected_rank,
        );
        insert_num(&mut attrs, "dcc_mcp.search.score", self.score);
        if !self.match_reasons.is_empty() {
            attrs.insert(
                "dcc_mcp.search.match_reasons".to_string(),
                json!(self.match_reasons),
            );
        }
        insert_opt(
            &mut attrs,
            "dcc_mcp.policy.outcome",
            self.policy_outcome.as_deref(),
        );
        insert_opt(
            &mut attrs,
            "dcc_mcp.policy.reason",
            self.policy_reason.as_deref(),
        );
        if let Some(success) = self.success {
            attrs.insert("dcc_mcp.success".to_string(), json!(success));
        }
        insert_opt(&mut attrs, "dcc_mcp.error.kind", self.error_kind.as_deref());
        if let Some(total) = self.total {
            attrs.insert("dcc_mcp.search.total".to_string(), json!(total));
        }
        if let Some(zero_results) = self.zero_results {
            attrs.insert(
                "dcc_mcp.search.zero_results".to_string(),
                json!(zero_results),
            );
        }
        if let Some(batch_size) = self.batch_size {
            attrs.insert("dcc_mcp.batch.size".to_string(), json!(batch_size));
        }
        attrs
    }

    pub(crate) fn emit(&self) {
        let match_reasons = self.match_reasons.join(",");
        let agent_tags = self.agent_tags.join(",");
        let selected_rank = self.selected_rank.unwrap_or_default() as u64;
        let score = self.score.unwrap_or_default() as u64;
        let total = self.total.unwrap_or_default() as u64;
        let batch_size = self.batch_size.unwrap_or_default() as u64;
        let user_input_chars = self.agent_user_input_chars.unwrap_or_default();
        let reply_chars = self.agent_reply_chars.unwrap_or_default();
        let success = self.success.unwrap_or(false);
        let zero_results = self.zero_results.unwrap_or(false);

        macro_rules! emit_span {
            ($name:literal) => {{
                let span = tracing::info_span!(
                    $name,
                    "openinference.span.kind" = %self.openinference_kind(),
                    "dcc_mcp.workflow.operation" = self.operation,
                    "dcc_mcp.transport" = %self.transport,
                    "dcc_mcp.trace_id" = self.trace_id.as_deref().unwrap_or(""),
                    "dcc_mcp.request_id" = self.request_id.as_deref().unwrap_or(""),
                    "dcc_mcp.parent_request_id" = self.parent_request_id.as_deref().unwrap_or(""),
                    "dcc_mcp.session_id" = self.session_id.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.id" = self.agent_id.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.name" = self.agent_name.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.kind" = self.agent_kind.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.model_provider" =
                        self.agent_model_provider.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.model_version" =
                        self.agent_model_version.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.model" = self.agent_model.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.reasoning_effort" =
                        self.agent_reasoning_effort.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.turn_id" = self.agent_turn_id.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.user_intent_summary" =
                        self.agent_user_intent_summary.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.reply_summary" =
                        self.agent_reply_summary.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.user_input_hash" =
                        self.agent_user_input_hash.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.reply_hash" = self.agent_reply_hash.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.user_input_chars" = user_input_chars,
                    "dcc_mcp.agent.reply_chars" = reply_chars,
                    "dcc_mcp.agent.task" = self.agent_task.as_deref().unwrap_or(""),
                    "dcc_mcp.agent.tags" = %agent_tags,
                    "dcc_mcp.dcc.type" = self.dcc_type.as_deref().unwrap_or(""),
                    "dcc_mcp.instance.id" = self.instance_id.as_deref().unwrap_or(""),
                    "dcc_mcp.skill.name" = self.skill_name.as_deref().unwrap_or(""),
                    "dcc_mcp.tool.slug" = self.tool_slug.as_deref().unwrap_or(""),
                    "dcc_mcp.search.id" = self.search_id.as_deref().unwrap_or(""),
                    "dcc_mcp.search.ranker_version" =
                        self.ranker_version.as_deref().unwrap_or(RANKER_VERSION),
                    "dcc_mcp.search.selected_rank" = selected_rank,
                    "dcc_mcp.search.score" = score,
                    "dcc_mcp.search.match_reasons" = %match_reasons,
                    "dcc_mcp.policy.outcome" = self.policy_outcome.as_deref().unwrap_or(""),
                    "dcc_mcp.policy.reason" = self.policy_reason.as_deref().unwrap_or(""),
                    "dcc_mcp.success" = success,
                    "dcc_mcp.error.kind" = self.error_kind.as_deref().unwrap_or(""),
                    "dcc_mcp.search.total" = total,
                    "dcc_mcp.search.zero_results" = zero_results,
                    "dcc_mcp.batch.size" = batch_size,
                );
                span.in_scope(|| {
                    tracing::info!("gateway agent workflow telemetry");
                });
                #[cfg(feature = "telemetry")]
                if dcc_mcp_telemetry::provider::direct_span_fallback_enabled() {
                    self.emit_otel_span($name);
                }
            }};
        }

        match self.operation {
            "gateway.search" => emit_span!("gateway.search"),
            "gateway.describe" => emit_span!("gateway.describe"),
            "gateway.load_skill" => emit_span!("gateway.load_skill"),
            "gateway.call" => emit_span!("gateway.call"),
            "gateway.call_batch" => emit_span!("gateway.call_batch"),
            _ => emit_span!("gateway.workflow"),
        }
    }

    fn openinference_kind(&self) -> &'static str {
        match self.operation {
            "gateway.call" | "gateway.call_batch" | "gateway.describe" | "gateway.load_skill" => {
                "TOOL"
            }
            _ => "CHAIN",
        }
    }

    #[cfg(feature = "telemetry")]
    fn emit_otel_span(&self, name: &'static str) {
        let tracer = global::tracer("dcc-mcp-gateway");
        let mut span = tracer.start(name);
        span.set_attributes(self.otel_attributes());
        match self.success {
            Some(true) => span.set_status(Status::Ok),
            Some(false) => {
                let description = self
                    .error_kind
                    .as_deref()
                    .unwrap_or("gateway workflow failed")
                    .to_string();
                span.set_status(Status::error(description));
            }
            None => {}
        }
        span.end();
    }

    #[cfg(feature = "telemetry")]
    fn otel_attributes(&self) -> Vec<KeyValue> {
        let mut attrs = Vec::new();
        push_attr(
            &mut attrs,
            "openinference.span.kind",
            self.openinference_kind(),
        );
        push_attr(&mut attrs, "dcc_mcp.workflow.operation", self.operation);
        push_attr(&mut attrs, "dcc_mcp.transport", &self.transport);
        push_opt_attr(&mut attrs, "dcc_mcp.trace_id", self.trace_id.as_deref());
        push_opt_attr(&mut attrs, "dcc_mcp.request_id", self.request_id.as_deref());
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.parent_request_id",
            self.parent_request_id.as_deref(),
        );
        push_opt_attr(&mut attrs, "dcc_mcp.session_id", self.session_id.as_deref());
        push_opt_attr(&mut attrs, "dcc_mcp.agent.id", self.agent_id.as_deref());
        push_opt_attr(&mut attrs, "dcc_mcp.agent.name", self.agent_name.as_deref());
        push_opt_attr(&mut attrs, "dcc_mcp.agent.kind", self.agent_kind.as_deref());
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.agent.model_provider",
            self.agent_model_provider.as_deref(),
        );
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.agent.model_version",
            self.agent_model_version.as_deref(),
        );
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.agent.model",
            self.agent_model.as_deref(),
        );
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.agent.reasoning_effort",
            self.agent_reasoning_effort.as_deref(),
        );
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.agent.turn_id",
            self.agent_turn_id.as_deref(),
        );
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.agent.user_intent_summary",
            self.agent_user_intent_summary.as_deref(),
        );
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.agent.reply_summary",
            self.agent_reply_summary.as_deref(),
        );
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.agent.user_input_hash",
            self.agent_user_input_hash.as_deref(),
        );
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.agent.reply_hash",
            self.agent_reply_hash.as_deref(),
        );
        push_num_attr(
            &mut attrs,
            "dcc_mcp.agent.user_input_chars",
            self.agent_user_input_chars.map(|value| value as i64),
        );
        push_num_attr(
            &mut attrs,
            "dcc_mcp.agent.reply_chars",
            self.agent_reply_chars.map(|value| value as i64),
        );
        push_opt_attr(&mut attrs, "dcc_mcp.agent.task", self.agent_task.as_deref());
        if !self.agent_tags.is_empty() {
            attrs.push(KeyValue::new(
                "dcc_mcp.agent.tags",
                self.agent_tags.join(","),
            ));
        }
        push_opt_attr(&mut attrs, "dcc_mcp.dcc.type", self.dcc_type.as_deref());
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.instance.id",
            self.instance_id.as_deref(),
        );
        push_opt_attr(&mut attrs, "dcc_mcp.skill.name", self.skill_name.as_deref());
        push_opt_attr(&mut attrs, "dcc_mcp.tool.slug", self.tool_slug.as_deref());
        push_opt_attr(&mut attrs, "dcc_mcp.search.id", self.search_id.as_deref());
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.search.ranker_version",
            self.ranker_version.as_deref(),
        );
        push_num_attr(
            &mut attrs,
            "dcc_mcp.search.selected_rank",
            self.selected_rank.map(i64::from),
        );
        push_num_attr(
            &mut attrs,
            "dcc_mcp.search.score",
            self.score.map(i64::from),
        );
        if !self.match_reasons.is_empty() {
            attrs.push(KeyValue::new(
                "dcc_mcp.search.match_reasons",
                self.match_reasons.join(","),
            ));
        }
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.policy.outcome",
            self.policy_outcome.as_deref(),
        );
        push_opt_attr(
            &mut attrs,
            "dcc_mcp.policy.reason",
            self.policy_reason.as_deref(),
        );
        if let Some(success) = self.success {
            attrs.push(KeyValue::new("dcc_mcp.success", success));
        }
        push_opt_attr(&mut attrs, "dcc_mcp.error.kind", self.error_kind.as_deref());
        push_num_attr(
            &mut attrs,
            "dcc_mcp.search.total",
            self.total.map(|value| value as i64),
        );
        if let Some(zero_results) = self.zero_results {
            attrs.push(KeyValue::new("dcc_mcp.search.zero_results", zero_results));
        }
        push_num_attr(
            &mut attrs,
            "dcc_mcp.batch.size",
            self.batch_size.map(|value| value as i64),
        );
        attrs
    }
}

pub(crate) struct McpToolTelemetryInput<'a> {
    pub(crate) search_telemetry: &'a SearchTelemetryStore,
    pub(crate) tool: &'a str,
    pub(crate) args: &'a Value,
    pub(crate) meta: Option<&'a Value>,
    pub(crate) trace_context: Option<&'a TraceContext>,
    pub(crate) agent_context: Option<&'a AgentContext>,
    pub(crate) session_id: Option<&'a str>,
    pub(crate) text: &'a str,
    pub(crate) is_error: bool,
}

pub(crate) fn emit_mcp_tool_event(input: McpToolTelemetryInput<'_>) {
    if let Some(event) = build_mcp_tool_event(input) {
        event.emit();
    }
}

pub(crate) fn build_mcp_tool_event(input: McpToolTelemetryInput<'_>) -> Option<AgentWorkflowEvent> {
    let McpToolTelemetryInput {
        search_telemetry,
        tool,
        args,
        meta,
        trace_context,
        agent_context,
        session_id,
        text,
        is_error,
    } = input;
    let operation = operation_for_mcp_tool(tool, args)?;
    let result_value = serde_json::from_str::<Value>(text).ok();
    let error_kind = is_error
        .then(|| {
            result_value
                .as_ref()
                .and_then(error_kind_from_value)
                .or_else(|| error_kind_from_text(text))
        })
        .flatten();
    let policy_reason = result_value.as_ref().and_then(policy_reason_from_value);

    let mut event = AgentWorkflowEvent::new(operation, "mcp")
        .with_trace_context(trace_context)
        .with_agent_context(agent_context)
        .with_session_id(session_id)
        .with_outcome(!is_error, error_kind.as_deref())
        .with_policy_reason(policy_reason.as_deref());

    if operation == "gateway.search" {
        if let Some(value) = result_value.as_ref() {
            event = event.with_search_result(value);
        }
        let dcc_type = args
            .get("dcc_type")
            .or_else(|| args.get("dcc"))
            .and_then(Value::as_str);
        let instance_id = args.get("instance_id").and_then(Value::as_str);
        return Some(event.with_route(None, None, dcc_type, instance_id));
    }

    let search_id = search_id_from_payload(args).or_else(|| meta.and_then(search_id_from_meta));
    let (tool_slug, batch_size) = route_from_args(args);
    let skill_name = skill_name_from_payload(args);
    let selected_hit = search_id.as_deref().and_then(|search_id| {
        search_telemetry.selected_hit(search_id, tool_slug, skill_name.as_deref())
    });

    Some(
        event
            .with_search_id(search_id.as_deref())
            .with_ranker_version(Some(RANKER_VERSION))
            .with_route(
                tool_slug,
                skill_name.as_deref(),
                dcc_type_from_args(args),
                None,
            )
            .with_selected_hit(selected_hit.as_ref())
            .with_batch_size(batch_size),
    )
}

pub(crate) fn error_kind_from_text(text: &str) -> Option<String> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| error_kind_from_value(&value))
}

pub(crate) fn error_kind_from_value(value: &Value) -> Option<String> {
    value
        .pointer("/error/kind")
        .or_else(|| value.pointer("/error/error/kind"))
        .or_else(|| value.pointer("/result/error/kind"))
        .or_else(|| value.pointer("/output/error/kind"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn policy_reason_from_value(value: &Value) -> Option<String> {
    value
        .pointer("/error/policy/reason")
        .or_else(|| value.pointer("/error/error/policy/reason"))
        .or_else(|| value.pointer("/result/error/policy/reason"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn operation_for_mcp_tool(tool: &str, args: &Value) -> Option<&'static str> {
    match tool {
        "search" | "search_tools" | "search_skills" | "list_skills" => Some("gateway.search"),
        "describe" | "describe_tool" | "get_skill_info" => Some("gateway.describe"),
        "load_skill" => Some("gateway.load_skill"),
        "call" if args.get("calls").and_then(Value::as_array).is_some() => {
            Some("gateway.call_batch")
        }
        "call" | "call_tool" => Some("gateway.call"),
        "call_tools" => Some("gateway.call_batch"),
        _ => None,
    }
}

fn route_from_args(args: &Value) -> (Option<&str>, Option<usize>) {
    if let Some(calls) = args.get("calls").and_then(Value::as_array) {
        let slug = calls
            .first()
            .and_then(|call| call.get("tool_slug"))
            .and_then(Value::as_str);
        return (slug, Some(calls.len()));
    }
    (args.get("tool_slug").and_then(Value::as_str), None)
}

fn skill_name_from_payload(payload: &Value) -> Option<String> {
    payload
        .get("skill_name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            payload
                .get("skill_names")
                .and_then(Value::as_array)
                .and_then(|items| items.iter().find_map(Value::as_str))
                .map(str::to_string)
        })
}

fn dcc_type_from_args(args: &Value) -> Option<&str> {
    args.get("dcc_type")
        .or_else(|| args.get("dcc"))
        .and_then(Value::as_str)
}

fn policy_outcome(error_kind: Option<&str>) -> &'static str {
    match error_kind {
        Some("policy-denied") => "denied",
        Some("throttled") => "throttled",
        _ => "allowed",
    }
}

#[cfg(feature = "telemetry")]
fn push_attr(attrs: &mut Vec<KeyValue>, key: &'static str, value: &str) {
    attrs.push(KeyValue::new(key, value.to_string()));
}

#[cfg(feature = "telemetry")]
fn push_opt_attr(attrs: &mut Vec<KeyValue>, key: &'static str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        push_attr(attrs, key, value);
    }
}

#[cfg(feature = "telemetry")]
fn push_num_attr(attrs: &mut Vec<KeyValue>, key: &'static str, value: Option<i64>) {
    if let Some(value) = value {
        attrs.push(KeyValue::new(key, value));
    }
}

#[cfg(test)]
fn insert_str(attrs: &mut Map<String, Value>, key: &str, value: &str) {
    attrs.insert(key.to_string(), json!(value));
}

#[cfg(test)]
fn insert_opt(attrs: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        attrs.insert(key.to_string(), json!(value));
    }
}

#[cfg(test)]
fn insert_num(attrs: &mut Map<String, Value>, key: &str, value: Option<u32>) {
    if let Some(value) = value {
        attrs.insert(key.to_string(), json!(value));
    }
}

#[cfg(test)]
fn insert_u64(attrs: &mut Map<String, Value>, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        attrs.insert(key.to_string(), json!(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace_context() -> TraceContext {
        TraceContext {
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_string(),
            request_id: "req-1180".to_string(),
            span_id: Some("00f067aa0ba902b7".to_string()),
            parent_span_id: None,
            parent_request_id: Some("parent-1".to_string()),
            trace_flags: Some("01".to_string()),
            trace_state: None,
        }
    }

    #[test]
    fn attributes_include_bounded_agent_and_search_fields_without_reasoning_payloads() {
        let event = AgentWorkflowEvent::new("gateway.search", "rest")
            .with_trace_context(Some(&trace_context()))
            .with_agent_context(Some(&AgentContext {
                actor_id: Some("user-1".to_string()),
                actor_name: Some("Morgan Artist".to_string()),
                actor_email_hash: Some("sha256:actor".to_string()),
                agent_id: Some("agent-1".to_string()),
                agent_name: Some("Codex".to_string()),
                agent_kind: Some("coding-agent".to_string()),
                agent_version: Some("0.9.0".to_string()),
                model_provider: Some("openai".to_string()),
                model_version: Some("gpt-5.1".to_string()),
                model: Some("gpt-5".to_string()),
                reasoning_effort: Some("high".to_string()),
                session_id: Some("session-1".to_string()),
                turn_id: Some("turn-1".to_string()),
                user_intent_summary: Some("Fix gateway telemetry.".to_string()),
                agent_reply_summary: Some("Implemented bounded context.".to_string()),
                user_input_hash: Some("sha256:user".to_string()),
                agent_reply_hash: Some("sha256:reply".to_string()),
                user_input_chars: Some(2048),
                agent_reply_chars: Some(512),
                task: Some("fix issue 1180".to_string()),
                client_platform: Some("cursor".to_string()),
                client_os: Some("windows".to_string()),
                client_host: Some("workstation-42".to_string()),
                auth_subject: Some("oauth:user-1".to_string()),
                source_ip: Some("192.0.2.44".to_string()),
                forwarded_for: vec!["198.51.100.7".to_string()],
                reasoning_summary: Some("do not export me".to_string()),
                tags: vec!["ci".to_string(), "gateway".to_string()],
                metadata: json!({"api_key": "secret"}),
                ..AgentContext::default()
            }))
            .with_search_id(Some("search-1"))
            .with_ranker_version(Some("gateway-hybrid-v2"))
            .with_search_result(&json!({"total": 0, "hits": []}))
            .with_outcome(true, None);

        let attrs = event.attributes();
        assert_eq!(attrs["dcc_mcp.workflow.operation"], "gateway.search");
        assert_eq!(attrs["dcc_mcp.session_id"], "session-1");
        assert_eq!(attrs["dcc_mcp.actor.id"], "user-1");
        assert_eq!(attrs["dcc_mcp.actor.name"], "Morgan Artist");
        assert_eq!(attrs["dcc_mcp.actor.email_hash"], "sha256:actor");
        assert_eq!(attrs["dcc_mcp.agent.id"], "agent-1");
        assert_eq!(attrs["dcc_mcp.agent.version"], "0.9.0");
        assert_eq!(attrs["dcc_mcp.agent.model_provider"], "openai");
        assert_eq!(attrs["dcc_mcp.agent.model_version"], "gpt-5.1");
        assert_eq!(attrs["dcc_mcp.agent.reasoning_effort"], "high");
        assert_eq!(attrs["dcc_mcp.agent.turn_id"], "turn-1");
        assert_eq!(
            attrs["dcc_mcp.agent.user_intent_summary"],
            "Fix gateway telemetry."
        );
        assert_eq!(
            attrs["dcc_mcp.agent.reply_summary"],
            "Implemented bounded context."
        );
        assert_eq!(attrs["dcc_mcp.agent.user_input_hash"], "sha256:user");
        assert_eq!(attrs["dcc_mcp.agent.reply_hash"], "sha256:reply");
        assert_eq!(attrs["dcc_mcp.agent.user_input_chars"], 2048);
        assert_eq!(attrs["dcc_mcp.agent.reply_chars"], 512);
        assert_eq!(attrs["dcc_mcp.agent.tags"], json!(["ci", "gateway"]));
        assert_eq!(attrs["dcc_mcp.client.platform"], "cursor");
        assert_eq!(attrs["dcc_mcp.client.os"], "windows");
        assert_eq!(attrs["dcc_mcp.client.host"], "workstation-42");
        assert_eq!(attrs["dcc_mcp.auth.subject"], "oauth:user-1");
        assert_eq!(attrs["dcc_mcp.source.ip"], "192.0.2.44");
        assert_eq!(attrs["dcc_mcp.forwarded_for"], json!(["198.51.100.7"]));
        assert_eq!(attrs["dcc_mcp.search.id"], "search-1");
        assert_eq!(attrs["dcc_mcp.search.zero_results"], true);
        assert!(!attrs.contains_key("dcc_mcp.agent.reasoning_summary"));
        assert!(!attrs.contains_key("dcc_mcp.agent.metadata"));
    }

    #[test]
    fn selected_hit_populates_rank_score_and_match_reason_summary() {
        let hit = SearchTelemetryHit {
            tool_slug: "maya.abcdef01.model__create_cube".to_string(),
            skill_name: Some("maya-modeling".to_string()),
            dcc_type: "maya".to_string(),
            rank: 2,
            score: 87,
            match_reasons: vec![
                "alias".to_string(),
                "schema".to_string(),
                "summary".to_string(),
                "extra".to_string(),
                "extra2".to_string(),
                "extra3".to_string(),
            ],
            loaded: true,
        };

        let attrs = AgentWorkflowEvent::new("gateway.call", "mcp")
            .with_selected_hit(Some(&hit))
            .attributes();

        assert_eq!(attrs["dcc_mcp.search.selected_rank"], 2);
        assert_eq!(attrs["dcc_mcp.search.score"], 87);
        assert_eq!(
            attrs["dcc_mcp.search.match_reasons"],
            json!(["alias", "schema", "summary", "extra", "extra2"])
        );
        assert_eq!(attrs["dcc_mcp.dcc.type"], "maya");
    }

    #[test]
    fn mcp_search_event_records_bounded_result_attributes() {
        let store = SearchTelemetryStore::with_capacity(4);
        let args = json!({"query": "cube", "dcc_type": "maya", "instance_id": "abcdef01"});
        let agent_context = AgentContext {
            agent_id: Some("agent-1".to_string()),
            tags: vec!["eval".to_string()],
            metadata: json!({"raw_prompt": "do not export"}),
            ..AgentContext::default()
        };
        let trace_context = trace_context();
        let event = build_mcp_tool_event(McpToolTelemetryInput {
            search_telemetry: &store,
            tool: "search",
            args: &args,
            meta: None,
            trace_context: Some(&trace_context),
            agent_context: Some(&agent_context),
            session_id: Some("sess-1"),
            text: r#"{"search_id":"search-1","ranker_version":"gateway-hybrid-v2","total":2,"hits":[{"tool_slug":"maya.abcdef01.model__create_cube"}]}"#,
            is_error: false,
        })
        .expect("search emits telemetry");

        let attrs = event.attributes();
        assert_eq!(attrs["openinference.span.kind"], "CHAIN");
        assert_eq!(attrs["dcc_mcp.workflow.operation"], "gateway.search");
        assert_eq!(attrs["dcc_mcp.dcc.type"], "maya");
        assert_eq!(attrs["dcc_mcp.instance.id"], "abcdef01");
        assert_eq!(attrs["dcc_mcp.search.id"], "search-1");
        assert_eq!(attrs["dcc_mcp.search.total"], 2);
        assert_eq!(attrs["dcc_mcp.search.zero_results"], false);
        assert_eq!(attrs["dcc_mcp.agent.tags"], json!(["eval"]));
        assert!(!attrs.contains_key("dcc_mcp.agent.metadata"));
    }

    #[test]
    fn mcp_events_cover_load_skill_success_and_call_failure() {
        let store = SearchTelemetryStore::with_capacity(4);
        let search_id = SearchTelemetryStore::new_search_id();
        store.record_search(crate::gateway::search_telemetry::SearchTelemetryInput {
            search_id: search_id.clone(),
            transport: "mcp".to_string(),
            kind: "tool".to_string(),
            query: "cube".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            instance_id: None,
            limit: Some(5),
            total: 1,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "gen-1".to_string(),
            hits: vec![SearchTelemetryHit {
                tool_slug: "maya.abcdef01.model__create_cube".to_string(),
                skill_name: Some("maya-modeling".to_string()),
                dcc_type: "maya".to_string(),
                rank: 3,
                score: 80,
                match_reasons: vec!["skill_lexical".to_string()],
                loaded: false,
            }],
            trace_context: Some(trace_context()),
            session_id: Some("sess-1".to_string()),
            agent_context: None,
            tags_any: vec![],
        });

        let load_args =
            json!({"skill_name": "maya-modeling", "meta": {"search_id": search_id.clone()}});
        let trace_context = trace_context();
        let load = build_mcp_tool_event(McpToolTelemetryInput {
            search_telemetry: &store,
            tool: "load_skill",
            args: &load_args,
            meta: None,
            trace_context: Some(&trace_context),
            agent_context: None,
            session_id: Some("sess-1"),
            text: r#"{"success":true}"#,
            is_error: false,
        })
        .expect("load_skill emits telemetry")
        .attributes();
        assert_eq!(load["openinference.span.kind"], "TOOL");
        assert_eq!(load["dcc_mcp.workflow.operation"], "gateway.load_skill");
        assert_eq!(load["dcc_mcp.skill.name"], "maya-modeling");
        assert_eq!(load["dcc_mcp.search.selected_rank"], 3);
        assert_eq!(load["dcc_mcp.success"], true);

        let call_args = json!({
                "tool_slug": "maya.abcdef01.model__create_cube",
                "arguments": {},
                "meta": {"search_id": search_id}
        });
        let failed_call = build_mcp_tool_event(McpToolTelemetryInput {
            search_telemetry: &store,
            tool: "call",
            args: &call_args,
            meta: None,
            trace_context: Some(&trace_context),
            agent_context: None,
            session_id: Some("sess-1"),
            text: r#"{"error":{"kind":"backend-timeout","message":"timed out"}}"#,
            is_error: true,
        })
        .expect("failed call emits telemetry")
        .attributes();
        assert_eq!(failed_call["dcc_mcp.workflow.operation"], "gateway.call");
        assert_eq!(failed_call["dcc_mcp.error.kind"], "backend-timeout");
        assert_eq!(failed_call["dcc_mcp.success"], false);
        assert_eq!(failed_call["dcc_mcp.search.selected_rank"], 3);
    }

    #[test]
    fn mcp_event_correlates_call_to_search_hit_and_policy_error() {
        let store = SearchTelemetryStore::with_capacity(4);
        let search_id = SearchTelemetryStore::new_search_id();
        store.record_search(crate::gateway::search_telemetry::SearchTelemetryInput {
            search_id: search_id.clone(),
            transport: "mcp".to_string(),
            kind: "tool".to_string(),
            query: "cube".to_string(),
            dcc_type: Some("maya".to_string()),
            dcc_types: vec![],
            instance_id: None,
            limit: Some(5),
            total: 1,
            ranker_version: RANKER_VERSION.to_string(),
            index_generation: "gen-1".to_string(),
            hits: vec![SearchTelemetryHit {
                tool_slug: "maya.abcdef01.model__create_cube".to_string(),
                skill_name: Some("maya-modeling".to_string()),
                dcc_type: "maya".to_string(),
                rank: 1,
                score: 99,
                match_reasons: vec!["tool_lexical".to_string()],
                loaded: true,
            }],
            trace_context: Some(trace_context()),
            session_id: Some("sess-1".to_string()),
            agent_context: None,
            tags_any: vec![],
        });

        let args = json!({
                "tool_slug": "maya.abcdef01.model__create_cube",
                "arguments": {},
                "meta": {"search_id": search_id}
        });
        let trace_context = trace_context();
        let event = build_mcp_tool_event(McpToolTelemetryInput {
            search_telemetry: &store,
            tool: "call",
            args: &args,
            meta: None,
            trace_context: Some(&trace_context),
            agent_context: None,
            session_id: Some("sess-1"),
            text: r#"{"error":{"kind":"policy-denied","policy":{"reason":"read-only"}}}"#,
            is_error: true,
        })
        .expect("call emits telemetry");

        let attrs = event.attributes();
        assert_eq!(attrs["dcc_mcp.workflow.operation"], "gateway.call");
        assert_eq!(attrs["dcc_mcp.search.selected_rank"], 1);
        assert_eq!(attrs["dcc_mcp.search.score"], 99);
        assert_eq!(attrs["dcc_mcp.policy.outcome"], "denied");
        assert_eq!(attrs["dcc_mcp.policy.reason"], "read-only");
        assert_eq!(attrs["dcc_mcp.success"], false);
    }
}
