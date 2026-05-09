//! Per-call dispatch trace types for the Admin UI `/api/traces` endpoint.
//!
//! Every `tools/call` routed through the gateway produces one [`DispatchTrace`]
//! that records a waterfall of [`TraceSpan`]s (gateway → middleware → backend →
//! response) plus optionally the raw request/response payloads (bounded and
//! pre-redacted by [`RedactionMiddleware`]).
//!
//! The ring buffer (`TraceLog`) lives in [`AdminState`] and is populated by
//! [`TraceSink`] which is called from `AuditMiddleware::after_call`.

use std::collections::HashMap;
use std::time::SystemTime;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Payload capture ───────────────────────────────────────────────────────────

/// Hard limits for payload capture (bytes, not tokens).
pub const MAX_INPUT_BYTES: usize = 16 * 1024; // 16 KB
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024; // 64 KB

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
    /// Wall-clock time when the call entered the gateway handler.
    #[serde(with = "timestamp_serde")]
    pub started_at: SystemTime,
    /// Total gateway wall-clock latency in milliseconds (0 if not yet complete).
    pub total_ms: u64,
    /// Whether the call completed without error.
    pub ok: bool,
    /// Waterfall of timing segments.
    pub spans: Vec<TraceSpan>,
    /// Captured `params.arguments` (pre-redacted, bounded to [`MAX_INPUT_BYTES`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<TracePayload>,
    /// Captured response content (pre-redacted, bounded to [`MAX_OUTPUT_BYTES`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<TracePayload>,
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
}
