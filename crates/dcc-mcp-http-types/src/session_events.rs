//! Wire types for bounded adapter session/job observability events.
//!
//! These types are intentionally transport-neutral: DCC adapters can append
//! events from in-process hosts, sidecars, or gateway-routed jobs, while MCP
//! and REST surfaces can expose the same JSON shape by cursor.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Event severity, modelled after MCP logging levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventLevel {
    /// Very fine-grained diagnostic detail.
    Trace,
    /// Developer-oriented diagnostic information.
    Debug,
    /// Informational runtime output.
    #[default]
    Info,
    /// Warning output that does not necessarily fail the tool call.
    Warning,
    /// Error output, exception text, or failed runtime checkpoint.
    Error,
}

impl SessionEventLevel {
    /// Parse a level string case-insensitively.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warning" | "warn" => Some(Self::Warning),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Return the stable wire value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Metadata describing a truncated event message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEventTruncation {
    /// Original UTF-8 byte length before truncation.
    pub original_size: usize,
    /// Returned UTF-8 byte length after truncation.
    pub truncated_size: usize,
    /// Configured maximum UTF-8 byte length.
    pub max_size: usize,
}

/// One adapter-published runtime observation event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    /// Monotonic cursor id assigned by the buffer.
    pub id: u64,
    /// Unix epoch nanoseconds when the event was appended.
    pub timestamp_ns: u128,
    /// DCC instance identifier when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// MCP/session identifier when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Tool call/request id that produced the event, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Async job id that produced the event, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    /// Adapter-defined correlation id for cross-system tracing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Event producer, such as `python`, `host`, `script_editor`, or `adapter`.
    pub source: String,
    /// Output stream or channel, such as `stdout`, `stderr`, `progress`, or `log`.
    pub stream: String,
    /// Event severity.
    pub level: SessionEventLevel,
    /// Short human-readable message.
    pub message: String,
    /// Truncation metadata when `message` exceeded the buffer limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<SessionEventTruncation>,
    /// Adapter-defined structured metadata.
    #[serde(default)]
    pub metadata: Value,
}

impl SessionEvent {
    /// Create a new event before it is assigned a cursor by a buffer.
    #[must_use]
    pub fn new(
        source: impl Into<String>,
        stream: impl Into<String>,
        level: SessionEventLevel,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: 0,
            timestamp_ns: now_ns(),
            instance_id: None,
            session_id: None,
            tool_call_id: None,
            job_id: None,
            correlation_id: None,
            source: source.into(),
            stream: stream.into(),
            level,
            message: message.into(),
            truncation: None,
            metadata: Value::Null,
        }
    }

    /// Attach structured metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Attach a tool-call/request id.
    #[must_use]
    pub fn with_tool_call_id(mut self, id: impl Into<String>) -> Self {
        self.tool_call_id = Some(id.into());
        self
    }

    /// Attach an async job id.
    #[must_use]
    pub fn with_job_id(mut self, id: impl Into<String>) -> Self {
        self.job_id = Some(id.into());
        self
    }
}

/// Cursor read options for a session event buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionEventReadOptions {
    /// Return events whose id is greater than this cursor.
    pub cursor: u64,
    /// Maximum number of events to return.
    pub limit: usize,
    /// When true, discard returned-and-older events after reading.
    pub drain: bool,
}

impl Default for SessionEventReadOptions {
    fn default() -> Self {
        Self {
            cursor: 0,
            limit: 100,
            drain: false,
        }
    }
}

/// A cursor page returned by a session event buffer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEventPage {
    /// Cursor supplied by the caller.
    pub cursor: u64,
    /// Cursor to supply on the next read to avoid duplicate events.
    pub next_cursor: u64,
    /// Oldest event id still retained in the buffer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_cursor: Option<u64>,
    /// Newest event id still retained in the buffer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newest_cursor: Option<u64>,
    /// Number of events currently retained after this read.
    pub retained_count: usize,
    /// Number of events dropped by retention or drain since buffer creation.
    pub dropped_count: u64,
    /// Returned events.
    pub events: Vec<SessionEvent>,
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_level_parses_common_values() {
        assert_eq!(
            SessionEventLevel::parse("warn"),
            Some(SessionEventLevel::Warning)
        );
        assert_eq!(
            SessionEventLevel::parse("ERROR"),
            Some(SessionEventLevel::Error)
        );
        assert_eq!(SessionEventLevel::parse("verbose"), None);
    }

    #[test]
    fn session_event_serializes_correlation_fields() {
        let event = SessionEvent::new("python", "stdout", SessionEventLevel::Info, "hello")
            .with_tool_call_id("req-1")
            .with_job_id("job-1")
            .with_metadata(serde_json::json!({"frame": 12}));

        let encoded = serde_json::to_value(&event).unwrap();
        assert_eq!(encoded["tool_call_id"], "req-1");
        assert_eq!(encoded["job_id"], "job-1");
        assert_eq!(encoded["metadata"]["frame"], 12);

        let decoded: SessionEvent = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, event);
    }
}
