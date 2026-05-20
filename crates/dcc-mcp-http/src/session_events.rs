//! Bounded adapter session/job observability events.
//!
//! `SessionEventBuffer` is the runtime counterpart to
//! `dcc_mcp_http_types::session_events`: adapters append short diagnostic
//! events, and MCP clients read them incrementally through an `events://`
//! resource using a cursor.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::resources::{ProducerContent, ResourceError, ResourceProducer, ResourceResult};
pub use dcc_mcp_http_types::session_events::SessionEventLevel;
use dcc_mcp_http_types::session_events::{
    SessionEvent, SessionEventPage, SessionEventReadOptions, SessionEventTruncation,
};
use dcc_mcp_jsonrpc::McpResource;
use parking_lot::RwLock;

/// Default number of events retained per buffer.
pub const DEFAULT_SESSION_EVENT_CAPACITY: usize = 1000;
/// Default maximum UTF-8 message bytes retained per event.
pub const DEFAULT_SESSION_EVENT_MAX_MESSAGE_BYTES: usize = 4096;
/// Default page size for resource reads.
pub const DEFAULT_SESSION_EVENT_READ_LIMIT: usize = 100;
/// Hard cap for one resource read.
pub const MAX_SESSION_EVENT_READ_LIMIT: usize = 1000;

/// Thread-safe bounded event buffer.
#[derive(Clone)]
pub struct SessionEventBuffer {
    inner: Arc<SessionEventBufferInner>,
}

struct SessionEventBufferInner {
    ring: RwLock<VecDeque<SessionEvent>>,
    capacity: usize,
    max_message_bytes: usize,
    next_id: AtomicU64,
    dropped_count: AtomicU64,
    instance_id: String,
}

impl SessionEventBuffer {
    /// Create a buffer with default capacity and message limit.
    #[must_use]
    pub fn new(instance_id: impl Into<String>) -> Self {
        Self::with_limits(
            instance_id,
            DEFAULT_SESSION_EVENT_CAPACITY,
            DEFAULT_SESSION_EVENT_MAX_MESSAGE_BYTES,
        )
    }

    /// Create a buffer with explicit capacity and message limit.
    #[must_use]
    pub fn with_limits(
        instance_id: impl Into<String>,
        capacity: usize,
        max_message_bytes: usize,
    ) -> Self {
        let capacity = capacity.max(1);
        let instance_id = instance_id.into();
        Self {
            inner: Arc::new(SessionEventBufferInner {
                ring: RwLock::new(VecDeque::with_capacity(capacity)),
                capacity,
                max_message_bytes,
                next_id: AtomicU64::new(1),
                dropped_count: AtomicU64::new(0),
                instance_id,
            }),
        }
    }

    /// Append a structured event and return the stored copy.
    pub fn append(&self, mut event: SessionEvent) -> SessionEvent {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        event.id = id;
        if event.instance_id.is_none() {
            event.instance_id = Some(self.inner.instance_id.clone());
        }
        let (message, truncation) = truncate_message(event.message, self.inner.max_message_bytes);
        event.message = message;
        event.truncation = truncation;

        {
            let mut ring = self.inner.ring.write();
            ring.push_back(event.clone());
            while ring.len() > self.inner.capacity {
                ring.pop_front();
                self.inner.dropped_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        event
    }

    /// Append a text event with common fields.
    pub fn append_text(
        &self,
        source: impl Into<String>,
        stream: impl Into<String>,
        level: SessionEventLevel,
        message: impl Into<String>,
    ) -> SessionEvent {
        self.append(SessionEvent::new(source, stream, level, message))
    }

    /// Read a cursor page from the buffer.
    #[must_use]
    pub fn read(&self, options: SessionEventReadOptions) -> SessionEventPage {
        let limit = normalized_limit(options.limit);
        let mut ring = self.inner.ring.write();
        let events: Vec<SessionEvent> = ring
            .iter()
            .filter(|event| event.id > options.cursor)
            .take(limit)
            .cloned()
            .collect();
        let next_cursor = events
            .last()
            .map(|event| event.id)
            .unwrap_or(options.cursor);

        if options.drain && next_cursor > 0 {
            let before = ring.len();
            ring.retain(|event| event.id > next_cursor);
            let removed = before.saturating_sub(ring.len()) as u64;
            if removed > 0 {
                self.inner
                    .dropped_count
                    .fetch_add(removed, Ordering::Relaxed);
            }
        }

        let oldest_cursor = ring.front().map(|event| event.id);
        let newest_cursor = ring.back().map(|event| event.id);
        SessionEventPage {
            cursor: options.cursor,
            next_cursor,
            oldest_cursor,
            newest_cursor,
            retained_count: ring.len(),
            dropped_count: self.inner.dropped_count.load(Ordering::Relaxed),
            events,
        }
    }

    /// Number of retained events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.ring.read().len()
    }

    /// Whether the buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// DCC instance id associated with this buffer.
    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.inner.instance_id
    }

    /// MCP resource URI for this buffer.
    #[must_use]
    pub fn resource_uri(&self) -> String {
        format!("events://session/{}", self.inner.instance_id)
    }
}

impl std::fmt::Debug for SessionEventBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionEventBuffer")
            .field("instance_id", &self.inner.instance_id)
            .field("len", &self.len())
            .field("capacity", &self.inner.capacity)
            .field("max_message_bytes", &self.inner.max_message_bytes)
            .finish()
    }
}

/// MCP `ResourceProducer` for `events://session/{instance_id}`.
pub struct SessionEventResourceProducer {
    buffer: SessionEventBuffer,
}

impl SessionEventResourceProducer {
    /// Build a resource producer backed by `buffer`.
    #[must_use]
    pub fn new(buffer: SessionEventBuffer) -> Self {
        Self { buffer }
    }
}

impl ResourceProducer for SessionEventResourceProducer {
    fn scheme(&self) -> &str {
        "events"
    }

    fn list(&self) -> Vec<McpResource> {
        vec![McpResource {
            uri: self.buffer.resource_uri(),
            name: format!("Session events for {}", self.buffer.instance_id()),
            description: Some(
                "Bounded DCC adapter session/job observability events. Supports ?cursor=N&limit=N&drain=true."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
        }]
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        let (base, query) = split_query(uri);
        let expected = self.buffer.resource_uri();
        if base != expected {
            return Err(ResourceError::NotFound(uri.to_string()));
        }
        let options = parse_read_options(query);
        let page = self.buffer.read(options);
        let text = serde_json::to_string(&page).map_err(|e| ResourceError::Read(e.to_string()))?;
        Ok(ProducerContent::Text {
            uri: uri.to_string(),
            mime_type: "application/json".to_string(),
            text,
        })
    }
}

fn normalized_limit(limit: usize) -> usize {
    let requested = if limit == 0 {
        DEFAULT_SESSION_EVENT_READ_LIMIT
    } else {
        limit
    };
    requested.min(MAX_SESSION_EVENT_READ_LIMIT)
}

fn truncate_message(message: String, max_bytes: usize) -> (String, Option<SessionEventTruncation>) {
    let original_size = message.len();
    if max_bytes == 0 || original_size <= max_bytes {
        return (message, None);
    }
    let mut end = max_bytes;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = message[..end].to_owned();
    let truncated_size = truncated.len();
    (
        truncated,
        Some(SessionEventTruncation {
            original_size,
            truncated_size,
            max_size: max_bytes,
        }),
    )
}

fn split_query(uri: &str) -> (&str, Option<&str>) {
    match uri.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (uri, None),
    }
}

fn parse_read_options(query: Option<&str>) -> SessionEventReadOptions {
    let Some(query) = query else {
        return SessionEventReadOptions::default();
    };
    let mut options = SessionEventReadOptions::default();
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "cursor" => {
                if let Ok(cursor) = value.parse() {
                    options.cursor = cursor;
                }
            }
            "limit" => {
                if let Ok(limit) = value.parse() {
                    options.limit = limit;
                }
            }
            "drain" => {
                options.drain = matches!(value, "1" | "true" | "yes");
            }
            _ => {}
        }
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_buffer_reads_by_cursor_and_tracks_retention() {
        let buffer = SessionEventBuffer::with_limits("maya-1", 3, 1024);
        for i in 0..5 {
            buffer.append_text(
                "python",
                "stdout",
                SessionEventLevel::Info,
                format!("line {i}"),
            );
        }

        let page = buffer.read(SessionEventReadOptions {
            cursor: 0,
            limit: 10,
            drain: false,
        });
        assert_eq!(page.events.len(), 3);
        assert_eq!(page.events[0].message, "line 2");
        assert_eq!(page.next_cursor, 5);
        assert_eq!(page.dropped_count, 2);

        let next = buffer.read(SessionEventReadOptions {
            cursor: page.next_cursor,
            limit: 10,
            drain: false,
        });
        assert!(next.events.is_empty());
    }

    #[test]
    fn event_buffer_preserves_tool_and_job_correlation() {
        let buffer = SessionEventBuffer::new("maya-1");
        buffer.append(
            SessionEvent::new("python", "stdout", SessionEventLevel::Info, "hello")
                .with_tool_call_id("req-1")
                .with_job_id("job-1"),
        );

        let page = buffer.read(SessionEventReadOptions {
            cursor: 0,
            limit: 10,
            drain: false,
        });
        assert_eq!(page.events[0].tool_call_id.as_deref(), Some("req-1"));
        assert_eq!(page.events[0].job_id.as_deref(), Some("job-1"));
        assert_eq!(page.events[0].instance_id.as_deref(), Some("maya-1"));
    }

    #[test]
    fn event_buffer_truncates_at_utf8_boundary() {
        let buffer = SessionEventBuffer::with_limits("houdini-1", 10, 4);
        let event = buffer.append_text("host", "log", SessionEventLevel::Warning, "日日日");
        assert_eq!(event.message, "日");
        assert_eq!(event.truncation.unwrap().original_size, 9);
    }

    #[test]
    fn event_resource_reads_json_page_with_query() {
        let buffer = SessionEventBuffer::new("photoshop-1");
        buffer.append_text("uxp", "stderr", SessionEventLevel::Error, "failed");
        let producer = SessionEventResourceProducer::new(buffer.clone());

        let content = producer
            .read("events://session/photoshop-1?cursor=0&limit=1")
            .unwrap();
        let ProducerContent::Text { text, .. } = content else {
            panic!("expected JSON text");
        };
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["events"][0]["message"], "failed");
        assert_eq!(value["next_cursor"], json!(1));
    }

    #[test]
    fn drain_removes_returned_events() {
        let buffer = SessionEventBuffer::new("custom-1");
        buffer.append_text("host", "progress", SessionEventLevel::Info, "a");
        buffer.append_text("host", "progress", SessionEventLevel::Info, "b");
        let page = buffer.read(SessionEventReadOptions {
            cursor: 0,
            limit: 1,
            drain: true,
        });
        assert_eq!(page.events.len(), 1);
        assert_eq!(buffer.len(), 1);
    }
}
