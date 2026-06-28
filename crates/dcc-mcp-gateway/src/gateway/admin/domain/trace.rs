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
//!
//! Agent/caller context types were extracted into `domain/agent_context.rs` to
//! keep this file under the 1500-line hard limit. They are re-exported here so
//! the public path `crate::gateway::admin::trace::*` is unchanged.

use std::collections::HashMap;
use std::time::SystemTime;

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::gateway::admin::trace_log::TraceLog;

// Re-export agent context types from the split module.
pub use super::agent_context::{
    AgentContext, AgentContextTrust, TRUST_AUTH, TRUST_HEADER, TRUST_SELF_REPORTED,
    TRUST_SERVER_DERIVED, TRUST_TRUSTED_PROXY,
};

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

    pub fn child_request(&self, request_id: impl Into<String>) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            request_id: request_id.into(),
            span_id: Some(new_span_id()),
            parent_span_id: self.span_id.clone(),
            parent_request_id: Some(self.request_id.clone()),
            trace_flags: self.trace_flags.clone(),
            trace_state: self.trace_state.clone(),
        }
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

// ── LLM usage ────────────────────────────────────────────────────────────────

/// Optional upstream LLM billing token counts provided by the client via
/// the `x-dcc-mcp-llm-usage` header. Stored alongside the gateway's own
/// byte4 token estimation but NEVER aggregated with it — the two
/// estimators measure different things and must stay separate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmUsage {
    /// Input/prompt tokens charged by the LLM provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    /// Output/completion tokens charged by the LLM provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    /// Total tokens (provider-supplied or prompt+completion sum).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    /// Model identifier (e.g. `"claude-opus-4-6"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl LlmUsage {
    /// Parse a compact JSON object from the `x-dcc-mcp-llm-usage` header.
    pub fn from_header_value(value: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(value).ok()?;
        let obj = v.as_object()?;
        if obj.is_empty() {
            return None;
        }
        Some(Self {
            prompt_tokens: obj.get("prompt_tokens").and_then(Value::as_u64),
            completion_tokens: obj.get("completion_tokens").and_then(Value::as_u64),
            total_tokens: obj.get("total_tokens").and_then(Value::as_u64),
            model: obj.get("model").and_then(Value::as_str).map(str::to_string),
        })
    }
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
    /// Optional upstream LLM billing token counts from `x-dcc-mcp-llm-usage`.
    /// Kept separate from `token_accounting` — they measure different things.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_usage: Option<LlmUsage>,
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

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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

#[cfg(test)]
#[path = "../trace_tests.rs"]
mod tests;
