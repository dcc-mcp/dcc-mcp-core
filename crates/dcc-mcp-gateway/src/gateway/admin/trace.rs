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
}

impl TracePayload {
    /// Build a `TracePayload`, truncating at `cap` bytes if necessary.
    pub fn from_value(v: &Value, cap: usize) -> Self {
        let raw = serde_json::to_string(v).unwrap_or_default();
        let original_size = raw.len();
        let truncated = original_size > cap;
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
        }
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
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
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
        self.agent_name
            .as_deref()
            .or(self.agent_id.as_deref())
            .or(self.agent_kind.as_deref())
    }

    fn is_empty(&self) -> bool {
        self.agent_id.is_none()
            && self.agent_name.is_none()
            && self.agent_kind.is_none()
            && self.model.is_none()
            && self.task.is_none()
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
        self.agent_id = self.agent_id.map(bound_context_string);
        self.agent_name = self.agent_name.map(bound_context_string);
        self.agent_kind = self.agent_kind.map(bound_context_string);
        self.model = self.model.map(bound_context_string);
        self.task = self.task.map(bound_context_string);
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
    if ctx.agent_id.is_none() {
        ctx.agent_id = header_str(headers, "x-dcc-mcp-agent-id").map(bound_context_string);
    }
    if ctx.agent_name.is_none() {
        ctx.agent_name = header_str(headers, "x-dcc-mcp-agent-name").map(bound_context_string);
    }
    if ctx.agent_kind.is_none() {
        ctx.agent_kind = header_str(headers, "x-dcc-mcp-agent-kind").map(bound_context_string);
    }
    if ctx.model.is_none() {
        ctx.model = header_str(headers, "x-dcc-mcp-agent-model").map(bound_context_string);
    }
    if ctx.task.is_none() {
        ctx.task = header_str(headers, "x-dcc-mcp-agent-task").map(bound_context_string);
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
    let raw = serde_json::to_string(&value).unwrap_or_default();
    if raw.len() <= MAX_AGENT_CONTEXT_METADATA_BYTES {
        return value;
    }
    let (preview, _) = truncate_utf8(raw.clone(), MAX_AGENT_CONTEXT_METADATA_BYTES);
    json!({
        "truncated": true,
        "original_size": raw.len(),
        "preview": preview,
    })
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
    fn payload_no_truncation_when_under_cap() {
        let small = json!({"x": 1});
        let p = TracePayload::from_value(&small, 1024);
        assert!(!p.truncated);
        assert_eq!(p.original_size, p.content.len());
    }

    #[test]
    fn agent_context_reads_meta_and_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-dcc-mcp-agent-id", "agent-7".parse().unwrap());
        headers.insert("x-dcc-mcp-agent-model", "gpt-test".parse().unwrap());
        let meta = json!({
            "agent_context": {
                "agent_name": "Scene Planner",
                "task": "inspect material bindings",
                "reasoning_summary": "Need a lightweight scene read before edit.",
                "plan": ["describe scene", "choose material patch"]
            }
        });

        let ctx = AgentContext::from_request_parts(&headers, None, Some(&meta)).unwrap();

        assert_eq!(ctx.agent_id.as_deref(), Some("agent-7"));
        assert_eq!(ctx.agent_name.as_deref(), Some("Scene Planner"));
        assert_eq!(ctx.model.as_deref(), Some("gpt-test"));
        assert_eq!(ctx.plan.len(), 2);
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
    fn trace_log_evicts_oldest_at_capacity() {
        let log = TraceLog::new(3);
        for i in 0u32..5 {
            log.push(DispatchTrace {
                request_id: format!("req-{i}"),
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
