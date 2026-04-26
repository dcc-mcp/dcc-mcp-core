//! DCC output capture as an MCP `output://` resource (issue #461).
//!
//! Exposes DCC application output (stdout, stderr, script editor) as a
//! live MCP resource. AI agents can subscribe to the resource and receive
//! new output asynchronously over the SSE channel.
//!
//! # URI scheme
//!
//! | URI | Description |
//! |-----|-------------|
//! | `output://instance/{instance_id}` | Per-instance ring-buffered output |
//!
//! # Integration (DCC adapter side)
//!
//! ```python
//! from dcc_mcp_core import OutputCapture
//!
//! # instance_id is typically the DCC process UUID from ServiceEntry
//! capture = OutputCapture(instance_id="my-maya-0001")
//!
//! import sys
//! sys.stdout = capture.wrap_stdout()
//! sys.stderr = capture.wrap_stderr()
//!
//! # Or push manually:
//! capture.push("stdout", "INFO: sphere created\n")
//! ```
//!
//! On the Rust / HTTP server side, wire `OutputBuffer` into the
//! [`crate::resources::ResourceRegistry`] so `resources/read` and
//! `resources/subscribe` serve the buffered lines.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::protocol::McpResource;
use crate::resources::{ProducerContent, ResourceError, ResourceProducer, ResourceResult};

// ── OutputEntry ────────────────────────────────────────────────────────────────

/// Which output stream an [`OutputEntry`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
    ScriptEditor,
}

impl OutputStream {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::ScriptEditor => "script_editor",
        }
    }
}

/// A single captured output line with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputEntry {
    /// Unix epoch nanoseconds when the line was captured.
    pub timestamp_ns: u128,
    /// DCC instance identifier (matches the resource URI segment).
    pub instance_id: String,
    /// Which output channel this came from.
    pub stream: OutputStream,
    /// The captured text (may include newlines).
    pub text: String,
}

impl OutputEntry {
    pub fn new(
        instance_id: impl Into<String>,
        stream: OutputStream,
        text: impl Into<String>,
    ) -> Self {
        let timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            timestamp_ns,
            instance_id: instance_id.into(),
            stream,
            text: text.into(),
        }
    }
}

// ── OutputBuffer ──────────────────────────────────────────────────────────────

/// Default ring buffer capacity.
const DEFAULT_CAPACITY: usize = 1000;

/// Thread-safe ring buffer for DCC output.
///
/// `OutputBuffer` is cheap to clone (backed by `Arc`). DCC adapters call
/// [`OutputBuffer::push`] from any thread; the server reads via
/// [`OutputBuffer::drain_since`] or subscribes via `subscribe()`.
#[derive(Clone)]
pub struct OutputBuffer {
    inner: Arc<OutputBufferInner>,
}

struct OutputBufferInner {
    ring: RwLock<VecDeque<OutputEntry>>,
    capacity: usize,
    /// Fan-out broadcast for SSE subscribers.
    tx: broadcast::Sender<OutputEntry>,
    instance_id: String,
}

impl OutputBuffer {
    /// Create a new buffer for `instance_id` with the default ring capacity.
    pub fn new(instance_id: impl Into<String>) -> Self {
        Self::with_capacity(instance_id, DEFAULT_CAPACITY)
    }

    /// Create a new buffer with a custom ring capacity.
    pub fn with_capacity(instance_id: impl Into<String>, capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(OutputBufferInner {
                ring: RwLock::new(VecDeque::with_capacity(capacity)),
                capacity,
                tx,
                instance_id: instance_id.into(),
            }),
        }
    }

    /// Push a new entry into the ring and notify all SSE subscribers.
    pub fn push(&self, entry: OutputEntry) {
        {
            let mut ring = self.inner.ring.write();
            ring.push_back(entry.clone());
            if ring.len() > self.inner.capacity {
                ring.pop_front();
            }
        }
        // Best-effort fan-out — ignore errors if no subscribers.
        let _ = self.inner.tx.send(entry);
    }

    /// Convenience: push a stdout line.
    pub fn push_stdout(&self, text: impl Into<String>) {
        self.push(OutputEntry::new(
            &self.inner.instance_id,
            OutputStream::Stdout,
            text,
        ));
    }

    /// Convenience: push a stderr line.
    pub fn push_stderr(&self, text: impl Into<String>) {
        self.push(OutputEntry::new(
            &self.inner.instance_id,
            OutputStream::Stderr,
            text,
        ));
    }

    /// Convenience: push a script-editor line.
    pub fn push_script_editor(&self, text: impl Into<String>) {
        self.push(OutputEntry::new(
            &self.inner.instance_id,
            OutputStream::ScriptEditor,
            text,
        ));
    }

    /// Return all entries with `timestamp_ns` >= `since_ns`.
    ///
    /// Pass `0` to get all buffered entries.
    pub fn drain_since(&self, since_ns: u128) -> Vec<OutputEntry> {
        let ring = self.inner.ring.read();
        ring.iter()
            .filter(|e| e.timestamp_ns >= since_ns)
            .cloned()
            .collect()
    }

    /// Subscribe to new output entries.
    pub fn subscribe(&self) -> broadcast::Receiver<OutputEntry> {
        self.inner.tx.subscribe()
    }

    /// The DCC instance ID this buffer belongs to.
    pub fn instance_id(&self) -> &str {
        &self.inner.instance_id
    }

    /// How many entries are currently in the ring.
    pub fn len(&self) -> usize {
        self.inner.ring.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Debug for OutputBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputBuffer")
            .field("instance_id", &self.inner.instance_id)
            .field("len", &self.len())
            .field("capacity", &self.inner.capacity)
            .finish()
    }
}

// ── ResourceProducer impl ─────────────────────────────────────────────────────

/// MCP `ResourceProducer` that serves buffered DCC output as `output://` URIs.
pub struct OutputResourceProducer {
    buffer: OutputBuffer,
}

impl OutputResourceProducer {
    pub fn new(buffer: OutputBuffer) -> Self {
        Self { buffer }
    }
}

impl ResourceProducer for OutputResourceProducer {
    fn scheme(&self) -> &str {
        "output"
    }

    fn list(&self) -> Vec<McpResource> {
        vec![McpResource {
            uri: format!("output://instance/{}", self.buffer.instance_id()),
            name: format!("DCC output for {}", self.buffer.instance_id()),
            description: Some(
                "Real-time DCC stdout/stderr/script editor output (ring buffer).".to_string(),
            ),
            mime_type: Some("text/plain".to_string()),
        }]
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        let expected = format!("output://instance/{}", self.buffer.instance_id());
        if uri != expected {
            return Err(ResourceError::NotFound(uri.to_string()));
        }
        // Parse optional `?since=<ns>` query parameter.
        let since_ns: u128 = uri
            .find('?')
            .and_then(|pos| {
                uri[pos + 1..]
                    .split('&')
                    .find(|p| p.starts_with("since="))
                    .and_then(|p| p["since=".len()..].parse().ok())
            })
            .unwrap_or(0);

        let entries = self.buffer.drain_since(since_ns);
        let text = entries
            .iter()
            .map(|e| {
                format!(
                    "[{}][{}] {}",
                    e.timestamp_ns,
                    e.stream.as_str(),
                    e.text.trim_end()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ProducerContent::Text {
            uri: uri.to_string(),
            mime_type: "text/plain".to_string(),
            text,
        })
    }
}

// ── OutputCapture (Python-facing high-level wrapper) ──────────────────────────

/// Python-friendly wrapper around [`OutputBuffer`].
///
/// DCC adapters call [`OutputCapture::push`] from Python to feed lines into
/// the ring buffer. A `sys.stdout`/`sys.stderr` replacement is provided via
/// [`OutputCapture::wrap_stdout`] / [`OutputCapture::wrap_stderr`] when used
/// from the Python side.
#[derive(Debug, Clone)]
pub struct OutputCapture {
    pub buffer: OutputBuffer,
}

impl OutputCapture {
    /// Create a new capture handle for `instance_id`.
    pub fn new(instance_id: impl Into<String>) -> Self {
        Self {
            buffer: OutputBuffer::new(instance_id),
        }
    }

    /// Create a capture with a custom ring buffer size.
    pub fn with_capacity(instance_id: impl Into<String>, maxlen: usize) -> Self {
        Self {
            buffer: OutputBuffer::with_capacity(instance_id, maxlen),
        }
    }

    /// Push a text line from the given stream.
    pub fn push(&self, stream: &str, text: impl Into<String>) {
        let s = match stream {
            "stderr" => OutputStream::Stderr,
            "script_editor" => OutputStream::ScriptEditor,
            _ => OutputStream::Stdout,
        };
        self.buffer
            .push(OutputEntry::new(self.buffer.instance_id(), s, text));
    }

    /// Drain all entries since `since_ns` (pass 0 for all).
    pub fn drain(&self, since_ns: u128) -> Vec<OutputEntry> {
        self.buffer.drain_since(since_ns)
    }

    /// Get the MCP resource URI for this capture instance.
    pub fn resource_uri(&self) -> String {
        format!("output://instance/{}", self.buffer.instance_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_drain() {
        let buf = OutputBuffer::new("test-instance");
        buf.push_stdout("hello stdout");
        buf.push_stderr("hello stderr");
        let all = buf.drain_since(0);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].stream, OutputStream::Stdout);
        assert_eq!(all[0].text, "hello stdout");
        assert_eq!(all[1].stream, OutputStream::Stderr);
    }

    #[test]
    fn test_drain_since_filters() {
        let buf = OutputBuffer::new("inst");
        buf.push_stdout("line1");
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::thread::sleep(std::time::Duration::from_millis(1));
        buf.push_stdout("line2");
        let after = buf.drain_since(cutoff);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].text, "line2");
    }

    #[test]
    fn test_capacity_limit() {
        let buf = OutputBuffer::with_capacity("cap-test", 3);
        for i in 0..5 {
            buf.push_stdout(format!("line {i}"));
        }
        let all = buf.drain_since(0);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].text, "line 2");
    }

    #[test]
    fn test_output_capture() {
        let cap = OutputCapture::new("maya-001");
        cap.push("stdout", "INFO: scene loaded");
        cap.push("stderr", "WARNING: deprecated node");
        let entries = cap.drain(0);
        assert_eq!(entries.len(), 2);
        assert_eq!(cap.resource_uri(), "output://instance/maya-001");
    }

    #[test]
    fn test_resource_producer() {
        let buf = OutputBuffer::new("dcc-123");
        buf.push_stdout("test line");
        let prod = OutputResourceProducer::new(buf);
        let resources = prod.list();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "output://instance/dcc-123");
        let content = prod.read("output://instance/dcc-123").unwrap();
        if let ProducerContent::Text { text, .. } = content {
            assert!(text.contains("test line"));
        } else {
            panic!("expected text content");
        }
    }
}
