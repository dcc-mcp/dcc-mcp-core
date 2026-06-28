//! Agent/caller context types for the Admin UI `/api/traces` endpoint.
//!
//! Extracted from `domain/trace.rs` to keep file sizes under the 1500-line
//! hard limit. All types are re-exported through `domain/trace.rs` so the
//! public path `crate::gateway::admin::trace::*` is unchanged.

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::gateway::admin::domain::trace::{
    MAX_AGENT_CONTEXT_LIST_ITEMS, MAX_AGENT_CONTEXT_METADATA_BYTES, MAX_AGENT_CONTEXT_STRING_BYTES,
    parse_traceparent,
};

// ── Trust constants ──────────────────────────────────────────────────────────

pub const TRUST_SELF_REPORTED: &str = "self_reported";
pub const TRUST_HEADER: &str = "header";
pub const TRUST_AUTH: &str = "auth";
pub const TRUST_SERVER_DERIVED: &str = "server_derived";
pub const TRUST_TRUSTED_PROXY: &str = "trusted_proxy";

// ── AgentContextTrust ────────────────────────────────────────────────────────

/// Server-computed trust source for caller-attribution fields.
///
/// This map is deliberately ignored while parsing external request metadata and
/// filled by the gateway after each carrier has been normalised. Persisted admin
/// rows can still deserialize it so historical traces keep their trust labels.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentContextTrust {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_email_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forwarded_for: Option<String>,
}

impl AgentContextTrust {
    pub fn is_empty(&self) -> bool {
        self.actor_id.is_none()
            && self.actor_name.is_none()
            && self.actor_email_hash.is_none()
            && self.agent_id.is_none()
            && self.agent_name.is_none()
            && self.agent_kind.is_none()
            && self.agent_version.is_none()
            && self.model.is_none()
            && self.model_provider.is_none()
            && self.model_version.is_none()
            && self.client_platform.is_none()
            && self.client_os.is_none()
            && self.client_host.is_none()
            && self.auth_subject.is_none()
            && self.source_ip.is_none()
            && self.forwarded_for.is_none()
    }

    fn mark_present(&mut self, ctx: &AgentContext, source: &str) {
        set_trust_if_present(&mut self.actor_id, ctx.actor_id.as_ref(), source);
        set_trust_if_present(&mut self.actor_name, ctx.actor_name.as_ref(), source);
        set_trust_if_present(
            &mut self.actor_email_hash,
            ctx.actor_email_hash.as_ref(),
            source,
        );
        set_trust_if_present(&mut self.agent_id, ctx.agent_id.as_ref(), source);
        set_trust_if_present(&mut self.agent_name, ctx.agent_name.as_ref(), source);
        set_trust_if_present(&mut self.agent_kind, ctx.agent_kind.as_ref(), source);
        set_trust_if_present(&mut self.agent_version, ctx.agent_version.as_ref(), source);
        set_trust_if_present(&mut self.model, ctx.model.as_ref(), source);
        set_trust_if_present(
            &mut self.model_provider,
            ctx.model_provider.as_ref(),
            source,
        );
        set_trust_if_present(&mut self.model_version, ctx.model_version.as_ref(), source);
        set_trust_if_present(
            &mut self.client_platform,
            ctx.client_platform.as_ref(),
            source,
        );
        set_trust_if_present(&mut self.client_os, ctx.client_os.as_ref(), source);
        set_trust_if_present(&mut self.client_host, ctx.client_host.as_ref(), source);
        set_trust_if_present(&mut self.auth_subject, ctx.auth_subject.as_ref(), source);
    }
}

// ── AgentContext ─────────────────────────────────────────────────────────────

/// Optional client-supplied context that explains why a request was made.
///
/// This is deliberately a telemetry contract, not an instruction to capture a
/// model's hidden chain-of-thought. Agents may provide concise summaries,
/// plans, observations, and correlation IDs; non-agent clients can use the same
/// fields as ordinary caller context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentContext {
    #[serde(default, alias = "actorId", skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, alias = "actorName", skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    #[serde(
        default,
        alias = "actorEmailHash",
        skip_serializing_if = "Option::is_none"
    )]
    pub actor_email_hash: Option<String>,
    #[serde(default, alias = "agentId", skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, alias = "agentName", skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, alias = "agentKind", skip_serializing_if = "Option::is_none")]
    pub agent_kind: Option<String>,
    #[serde(
        default,
        alias = "agentVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub agent_version: Option<String>,
    #[serde(
        default,
        alias = "modelProvider",
        alias = "agentModelProvider",
        skip_serializing_if = "Option::is_none"
    )]
    pub model_provider: Option<String>,
    #[serde(
        default,
        alias = "modelVersion",
        alias = "agentModelVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub model_version: Option<String>,
    #[serde(default, alias = "agentModel", skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(
        default,
        alias = "reasoningEffort",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_effort: Option<String>,
    #[serde(default, alias = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, alias = "turnId", skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(
        default,
        alias = "clientPlatform",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_platform: Option<String>,
    #[serde(
        default,
        alias = "clientOs",
        alias = "clientOS",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_os: Option<String>,
    #[serde(default, alias = "clientHost", skip_serializing_if = "Option::is_none")]
    pub client_host: Option<String>,
    #[serde(
        default,
        alias = "authSubject",
        skip_serializing_if = "Option::is_none"
    )]
    pub auth_subject: Option<String>,
    /// Server-derived remote address. Request metadata and headers must not set
    /// this field; use [`AgentContext::with_server_network_source`] at the
    /// transport boundary after proxy trust policy has been applied.
    #[serde(default, alias = "sourceIp", skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<String>,
    /// Server-derived forwarded chain after proxy trust policy has been
    /// applied. Client-supplied request metadata is ignored.
    #[serde(default, alias = "forwardedFor", skip_serializing_if = "Vec::is_empty")]
    pub forwarded_for: Vec<String>,
    #[serde(
        default,
        alias = "userIntentSummary",
        skip_serializing_if = "Option::is_none"
    )]
    pub user_intent_summary: Option<String>,
    #[serde(
        default,
        alias = "agentReplySummary",
        skip_serializing_if = "Option::is_none"
    )]
    pub agent_reply_summary: Option<String>,
    #[serde(
        default,
        alias = "userInputHash",
        skip_serializing_if = "Option::is_none"
    )]
    pub user_input_hash: Option<String>,
    #[serde(
        default,
        alias = "agentReplyHash",
        skip_serializing_if = "Option::is_none"
    )]
    pub agent_reply_hash: Option<String>,
    #[serde(
        default,
        alias = "userInputChars",
        skip_serializing_if = "Option::is_none"
    )]
    pub user_input_chars: Option<u64>,
    #[serde(
        default,
        alias = "agentReplyChars",
        skip_serializing_if = "Option::is_none"
    )]
    pub agent_reply_chars: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plan: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub metadata: Value,
    #[serde(default, skip_serializing_if = "AgentContextTrust::is_empty")]
    pub trust: AgentContextTrust,
}

impl AgentContext {
    pub fn from_mcp_client_info(session_id: &str, params: Option<&Value>) -> Option<Self> {
        let client_info = params
            .and_then(|value| value.get("clientInfo").or_else(|| value.get("client_info")))?;
        let Value::Object(info) = client_info else {
            return None;
        };
        let name = info
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let version = info
            .get("version")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if name.is_none() && version.is_none() {
            return None;
        }

        let mut ctx = AgentContext {
            agent_name: name.clone(),
            agent_kind: Some("mcp-client".to_string()),
            agent_version: version,
            client_platform: name,
            session_id: Some(session_id.to_string()),
            ..AgentContext::default()
        }
        .normalise();
        let snapshot = ctx.clone();
        ctx.trust.mark_present(&snapshot, TRUST_SELF_REPORTED);
        Some(ctx)
    }

    pub fn merge_missing_client_identity_from(&mut self, fallback: &AgentContext) {
        fill_missing_context_string(
            &mut self.agent_id,
            &mut self.trust.agent_id,
            &fallback.agent_id,
            &fallback.trust.agent_id,
        );
        fill_missing_context_string(
            &mut self.agent_name,
            &mut self.trust.agent_name,
            &fallback.agent_name,
            &fallback.trust.agent_name,
        );
        fill_missing_context_string(
            &mut self.agent_kind,
            &mut self.trust.agent_kind,
            &fallback.agent_kind,
            &fallback.trust.agent_kind,
        );
        fill_missing_context_string(
            &mut self.agent_version,
            &mut self.trust.agent_version,
            &fallback.agent_version,
            &fallback.trust.agent_version,
        );
        fill_missing_context_string(
            &mut self.model,
            &mut self.trust.model,
            &fallback.model,
            &fallback.trust.model,
        );
        fill_missing_context_string(
            &mut self.model_provider,
            &mut self.trust.model_provider,
            &fallback.model_provider,
            &fallback.trust.model_provider,
        );
        fill_missing_context_string(
            &mut self.model_version,
            &mut self.trust.model_version,
            &fallback.model_version,
            &fallback.trust.model_version,
        );
        fill_missing_context_string(
            &mut self.client_platform,
            &mut self.trust.client_platform,
            &fallback.client_platform,
            &fallback.trust.client_platform,
        );
        fill_missing_context_string(
            &mut self.client_os,
            &mut self.trust.client_os,
            &fallback.client_os,
            &fallback.trust.client_os,
        );
        fill_missing_context_string(
            &mut self.client_host,
            &mut self.trust.client_host,
            &fallback.client_host,
            &fallback.trust.client_host,
        );
        if self.session_id.is_none() {
            self.session_id = fallback.session_id.clone();
        }
    }

    pub fn from_request_parts(
        headers: &HeaderMap,
        body: Option<&Value>,
        meta: Option<&Value>,
    ) -> Option<Self> {
        let mut ctx = body
            .and_then(|value| agent_context_from_value(value, TRUST_SELF_REPORTED))
            .or_else(|| meta.and_then(|value| agent_context_from_value(value, TRUST_SELF_REPORTED)))
            .or_else(|| agent_context_from_header(headers))
            .unwrap_or_default();

        merge_header_agent_context(&mut ctx, headers);
        if ctx.is_empty() { None } else { Some(ctx) }
    }

    pub fn from_request_parts_with_server_network(
        headers: &HeaderMap,
        body: Option<&Value>,
        meta: Option<&Value>,
    ) -> Option<Self> {
        let mut ctx = Self::from_request_parts(headers, body, meta).unwrap_or_default();
        let network = crate::gateway::caller_attribution::internal_network_attribution(headers);
        ctx = ctx.with_server_network_source(network.source_ip, network.forwarded_for);
        if ctx.is_empty() { None } else { Some(ctx) }
    }

    pub fn display_name(&self) -> Option<&str> {
        self.actor_name
            .as_deref()
            .or(self.actor_id.as_deref())
            .or(self.agent_name.as_deref())
            .or(self.agent_id.as_deref())
            .or(self.agent_kind.as_deref())
    }

    #[must_use]
    pub fn with_server_network_source(
        mut self,
        source_ip: Option<String>,
        forwarded_for: Vec<String>,
    ) -> Self {
        self.source_ip = source_ip.map(bound_context_string);
        self.forwarded_for = bound_context_list(forwarded_for);
        if self.source_ip.is_some() {
            let source = if self.forwarded_for.is_empty() {
                TRUST_SERVER_DERIVED
            } else {
                TRUST_TRUSTED_PROXY
            };
            set_trust(&mut self.trust.source_ip, source);
        }
        if !self.forwarded_for.is_empty() {
            set_trust(&mut self.trust.forwarded_for, TRUST_TRUSTED_PROXY);
        }
        self
    }

    fn is_empty(&self) -> bool {
        self.actor_id.is_none()
            && self.actor_name.is_none()
            && self.actor_email_hash.is_none()
            && self.agent_id.is_none()
            && self.agent_name.is_none()
            && self.agent_kind.is_none()
            && self.agent_version.is_none()
            && self.model_provider.is_none()
            && self.model_version.is_none()
            && self.model.is_none()
            && self.reasoning_effort.is_none()
            && self.session_id.is_none()
            && self.turn_id.is_none()
            && self.task.is_none()
            && self.client_platform.is_none()
            && self.client_os.is_none()
            && self.client_host.is_none()
            && self.auth_subject.is_none()
            && self.source_ip.is_none()
            && self.forwarded_for.is_empty()
            && self.user_intent_summary.is_none()
            && self.agent_reply_summary.is_none()
            && self.user_input_hash.is_none()
            && self.agent_reply_hash.is_none()
            && self.user_input_chars.is_none()
            && self.agent_reply_chars.is_none()
            && self.reasoning_summary.is_none()
            && self.plan.is_empty()
            && self.observations.is_empty()
            && self.tags.is_empty()
            && self.parent_request_id.is_none()
            && self.trace_id.is_none()
            && self.turn_index.is_none()
            && self.metadata.is_null()
            && self.trust.is_empty()
    }

    fn normalise(mut self) -> Self {
        self.trust = AgentContextTrust::default();
        self.actor_id = self.actor_id.map(bound_context_string);
        self.actor_name = self.actor_name.map(bound_context_string);
        self.actor_email_hash = self.actor_email_hash.map(bound_context_string);
        self.agent_id = self.agent_id.map(bound_context_string);
        self.agent_name = self.agent_name.map(bound_context_string);
        self.agent_kind = self.agent_kind.map(bound_context_string);
        self.agent_version = self.agent_version.map(bound_context_string);
        self.model_provider = self.model_provider.map(bound_context_string);
        self.model_version = self.model_version.map(bound_context_string);
        self.model = self.model.map(bound_context_string);
        self.reasoning_effort = self.reasoning_effort.map(bound_context_string);
        self.session_id = self.session_id.map(bound_context_string);
        self.turn_id = self.turn_id.map(bound_context_string);
        self.task = self.task.map(bound_context_string);
        self.client_platform = self.client_platform.map(bound_context_string);
        self.client_os = self.client_os.map(bound_context_string);
        self.client_host = self.client_host.map(bound_context_string);
        self.auth_subject = self.auth_subject.map(bound_context_string);
        self.source_ip = None;
        self.forwarded_for.clear();
        self.user_intent_summary = self.user_intent_summary.map(bound_context_string);
        self.agent_reply_summary = self.agent_reply_summary.map(bound_context_string);
        self.user_input_hash = self.user_input_hash.map(bound_context_string);
        self.agent_reply_hash = self.agent_reply_hash.map(bound_context_string);
        self.reasoning_summary = self.reasoning_summary.map(bound_context_string);
        self.parent_request_id = self.parent_request_id.map(bound_context_string);
        self.trace_id = self.trace_id.map(bound_context_string);
        self.plan = bound_context_list(self.plan);
        self.observations = bound_context_list(self.observations);
        self.tags = bound_context_list(self.tags);
        self.metadata = bound_context_metadata(self.metadata);
        self
    }
}

// ── Private helpers for AgentContext ─────────────────────────────────────────

fn set_trust_if_present(slot: &mut Option<String>, value: Option<&String>, source: &str) {
    if value.is_some() {
        *slot = Some(source.to_string());
    }
}

fn set_trust(slot: &mut Option<String>, source: &str) {
    *slot = Some(source.to_string());
}

fn fill_missing_context_string(
    slot: &mut Option<String>,
    trust_slot: &mut Option<String>,
    fallback: &Option<String>,
    fallback_trust: &Option<String>,
) {
    if slot.is_none()
        && let Some(value) = fallback
    {
        *slot = Some(value.clone());
        *trust_slot = fallback_trust.clone();
    }
}

fn agent_context_from_value(value: &Value, trust_source: &str) -> Option<AgentContext> {
    let raw = value
        .get("agent_context")
        .or_else(|| value.get("agentContext"))
        .or_else(|| value.get("agent"))
        .or_else(|| value.get("caller_context"))
        .or_else(|| value.get("callerContext"))
        .or_else(|| {
            value
                .get("dcc_mcp")
                .and_then(|v| v.get("agent_context").or_else(|| v.get("agentContext")))
        })?;
    match raw {
        Value::String(s) => Some(AgentContext {
            reasoning_summary: Some(bound_context_string(s.clone())),
            ..AgentContext::default()
        }),
        Value::Object(_) => serde_json::from_value::<AgentContext>(raw.clone())
            .ok()
            .map(AgentContext::normalise)
            .map(|mut ctx| {
                let snapshot = ctx.clone();
                ctx.trust.mark_present(&snapshot, trust_source);
                ctx
            }),
        _ => None,
    }
}

fn agent_context_from_header(headers: &HeaderMap) -> Option<AgentContext> {
    let raw = header_str(headers, "x-dcc-mcp-agent-context")?;
    serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|v| match v {
            Value::String(s) => Some(AgentContext {
                reasoning_summary: Some(bound_context_string(s)),
                ..AgentContext::default()
            }),
            Value::Object(_) => serde_json::from_value::<AgentContext>(v)
                .ok()
                .map(AgentContext::normalise)
                .map(|mut ctx| {
                    let snapshot = ctx.clone();
                    ctx.trust.mark_present(&snapshot, TRUST_HEADER);
                    ctx
                }),
            _ => None,
        })
}

fn fill_header_trusted_string(
    slot: &mut Option<String>,
    trust_slot: &mut Option<String>,
    headers: &HeaderMap,
    name: &str,
) {
    if slot.is_none()
        && let Some(value) = header_str(headers, name).map(bound_context_string)
    {
        *slot = Some(value);
        set_trust(trust_slot, TRUST_HEADER);
    }
}

fn fill_header_trusted_string_any(
    slot: &mut Option<String>,
    trust_slot: &mut Option<String>,
    headers: &HeaderMap,
    names: &[&str],
) {
    if slot.is_none()
        && let Some(value) = header_str_any(headers, names).map(bound_context_string)
    {
        *slot = Some(value);
        set_trust(trust_slot, TRUST_HEADER);
    }
}

fn merge_header_agent_context(ctx: &mut AgentContext, headers: &HeaderMap) {
    fill_header_trusted_string(
        &mut ctx.actor_id,
        &mut ctx.trust.actor_id,
        headers,
        "x-dcc-mcp-actor-id",
    );
    fill_header_trusted_string(
        &mut ctx.actor_name,
        &mut ctx.trust.actor_name,
        headers,
        "x-dcc-mcp-actor-name",
    );
    fill_header_trusted_string(
        &mut ctx.actor_email_hash,
        &mut ctx.trust.actor_email_hash,
        headers,
        "x-dcc-mcp-actor-email-hash",
    );
    fill_header_trusted_string(
        &mut ctx.agent_id,
        &mut ctx.trust.agent_id,
        headers,
        "x-dcc-mcp-agent-id",
    );
    fill_header_trusted_string_any(
        &mut ctx.agent_name,
        &mut ctx.trust.agent_name,
        headers,
        &["x-dcc-mcp-agent-name", "x-dcc-mcp-agent"],
    );
    fill_header_trusted_string(
        &mut ctx.agent_kind,
        &mut ctx.trust.agent_kind,
        headers,
        "x-dcc-mcp-agent-kind",
    );
    fill_header_trusted_string(
        &mut ctx.agent_version,
        &mut ctx.trust.agent_version,
        headers,
        "x-dcc-mcp-agent-version",
    );
    fill_header_trusted_string_any(
        &mut ctx.model_provider,
        &mut ctx.trust.model_provider,
        headers,
        &["x-dcc-mcp-agent-model-provider", "x-dcc-mcp-model-provider"],
    );
    fill_header_trusted_string_any(
        &mut ctx.model_version,
        &mut ctx.trust.model_version,
        headers,
        &["x-dcc-mcp-agent-model-version", "x-dcc-mcp-model-version"],
    );
    fill_header_trusted_string(
        &mut ctx.model,
        &mut ctx.trust.model,
        headers,
        "x-dcc-mcp-agent-model",
    );
    if ctx.reasoning_effort.is_none() {
        ctx.reasoning_effort = header_str_any(
            headers,
            &[
                "x-dcc-mcp-agent-reasoning-effort",
                "x-dcc-mcp-reasoning-effort",
            ],
        )
        .map(bound_context_string);
    }
    if ctx.session_id.is_none() {
        ctx.session_id = header_str_any(
            headers,
            &["x-dcc-mcp-agent-session-id", "x-dcc-mcp-session-id"],
        )
        .map(bound_context_string);
    }
    if ctx.turn_id.is_none() {
        ctx.turn_id = header_str_any(headers, &["x-dcc-mcp-agent-turn-id", "x-dcc-mcp-turn-id"])
            .map(bound_context_string);
    }
    if ctx.task.is_none() {
        ctx.task = header_str(headers, "x-dcc-mcp-agent-task").map(bound_context_string);
    }
    fill_header_trusted_string(
        &mut ctx.client_platform,
        &mut ctx.trust.client_platform,
        headers,
        "x-dcc-mcp-client-platform",
    );
    if ctx.client_platform.is_none()
        && let Some(value) = client_platform_from_user_agent(headers)
    {
        ctx.client_platform = Some(value);
        set_trust(&mut ctx.trust.client_platform, TRUST_HEADER);
    }
    fill_header_trusted_string(
        &mut ctx.client_os,
        &mut ctx.trust.client_os,
        headers,
        "x-dcc-mcp-client-os",
    );
    fill_header_trusted_string(
        &mut ctx.client_host,
        &mut ctx.trust.client_host,
        headers,
        "x-dcc-mcp-client-host",
    );
    if let Some(value) = header_str(
        headers,
        crate::gateway::caller_attribution::INTERNAL_AUTH_SUBJECT_HEADER,
    )
    .map(bound_context_string)
    {
        ctx.auth_subject = Some(value);
        set_trust(&mut ctx.trust.auth_subject, TRUST_AUTH);
    } else if ctx.auth_subject.is_none()
        && let Some(value) = header_str(headers, "x-dcc-mcp-auth-subject").map(bound_context_string)
    {
        ctx.auth_subject = Some(value);
        set_trust(&mut ctx.trust.auth_subject, TRUST_HEADER);
    }
    if ctx.user_intent_summary.is_none() {
        ctx.user_intent_summary = header_str_any(
            headers,
            &[
                "x-dcc-mcp-agent-user-intent-summary",
                "x-dcc-mcp-user-intent-summary",
            ],
        )
        .map(bound_context_string);
    }
    if ctx.agent_reply_summary.is_none() {
        ctx.agent_reply_summary = header_str_any(
            headers,
            &[
                "x-dcc-mcp-agent-reply-summary",
                "x-dcc-mcp-agent-agent-reply-summary",
            ],
        )
        .map(bound_context_string);
    }
    if ctx.user_input_hash.is_none() {
        ctx.user_input_hash = header_str_any(
            headers,
            &[
                "x-dcc-mcp-agent-user-input-hash",
                "x-dcc-mcp-user-input-hash",
            ],
        )
        .map(bound_context_string);
    }
    if ctx.agent_reply_hash.is_none() {
        ctx.agent_reply_hash = header_str_any(
            headers,
            &[
                "x-dcc-mcp-agent-reply-hash",
                "x-dcc-mcp-agent-agent-reply-hash",
            ],
        )
        .map(bound_context_string);
    }
    if ctx.user_input_chars.is_none() {
        ctx.user_input_chars = header_u64_any(
            headers,
            &[
                "x-dcc-mcp-agent-user-input-chars",
                "x-dcc-mcp-user-input-chars",
            ],
        );
    }
    if ctx.agent_reply_chars.is_none() {
        ctx.agent_reply_chars = header_u64_any(
            headers,
            &[
                "x-dcc-mcp-agent-reply-chars",
                "x-dcc-mcp-agent-agent-reply-chars",
            ],
        );
    }
    if ctx.reasoning_summary.is_none() {
        ctx.reasoning_summary =
            header_str(headers, "x-dcc-mcp-reasoning-summary").map(bound_context_string);
    }
    if ctx.parent_request_id.is_none() {
        ctx.parent_request_id =
            header_str(headers, "x-dcc-mcp-parent-request-id").map(bound_context_string);
    }
    if ctx.trace_id.is_none() {
        ctx.trace_id = header_str(headers, "traceparent")
            .and_then(|value| parse_traceparent(&value).map(|tp| tp.trace_id))
            .or_else(|| header_str(headers, "x-trace-id"))
            .map(bound_context_string);
    }
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn header_str_any(headers: &HeaderMap, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| header_str(headers, name))
}

fn header_u64_any(headers: &HeaderMap, names: &[&str]) -> Option<u64> {
    header_str_any(headers, names).and_then(|value| value.parse::<u64>().ok())
}

fn client_platform_from_user_agent(headers: &HeaderMap) -> Option<String> {
    let raw = header_str(headers, "user-agent")?;
    let product = raw
        .split_whitespace()
        .next()
        .unwrap_or(raw.as_str())
        .split('/')
        .next()
        .unwrap_or(raw.as_str())
        .trim();
    if product.is_empty() {
        None
    } else {
        Some(bound_context_string(product.to_string()))
    }
}

// ── Bounding helpers ────────────────────────────────────────────────────────

fn bound_context_string(value: String) -> String {
    truncate_utf8(value, MAX_AGENT_CONTEXT_STRING_BYTES).0
}

fn bound_context_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .take(MAX_AGENT_CONTEXT_LIST_ITEMS)
        .map(bound_context_string)
        .collect()
}

fn bound_context_metadata(value: Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    let sanitized = sanitize_context_metadata(value);
    let raw = serde_json::to_string(&sanitized).unwrap_or_default();
    if raw.len() <= MAX_AGENT_CONTEXT_METADATA_BYTES {
        return sanitized;
    }
    let (preview, _) = truncate_utf8(raw.clone(), MAX_AGENT_CONTEXT_METADATA_BYTES);
    json!({
        "truncated": true,
        "original_size": raw.len(),
        "preview": preview,
    })
}

fn sanitize_context_metadata(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sanitized = serde_json::Map::new();
            let mut redacted = 0usize;
            for (key, value) in map {
                if is_high_sensitivity_agent_key(&key) {
                    redacted += 1;
                } else {
                    sanitized.insert(key, sanitize_context_metadata(value));
                }
            }
            if redacted > 0 {
                sanitized.insert(
                    "redacted_high_sensitivity_fields".to_string(),
                    json!(redacted),
                );
            }
            Value::Object(sanitized)
        }
        Value::Array(values) => {
            Value::Array(values.into_iter().map(sanitize_context_metadata).collect())
        }
        other => other,
    }
}

fn is_high_sensitivity_agent_key(key: &str) -> bool {
    let normalised = key
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-' && *ch != ' ')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalised.as_str(),
        "agentreply"
            | "agentresponse"
            | "apikey"
            | "authsubject"
            | "authorization"
            | "bearertoken"
            | "chainofthought"
            | "email"
            | "hiddencot"
            | "messages"
            | "password"
            | "prompt"
            | "prompts"
            | "rawagentreply"
            | "rawagentresponse"
            | "rawprompt"
            | "rawresponse"
            | "rawuserinput"
            | "reply"
            | "response"
            | "token"
            | "userinput"
    ) || normalised.contains("secret")
        || normalised.contains("email")
        || normalised.contains("apikey")
        || normalised.contains("token")
}

// ── UTF-8 truncation ────────────────────────────────────────────────────────

fn truncate_utf8(value: String, cap: usize) -> (String, bool) {
    let original_size = value.len();
    if original_size <= cap {
        return (value, false);
    }
    let mut end = cap;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), true)
}
