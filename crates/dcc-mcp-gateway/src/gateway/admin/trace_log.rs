//! Bounded in-memory dispatch trace storage for admin diagnostics.

use parking_lot::Mutex;

use super::trace::DispatchTrace;

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
