//! Per-call dispatch trace types for the Admin UI `/api/traces` endpoint.
//!
//! Every `tools/call` routed through the gateway produces one [`DispatchTrace`]
//! that records a waterfall of [`TraceSpan`]s (gateway → middleware → backend →
//! response) plus optionally the request/response payloads. Input payloads are
//! captured after [`RedactionMiddleware`] and other before-call middleware have
//! run, then bounded before storage.
//!
//! The ring buffer (`TraceLog`) lives in [`AdminState`] and is populated by
//! [`TraceSink`] which is called from `AuditMiddleware::after_call`.

use std::collections::HashMap;
use std::time::SystemTime;

use axum::http::HeaderMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// ── Trace Context ────────────────────────────────────────────────────────────

/// End-to-end trace identity propagated across gateway, sidecar, and host hops.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    /// End-to-end unit of work, W3C-compatible 16-byte lowercase hex.
    pub trace_id: String,
    /// Gateway-facing request id, kept distinct from `trace_id`.
    pub request_id: String,
    /// Current gateway span id, W3C-compatible 8-byte lowercase hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    /// Incoming parent span id from W3C `traceparent`, when supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    /// Request-level parent/child relationship for agent turns, jobs, retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    /// W3C trace flags, usually `"00"` or `"01"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_flags: Option<String>,
    /// W3C `tracestate` header value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_state: Option<String>,
}

impl TraceContext {
    /// Build context for an HTTP request, preserving `x-request-id` separately
    /// from any W3C `traceparent` trace id.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let request_id = header_str(headers, "x-request-id")
            .or_else(|| header_str(headers, "x-correlation-id"))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Self::from_headers_with_request_id(headers, request_id)
    }

    /// Build context for a JSON-RPC request where the id is already the
    /// request identity and must not be replaced by transport headers.
    pub fn from_headers_with_request_id(
        headers: &HeaderMap,
        request_id: impl Into<String>,
    ) -> Self {
        let parsed = headers
            .get("traceparent")
            .and_then(|v| v.to_str().ok())
            .and_then(parse_traceparent);
        let explicit_trace_id = header_str(headers, "x-trace-id")
            .filter(|value| is_valid_trace_id(value))
            .map(|value| value.to_ascii_lowercase());
        let parent_request_id = header_str(headers, "x-dcc-mcp-parent-request-id");
        let trace_state = header_str(headers, "tracestate");

        Self {
            trace_id: parsed
                .as_ref()
                .map(|tp| tp.trace_id.clone())
                .or(explicit_trace_id)
                .unwrap_or_else(new_trace_id),
            request_id: request_id.into(),
            span_id: Some(new_span_id()),
            parent_span_id: parsed.as_ref().map(|tp| tp.parent_span_id.clone()),
            parent_request_id,
            trace_flags: parsed
                .as_ref()
                .map(|tp| tp.trace_flags.clone())
                .or_else(|| Some("00".to_string())),
            trace_state,
        }
    }

    pub fn child_span(
        &self,
        name: impl Into<String>,
        started_ns: u64,
        duration_ns: u64,
    ) -> TraceSpan {
        let mut span = TraceSpan::new(name, started_ns, duration_ns);
        span.parent_span_id = self.span_id.clone();
        span
    }

    pub fn traceparent(&self) -> Option<String> {
        let span_id = self.span_id.as_deref()?;
        Some(format!(
            "00-{}-{}-{}",
            self.trace_id,
            span_id,
            self.trace_flags.as_deref().unwrap_or("00")
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTraceParent {
    trace_id: String,
    parent_span_id: String,
    trace_flags: String,
}

pub fn parse_traceparent(value: &str) -> Option<TraceContextHeader> {
    parse_traceparent_inner(value).map(|p| TraceContextHeader {
        trace_id: p.trace_id,
        parent_span_id: p.parent_span_id,
        trace_flags: p.trace_flags,
    })
}

/// Parsed public shape for tests and callers that only need header fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContextHeader {
    pub trace_id: String,
    pub parent_span_id: String,
    pub trace_flags: String,
}

fn parse_traceparent_inner(value: &str) -> Option<ParsedTraceParent> {
    let mut parts = value.trim().split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let parent_span_id = parts.next()?;
    let trace_flags = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if version.len() != 2 || !is_lower_hex(version) || version.eq_ignore_ascii_case("ff") {
        return None;
    }
    if !is_valid_trace_id(trace_id) || !is_valid_span_id(parent_span_id) {
        return None;
    }
    if trace_flags.len() != 2 || !is_lower_hex(trace_flags) {
        return None;
    }
    Some(ParsedTraceParent {
        trace_id: trace_id.to_ascii_lowercase(),
        parent_span_id: parent_span_id.to_ascii_lowercase(),
        trace_flags: trace_flags.to_ascii_lowercase(),
    })
}

fn is_valid_trace_id(value: &str) -> bool {
    value.len() == 32 && is_lower_hex(value) && value != "00000000000000000000000000000000"
}

fn is_valid_span_id(value: &str) -> bool {
    value.len() == 16 && is_lower_hex(value) && value != "0000000000000000"
}

fn is_lower_hex(value: &str) -> bool {
    value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn new_trace_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn new_span_id() -> String {
    uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(16)
        .collect()
}

// ── Payload capture ───────────────────────────────────────────────────────────

/// Hard limits for payload capture (bytes, not tokens).
pub const MAX_INPUT_BYTES: usize = 16 * 1024; // 16 KB
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024; // 64 KB
pub const MAX_AGENT_CONTEXT_STRING_BYTES: usize = 2 * 1024; // 2 KB per field
pub const MAX_AGENT_CONTEXT_METADATA_BYTES: usize = 8 * 1024; // 8 KB JSON preview
pub const MAX_AGENT_CONTEXT_LIST_ITEMS: usize = 16;

/// Captured payload (input arguments or output content), optionally truncated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracePayload {
    /// UTF-8 content, possibly truncated.
    pub content: String,
    /// MIME type hint — always `"application/json"` for gateway traffic.
    pub mime_type: String,
    /// True when `original_size > content.len()`.
    pub truncated: bool,
    /// Original byte length before truncation.
    pub original_size: usize,
    /// Approximate token count inferred from the raw JSON/string payload.
    ///
    /// This is a deterministic, lightweight estimate intended for
    /// call-size triage; it intentionally does not require any tokenization
    /// runtime or model-specific encoder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_tokens: Option<usize>,
}

impl TracePayload {
    /// Build a `TracePayload`, truncating at `cap` bytes if necessary.
    pub fn from_value(v: &Value, cap: usize) -> Self {
        let raw = serde_json::to_string(v).unwrap_or_default();
        let original_size = raw.len();
        let truncated = original_size > cap;
        let estimated_tokens = crate::gateway::response_codec::estimate_tokens(raw.as_bytes());
        let content = if truncated {
            // Truncate at a valid UTF-8 boundary.
            let boundary = raw
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i < cap)
                .last()
                .unwrap_or(cap.min(original_size));
            raw[..boundary].to_owned()
        } else {
            raw
        };
        Self {
            content,
            mime_type: "application/json".to_string(),
            truncated,
            original_size,
            estimated_tokens: Some(estimated_tokens),
        }
    }

    /// Build an input payload with default script-source redaction.
    ///
    /// The gateway stores request arguments for admin traces and audit rows.
    /// Ad-hoc script source can be large and sensitive, so default capture
    /// keeps the shape and records that source existed without storing it.
    pub fn from_input_value(v: &Value, cap: usize) -> Self {
        let mut redacted = v.clone();
        redact_script_source_fields(&mut redacted);
        Self::from_value(&redacted, cap)
    }

    pub fn from_str(s: &str, cap: usize) -> Self {
        let original_size = s.len();
        let truncated = original_size > cap;
        let content = if truncated {
            // Truncate at valid UTF-8 boundary.
            let boundary = s
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i < cap)
                .last()
                .unwrap_or(cap.min(original_size));
            s[..boundary].to_owned()
        } else {
            s.to_owned()
        };
        Self {
            content,
            mime_type: "text/plain".to_string(),
            truncated,
            original_size,
            estimated_tokens: Some(crate::gateway::response_codec::estimate_tokens(
                s.as_bytes(),
            )),
        }
    }
}

fn redact_script_source_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if is_script_source_key(key) {
                    *child = Value::String("[REDACTED_SCRIPT_SOURCE]".to_string());
                } else {
                    redact_script_source_fields(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_script_source_fields(item);
            }
        }
        _ => {}
    }
}

fn is_script_source_key(key: &str) -> bool {
    matches!(key, "code" | "content" | "script" | "python" | "mel")
}

// ── Token telemetry ─────────────────────────────────────────────────────────

/// Bounded token-accounting metadata persisted with trace and audit rows.
///
/// This stores derived sizes and token estimates only. It deliberately avoids
/// storing raw response bodies, so existing trace/audit payload caps remain the
/// only place where content previews can appear.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenTelemetry {
    /// Response format returned to the client, e.g. `"json"` or `"toon"`.
    pub response_format: String,
    /// Stable id for the estimator used to compute token counts.
    pub token_estimator: String,
    /// Byte length of the un-compacted JSON response candidate.
    pub original_bytes: usize,
    /// Byte length of the response body returned to the client.
    pub returned_bytes: usize,
    /// Estimated tokens for the original JSON response candidate.
    pub original_tokens: usize,
    /// Estimated tokens for the response returned to the client.
    pub returned_tokens: usize,
    /// Estimated tokens saved by compact output. Legacy JSON uses `0`.
    pub saved_tokens: usize,
    /// Savings percentage as a numeric value in the range `[0.0, 100.0]`.
    pub savings_pct: f64,
}

impl TokenTelemetry {
    pub(crate) fn from_accounting(
        format: crate::gateway::response_codec::ResponseFormat,
        accounting: crate::gateway::response_codec::TokenAccounting,
    ) -> Self {
        Self {
            response_format: format.as_str().to_string(),
            token_estimator: crate::gateway::response_codec::TOKEN_ESTIMATOR.to_string(),
            original_bytes: accounting.original_bytes,
            returned_bytes: accounting.returned_bytes,
            original_tokens: accounting.original_tokens,
            returned_tokens: accounting.returned_tokens,
            saved_tokens: accounting.saved_tokens,
            savings_pct: round_two(accounting.savings_percent()),
        }
    }

    #[must_use]
    pub fn is_legacy_json(&self) -> bool {
        self.response_format == "json" && self.saved_tokens == 0
    }
}

fn round_two(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

// ── Agent / caller context ───────────────────────────────────────────────────

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
}

impl AgentContext {
    pub fn from_request_parts(
        headers: &HeaderMap,
        body: Option<&Value>,
        meta: Option<&Value>,
    ) -> Option<Self> {
        let mut ctx = body
            .and_then(agent_context_from_value)
            .or_else(|| meta.and_then(agent_context_from_value))
            .or_else(|| agent_context_from_header(headers))
            .unwrap_or_default();

        merge_header_agent_context(&mut ctx, headers);
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
    }

    fn normalise(mut self) -> Self {
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

fn agent_context_from_value(value: &Value) -> Option<AgentContext> {
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
            .map(AgentContext::normalise),
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
                .map(AgentContext::normalise),
            _ => None,
        })
}

fn merge_header_agent_context(ctx: &mut AgentContext, headers: &HeaderMap) {
    if ctx.actor_id.is_none() {
        ctx.actor_id = header_str(headers, "x-dcc-mcp-actor-id").map(bound_context_string);
    }
    if ctx.actor_name.is_none() {
        ctx.actor_name = header_str(headers, "x-dcc-mcp-actor-name").map(bound_context_string);
    }
    if ctx.actor_email_hash.is_none() {
        ctx.actor_email_hash =
            header_str(headers, "x-dcc-mcp-actor-email-hash").map(bound_context_string);
    }
    if ctx.agent_id.is_none() {
        ctx.agent_id = header_str(headers, "x-dcc-mcp-agent-id").map(bound_context_string);
    }
    if ctx.agent_name.is_none() {
        ctx.agent_name = header_str(headers, "x-dcc-mcp-agent-name").map(bound_context_string);
    }
    if ctx.agent_kind.is_none() {
        ctx.agent_kind = header_str(headers, "x-dcc-mcp-agent-kind").map(bound_context_string);
    }
    if ctx.agent_version.is_none() {
        ctx.agent_version =
            header_str(headers, "x-dcc-mcp-agent-version").map(bound_context_string);
    }
    if ctx.model_provider.is_none() {
        ctx.model_provider = header_str_any(
            headers,
            &["x-dcc-mcp-agent-model-provider", "x-dcc-mcp-model-provider"],
        )
        .map(bound_context_string);
    }
    if ctx.model_version.is_none() {
        ctx.model_version = header_str_any(
            headers,
            &["x-dcc-mcp-agent-model-version", "x-dcc-mcp-model-version"],
        )
        .map(bound_context_string);
    }
    if ctx.model.is_none() {
        ctx.model = header_str(headers, "x-dcc-mcp-agent-model").map(bound_context_string);
    }
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
    if ctx.client_platform.is_none() {
        ctx.client_platform =
            header_str(headers, "x-dcc-mcp-client-platform").map(bound_context_string);
    }
    if ctx.client_os.is_none() {
        ctx.client_os = header_str(headers, "x-dcc-mcp-client-os").map(bound_context_string);
    }
    if ctx.client_host.is_none() {
        ctx.client_host = header_str(headers, "x-dcc-mcp-client-host").map(bound_context_string);
    }
    if ctx.auth_subject.is_none() {
        ctx.auth_subject = header_str(headers, "x-dcc-mcp-auth-subject").map(bound_context_string);
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
            | "chainofthought"
            | "hiddencot"
            | "messages"
            | "prompt"
            | "prompts"
            | "rawagentreply"
            | "rawagentresponse"
            | "rawprompt"
            | "rawresponse"
            | "rawuserinput"
            | "reply"
            | "response"
            | "userinput"
    ) || normalised.contains("secret")
}

// ── Span ──────────────────────────────────────────────────────────────────────

/// One timed segment within a [`DispatchTrace`] waterfall.
///
/// Span names follow the convention described in issue #863 Phase 2:
/// `gateway.received`, `middleware.before`, `gateway.route`,
/// `backend.dispatch`, `backend.execute`, `backend.response_decode`,
/// `middleware.after`, `gateway.response`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSpan {
    /// Unique span id for this segment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    /// Parent span id, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    /// Segment label (e.g. `"backend.dispatch"`).
    pub name: String,
    /// Nanoseconds since Unix epoch when this span started.
    pub started_ns: u64,
    /// Wall-clock duration of this span in nanoseconds.
    pub duration_ns: u64,
    /// Whether this segment completed without error.
    pub ok: bool,
    /// Span-specific attributes (e.g. `mcp_url`, `bytes_sent`).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, Value>,
}

impl TraceSpan {
    pub fn new(name: impl Into<String>, started_ns: u64, duration_ns: u64) -> Self {
        Self {
            span_id: Some(new_span_id()),
            parent_span_id: None,
            name: name.into(),
            started_ns,
            duration_ns,
            ok: true,
            attributes: HashMap::new(),
        }
    }

    pub fn with_error(mut self) -> Self {
        self.ok = false;
        self
    }

    pub fn with_attr(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

// ── Trace ─────────────────────────────────────────────────────────────────────

/// Full per-call dispatch trace stored in the admin ring buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchTrace {
    /// Matches the JSON-RPC `id` string used throughout the call.
    pub request_id: String,
    /// End-to-end trace id shared by related requests.
    #[serde(default = "new_trace_id")]
    pub trace_id: String,
    /// Root gateway span id for this request, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    /// Parent span id from incoming trace context, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    /// Parent request id for request-chain correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    /// W3C trace flags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_flags: Option<String>,
    /// W3C tracestate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_state: Option<String>,
    /// MCP method (e.g. `"tools/call"`, `"tools/list"`).
    pub method: String,
    /// Tool slug from `params.name` (present for `tools/call`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_slug: Option<String>,
    /// Target instance UUID as a hex string (present after routing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// Session that originated the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// DCC type of the target backend (e.g. `"maya"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,
    /// Transport surface that produced this trace, such as `"mcp"` or `"rest"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    /// Optional agent/caller context supplied by the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_context: Option<AgentContext>,
    /// Wall-clock time when the call entered the gateway handler.
    #[serde(with = "timestamp_serde")]
    pub started_at: SystemTime,
    /// Total gateway wall-clock latency in milliseconds (0 if not yet complete).
    pub total_ms: u64,
    /// Whether the call completed without error.
    pub ok: bool,
    /// Waterfall of timing segments.
    pub spans: Vec<TraceSpan>,
    /// Captured `params.arguments` (redacted, bounded to [`MAX_INPUT_BYTES`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<TracePayload>,
    /// Captured response content (bounded to [`MAX_OUTPUT_BYTES`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<TracePayload>,
    /// Token accounting for the client-visible response, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_accounting: Option<TokenTelemetry>,
}

impl DispatchTrace {
    pub fn span_count(&self) -> usize {
        self.spans.len()
    }

    pub fn input_bytes(&self) -> Option<usize> {
        self.input.as_ref().map(|p| p.original_size)
    }

    pub fn output_bytes(&self) -> Option<usize> {
        self.output.as_ref().map(|p| p.original_size)
    }

    pub fn input_tokens(&self) -> Option<usize> {
        self.input.as_ref().and_then(|p| p.estimated_tokens)
    }

    pub fn output_tokens(&self) -> Option<usize> {
        self.output.as_ref().and_then(|p| p.estimated_tokens)
    }

    pub fn total_tokens(&self) -> Option<usize> {
        match (self.input_tokens(), self.output_tokens()) {
            (Some(input), Some(output)) => Some(input.saturating_add(output)),
            (Some(input), None) => Some(input),
            (None, Some(output)) => Some(output),
            (None, None) => None,
        }
    }

    pub fn slowest_span(&self) -> Option<(&TraceSpan, u64)> {
        self.spans
            .iter()
            .max_by_key(|span| span.duration_ns)
            .map(|span| (span, span.duration_ns / 1_000_000))
    }
}

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

mod timestamp_serde {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let ms = t
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;
        ms.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + Duration::from_millis(ms))
    }
}

// ── Ring buffer ───────────────────────────────────────────────────────────────

/// Bounded ring buffer of completed traces.
pub struct TraceLog {
    buf: Mutex<Vec<DispatchTrace>>,
    capacity: usize,
}

impl TraceLog {
    pub const DEFAULT_CAPACITY: usize = 200;

    pub fn new(capacity: usize) -> Self {
        Self {
            buf: Mutex::new(Vec::with_capacity(capacity.min(TraceLog::DEFAULT_CAPACITY))),
            capacity,
        }
    }

    /// Seed the in-memory ring from durable storage.
    pub fn extend(&self, traces: impl IntoIterator<Item = DispatchTrace>) {
        for trace in traces {
            self.push(trace);
        }
    }

    /// Append a completed trace, evicting the oldest entry if at capacity.
    pub fn push(&self, trace: DispatchTrace) {
        let mut buf = self.buf.lock();
        buf.push(trace);
        while self.capacity > 0 && buf.len() > self.capacity {
            buf.remove(0);
        }
    }

    /// Return the last `limit` traces, newest first.
    pub fn recent(&self, limit: usize) -> Vec<DispatchTrace> {
        let buf = self.buf.lock();
        buf.iter().rev().take(limit).cloned().collect()
    }

    /// Fetch a single trace by `request_id`.
    pub fn get(&self, request_id: &str) -> Option<DispatchTrace> {
        self.buf
            .lock()
            .iter()
            .rev()
            .find(|t| t.request_id == request_id)
            .cloned()
    }

    /// Fetch traces by trace id, newest first.
    pub fn by_trace_id(&self, trace_id: &str, limit: usize) -> Vec<DispatchTrace> {
        self.buf
            .lock()
            .iter()
            .rev()
            .filter(|t| t.trace_id == trace_id)
            .take(limit)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_truncates_at_cap() {
        let big = json!({"data": "a".repeat(100)});
        let p = TracePayload::from_value(&big, 50);
        assert!(p.truncated);
        assert!(p.content.len() <= 50);
        assert!(p.original_size > 50);
    }

    #[test]
    fn payload_estimates_tokens_for_json() {
        let p = TracePayload::from_value(&json!("hello world"), 1024);
        assert!(p.estimated_tokens.is_some());
        assert!(p.estimated_tokens.unwrap() > 0);
    }

    #[test]
    fn payload_no_truncation_when_under_cap() {
        let small = json!({"x": 1});
        let p = TracePayload::from_value(&small, 1024);
        assert!(!p.truncated);
        assert_eq!(p.original_size, p.content.len());
    }

    #[test]
    fn input_payload_redacts_script_source_fields() {
        let raw = json!({
            "tool_slug": "maya.abc.execute_python",
            "arguments": {
                "code": "print('secret')",
                "nested": {
                    "content": "raw script body"
                },
                "file_path": "/tmp/materialized.py"
            }
        });

        let p = TracePayload::from_input_value(&raw, 4096);

        assert!(p.content.contains("[REDACTED_SCRIPT_SOURCE]"));
        assert!(!p.content.contains("print('secret')"));
        assert!(!p.content.contains("raw script body"));
        assert!(p.content.contains("/tmp/materialized.py"));
    }

    #[test]
    fn agent_context_reads_meta_and_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-dcc-mcp-actor-id", "user-7".parse().unwrap());
        headers.insert("x-dcc-mcp-actor-name", "Morgan Artist".parse().unwrap());
        headers.insert("x-dcc-mcp-agent-id", "agent-7".parse().unwrap());
        headers.insert("x-dcc-mcp-agent-version", "0.9.0".parse().unwrap());
        headers.insert("x-dcc-mcp-agent-model", "gpt-test".parse().unwrap());
        headers.insert("x-dcc-mcp-agent-model-provider", "openai".parse().unwrap());
        headers.insert("x-dcc-mcp-agent-turn-id", "turn-9".parse().unwrap());
        headers.insert("x-dcc-mcp-client-platform", "custom-http".parse().unwrap());
        headers.insert("x-dcc-mcp-client-os", "windows".parse().unwrap());
        headers.insert("x-dcc-mcp-auth-subject", "apikey:team-a".parse().unwrap());
        headers.insert("x-dcc-mcp-user-input-chars", "2500".parse().unwrap());
        let meta = json!({
            "agent_context": {
                "actorEmailHash": "sha256:actor",
                "agent_name": "Scene Planner",
                "modelVersion": "gpt-5.1",
                "reasoningEffort": "medium",
                "sessionId": "session-meta",
                "task": "inspect material bindings",
                "userIntentSummary": "Inspect scene before editing.",
                "agentReplySummary": "I will inspect the scene graph first.",
                "userInputHash": "sha256:user",
                "agentReplyHash": "sha256:reply",
                "agentReplyChars": 140,
                "reasoning_summary": "Need a lightweight scene read before edit.",
                "plan": ["describe scene", "choose material patch"]
            }
        });

        let ctx = AgentContext::from_request_parts(&headers, None, Some(&meta)).unwrap();

        assert_eq!(ctx.actor_id.as_deref(), Some("user-7"));
        assert_eq!(ctx.actor_name.as_deref(), Some("Morgan Artist"));
        assert_eq!(ctx.actor_email_hash.as_deref(), Some("sha256:actor"));
        assert_eq!(ctx.agent_id.as_deref(), Some("agent-7"));
        assert_eq!(ctx.agent_name.as_deref(), Some("Scene Planner"));
        assert_eq!(ctx.agent_version.as_deref(), Some("0.9.0"));
        assert_eq!(ctx.model.as_deref(), Some("gpt-test"));
        assert_eq!(ctx.model_provider.as_deref(), Some("openai"));
        assert_eq!(ctx.model_version.as_deref(), Some("gpt-5.1"));
        assert_eq!(ctx.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(ctx.session_id.as_deref(), Some("session-meta"));
        assert_eq!(ctx.turn_id.as_deref(), Some("turn-9"));
        assert_eq!(
            ctx.user_intent_summary.as_deref(),
            Some("Inspect scene before editing.")
        );
        assert_eq!(
            ctx.agent_reply_summary.as_deref(),
            Some("I will inspect the scene graph first.")
        );
        assert_eq!(ctx.client_platform.as_deref(), Some("custom-http"));
        assert_eq!(ctx.client_os.as_deref(), Some("windows"));
        assert_eq!(ctx.auth_subject.as_deref(), Some("apikey:team-a"));
        assert_eq!(ctx.user_input_hash.as_deref(), Some("sha256:user"));
        assert_eq!(ctx.agent_reply_hash.as_deref(), Some("sha256:reply"));
        assert_eq!(ctx.user_input_chars, Some(2500));
        assert_eq!(ctx.agent_reply_chars, Some(140));
        assert_eq!(ctx.plan.len(), 2);
        assert_eq!(ctx.display_name(), Some("Morgan Artist"));
    }

    #[test]
    fn agent_context_accepts_plain_summary() {
        let headers = HeaderMap::new();
        let body = json!({"caller_context": "manual smoke test"});

        let ctx = AgentContext::from_request_parts(&headers, Some(&body), None).unwrap();

        assert_eq!(ctx.reasoning_summary.as_deref(), Some("manual smoke test"));
        assert_eq!(ctx.display_name(), None);
    }

    #[test]
    fn caller_attribution_handles_missing_partial_and_malformed_metadata() {
        let headers = HeaderMap::new();
        assert!(AgentContext::from_request_parts(&headers, None, None).is_none());

        let partial = json!({
            "caller_context": {
                "actor_id": "artist-1"
            }
        });
        let partial_ctx = AgentContext::from_request_parts(&headers, Some(&partial), None).unwrap();
        assert_eq!(partial_ctx.actor_id.as_deref(), Some("artist-1"));
        assert_eq!(partial_ctx.agent_id, None);
        assert_eq!(partial_ctx.source_ip, None);

        let mut header_fallback = HeaderMap::new();
        header_fallback.insert("x-dcc-mcp-client-platform", "studio-tool".parse().unwrap());
        let malformed = json!({
            "caller_context": {
                "actor_id": { "nested": "not a string" }
            }
        });
        let ctx =
            AgentContext::from_request_parts(&header_fallback, Some(&malformed), None).unwrap();
        assert_eq!(ctx.actor_id, None);
        assert_eq!(ctx.client_platform.as_deref(), Some("studio-tool"));
    }

    #[test]
    fn caller_attribution_bounds_fields_and_ignores_client_network_source() {
        let headers = HeaderMap::new();
        let long_actor = "artist".repeat(MAX_AGENT_CONTEXT_STRING_BYTES);
        let body = json!({
            "caller_context": {
                "actorId": long_actor,
                "actorName": "Morgan Artist",
                "agentId": "agent-camel",
                "agentVersion": "1.2.3",
                "agentModel": "gpt-test",
                "clientPlatform": "cursor",
                "clientOs": "macos",
                "clientHost": "workstation-42",
                "authSubject": "oauth:user-7",
                "sourceIp": "203.0.113.99",
                "forwardedFor": ["198.51.100.10"]
            }
        });

        let ctx = AgentContext::from_request_parts(&headers, Some(&body), None).unwrap();

        assert!(
            ctx.actor_id.as_ref().unwrap().len() <= MAX_AGENT_CONTEXT_STRING_BYTES,
            "actor_id should be bounded"
        );
        assert_eq!(ctx.actor_name.as_deref(), Some("Morgan Artist"));
        assert_eq!(ctx.agent_id.as_deref(), Some("agent-camel"));
        assert_eq!(ctx.agent_version.as_deref(), Some("1.2.3"));
        assert_eq!(ctx.model.as_deref(), Some("gpt-test"));
        assert_eq!(ctx.client_platform.as_deref(), Some("cursor"));
        assert_eq!(ctx.client_os.as_deref(), Some("macos"));
        assert_eq!(ctx.client_host.as_deref(), Some("workstation-42"));
        assert_eq!(ctx.auth_subject.as_deref(), Some("oauth:user-7"));
        assert_eq!(ctx.source_ip, None, "source_ip must be server-derived");
        assert!(
            ctx.forwarded_for.is_empty(),
            "forwarded_for must be server-derived"
        );
    }

    #[test]
    fn caller_attribution_network_source_can_be_added_by_server_boundary() {
        let ctx = AgentContext {
            actor_id: Some("user-7".to_string()),
            ..AgentContext::default()
        }
        .with_server_network_source(
            Some("192.0.2.44".to_string()),
            vec!["198.51.100.2".to_string(), "203.0.113.3".to_string()],
        );

        assert_eq!(ctx.source_ip.as_deref(), Some("192.0.2.44"));
        assert_eq!(
            ctx.forwarded_for,
            vec!["198.51.100.2".to_string(), "203.0.113.3".to_string()]
        );
    }

    #[test]
    fn agent_context_bounds_turn_summaries_and_excludes_raw_text() {
        let headers = HeaderMap::new();
        let raw_prompt = "secret production prompt".to_string();
        let long_summary = "summary ".repeat(MAX_AGENT_CONTEXT_STRING_BYTES);
        let body = json!({
            "caller_context": {
                "agent_id": "agent-raw",
                "turnId": "turn-raw",
                "userIntentSummary": long_summary,
                "user_input": raw_prompt,
                "agentReply": "raw reply should not be stored",
                "metadata": {
                    "workflow_id": "wf-1",
                    "prompt": "raw prompt in metadata",
                    "nested": {
                        "rawAgentReply": "raw reply in nested metadata",
                        "safe": "kept"
                    }
                }
            }
        });

        let ctx = AgentContext::from_request_parts(&headers, Some(&body), None).unwrap();
        let encoded = serde_json::to_string(&ctx).unwrap();

        assert!(ctx.user_intent_summary.unwrap().len() <= MAX_AGENT_CONTEXT_STRING_BYTES);
        assert!(!encoded.contains("secret production prompt"));
        assert!(!encoded.contains("raw reply should not be stored"));
        assert!(!encoded.contains("raw prompt in metadata"));
        assert!(!encoded.contains("raw reply in nested metadata"));
        assert_eq!(ctx.metadata["workflow_id"], "wf-1");
        assert_eq!(ctx.metadata["nested"]["safe"], "kept");
        assert_eq!(ctx.metadata["redacted_high_sensitivity_fields"], 1);
        assert_eq!(
            ctx.metadata["nested"]["redacted_high_sensitivity_fields"],
            1
        );
    }

    #[test]
    fn trace_context_parses_w3c_traceparent_without_using_it_as_request_id() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", "req-explicit".parse().unwrap());
        headers.insert(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
                .parse()
                .unwrap(),
        );
        headers.insert("tracestate", "vendor=value".parse().unwrap());

        let ctx = TraceContext::from_headers(&headers);

        assert_eq!(ctx.request_id, "req-explicit");
        assert_eq!(ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(ctx.parent_span_id.as_deref(), Some("00f067aa0ba902b7"));
        assert_eq!(ctx.trace_flags.as_deref(), Some("01"));
        assert_eq!(ctx.trace_state.as_deref(), Some("vendor=value"));
    }

    #[test]
    fn trace_context_generates_ids_when_headers_are_absent() {
        let ctx = TraceContext::from_headers(&HeaderMap::new());

        assert_eq!(ctx.trace_id.len(), 32);
        assert_eq!(ctx.span_id.as_deref().unwrap_or_default().len(), 16);
        assert!(!ctx.request_id.is_empty());
        assert!(ctx.traceparent().is_some());
    }

    #[test]
    fn trace_log_evicts_oldest_at_capacity() {
        let log = TraceLog::new(3);
        for i in 0u32..5 {
            log.push(DispatchTrace {
                request_id: format!("req-{i}"),
                trace_id: "trace-ring".into(),
                span_id: None,
                parent_span_id: None,
                parent_request_id: None,
                trace_flags: None,
                trace_state: None,
                method: "tools/call".into(),
                tool_slug: None,
                instance_id: None,
                session_id: None,
                dcc_type: None,
                transport: None,
                agent_context: None,
                started_at: SystemTime::now(),
                total_ms: i as u64,
                ok: true,
                spans: vec![],
                input: None,
                output: None,
                token_accounting: None,
            });
        }
        let recent = log.recent(10);
        assert_eq!(recent.len(), 3);
        // Newest first.
        assert_eq!(recent[0].request_id, "req-4");
        assert_eq!(recent[2].request_id, "req-2");
    }

    #[test]
    fn trace_log_get_by_request_id() {
        let log = TraceLog::new(10);
        log.push(DispatchTrace {
            request_id: "abc-123".into(),
            trace_id: "trace-abc".into(),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: Some("maya.create_sphere".into()),
            instance_id: None,
            session_id: None,
            dcc_type: Some("maya".into()),
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms: 42,
            ok: true,
            spans: vec![],
            input: None,
            output: None,
            token_accounting: None,
        });
        let found = log.get("abc-123");
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().tool_slug.as_deref(),
            Some("maya.create_sphere")
        );
        assert!(log.get("unknown").is_none());
    }

    // ── Property-based tests (#846) ────────────────────────────────────────

    use proptest::prelude::*;

    fn arb_trace(idx: u32) -> DispatchTrace {
        DispatchTrace {
            request_id: format!("req-{idx}"),
            trace_id: format!("trace-{idx}"),
            span_id: None,
            parent_span_id: None,
            parent_request_id: None,
            trace_flags: None,
            trace_state: None,
            method: "tools/call".into(),
            tool_slug: None,
            instance_id: None,
            session_id: None,
            dcc_type: None,
            transport: None,
            agent_context: None,
            started_at: SystemTime::now(),
            total_ms: idx as u64,
            ok: true,
            spans: vec![],
            input: None,
            output: None,
            token_accounting: None,
        }
    }

    proptest! {
        /// Ring-buffer law: after pushing `pushes` traces into a buffer of
        /// capacity `capacity`, `recent(usize::MAX).len() == min(pushes, capacity)`.
        /// Proves the buffer never exceeds capacity (memory bound) and never
        /// drops more than necessary.
        #[test]
        fn prop_trace_log_capacity_is_respected(
            capacity in 1usize..32,
            pushes in 0u32..64,
        ) {
            let log = TraceLog::new(capacity);
            for i in 0..pushes {
                log.push(arb_trace(i));
            }
            let recent = log.recent(usize::MAX);
            let expected = (pushes as usize).min(capacity);
            prop_assert_eq!(recent.len(), expected);
        }

        /// Ring-buffer law: `recent(limit)` always returns ≤ `limit` items
        /// and ≤ buffer occupancy. First item is the most recently pushed
        /// trace (LIFO order).
        #[test]
        fn prop_trace_log_recent_returns_newest_first(
            capacity in 1usize..16,
            pushes in 1u32..32,
            limit in 1usize..32,
        ) {
            let log = TraceLog::new(capacity);
            for i in 0..pushes {
                log.push(arb_trace(i));
            }
            let recent = log.recent(limit);
            let bound = limit.min((pushes as usize).min(capacity));
            prop_assert_eq!(recent.len(), bound);
            if !recent.is_empty() {
                prop_assert_eq!(
                    &recent[0].request_id,
                    &format!("req-{}", pushes - 1)
                );
            }
        }
    }
}
