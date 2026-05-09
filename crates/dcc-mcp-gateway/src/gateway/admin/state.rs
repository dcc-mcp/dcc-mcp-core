//! Shared state for the admin UI handlers.

use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::Mutex;
use serde_json::Value;

use crate::gateway::middleware::{AuditEntry, AuditSink};
use crate::gateway::state::GatewayState;

use super::stats::StatsAggregator;
use super::trace::{DispatchTrace, TraceLog};

/// Minimal audit record that the admin UI consumes.
#[derive(Debug, Clone)]
pub struct AdminAuditRecord {
    pub timestamp: SystemTime,
    /// Tool slug or MCP method name.
    pub action: String,
    /// DCC type of the target backend (e.g. `"maya"`).
    pub dcc_type: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    /// Wall-clock call duration in milliseconds.
    pub duration_ms: Option<u64>,
}

/// Append-only ring buffer for gateway event log entries.
pub type EventLog = Mutex<Vec<Value>>;

/// Append-only audit log shared with the admin UI.
pub type AuditLog = Mutex<Vec<AdminAuditRecord>>;

/// [`AuditSink`] that pushes completed entries into the admin UI ring buffer
/// and optionally a [`TraceLog`] for Phase 2 dispatch traces.
pub struct AdminAuditSink {
    log: Arc<AuditLog>,
    capacity: usize,
    trace_log: Option<Arc<TraceLog>>,
}

impl AdminAuditSink {
    pub fn new(log: Arc<AuditLog>, capacity: usize) -> Self {
        Self {
            log,
            capacity,
            trace_log: None,
        }
    }

    /// Attach a trace log so `record()` also appends a [`DispatchTrace`].
    pub fn with_trace_log(mut self, trace_log: Arc<TraceLog>) -> Self {
        self.trace_log = Some(trace_log);
        self
    }
}

impl AuditSink for AdminAuditSink {
    fn record(&self, entry: AuditEntry) {
        let record = AdminAuditRecord {
            timestamp: entry.timestamp,
            action: entry
                .tool_slug
                .clone()
                .unwrap_or_else(|| entry.method.clone()),
            dcc_type: entry.dcc_type.clone(),
            success: !entry.is_error,
            error: if entry.is_error {
                Some(entry.result_preview.clone())
            } else {
                None
            },
            duration_ms: entry.duration_ms,
        };
        let mut buf = self.log.lock();
        buf.push(record);
        if self.capacity > 0 {
            while buf.len() > self.capacity {
                buf.remove(0);
            }
        }

        // Phase 2: promote AuditEntry into a DispatchTrace when a trace log is attached.
        if let Some(tl) = &self.trace_log {
            let trace = DispatchTrace {
                request_id: entry.request_id.clone(),
                method: entry.method.clone(),
                tool_slug: entry.tool_slug.clone(),
                instance_id: entry.instance_id.clone(),
                session_id: entry.session_id.clone(),
                dcc_type: entry.dcc_type.clone(),
                started_at: entry.timestamp,
                total_ms: entry.duration_ms.unwrap_or(0),
                ok: !entry.is_error,
                spans: entry.trace_spans,
                input: entry.input_payload,
                output: entry.output_payload,
            };
            tl.push(trace);
        }
    }
}

/// State injected into every admin handler via axum's `State` extractor.
#[derive(Clone)]
pub struct AdminState {
    pub gateway: GatewayState,
    pub audit_log: Option<Arc<AuditLog>>,
    /// Phase 2 trace log — `None` until `with_trace_log` is called.
    pub trace_log: Option<Arc<TraceLog>>,
    /// Phase 3 stats aggregator — `None` until `with_trace_log` is called.
    pub stats: Option<Arc<StatsAggregator>>,
    pub event_log: Arc<EventLog>,
    pub started_at: SystemTime,
}

impl AdminState {
    pub fn new(gateway: GatewayState) -> Self {
        Self {
            gateway,
            audit_log: None,
            trace_log: None,
            stats: None,
            event_log: Arc::new(Mutex::new(Vec::new())),
            started_at: SystemTime::now(),
        }
    }

    pub fn with_audit_log(mut self, log: Arc<AuditLog>) -> Self {
        self.audit_log = Some(log);
        self
    }

    pub fn with_trace_log(mut self, log: Arc<TraceLog>) -> Self {
        // Phase 3: auto-create StatsAggregator when a TraceLog is attached.
        self.stats = Some(Arc::new(StatsAggregator::new(log.clone())));
        self.trace_log = Some(log);
        self
    }

    pub fn with_event_log(mut self, log: Arc<EventLog>) -> Self {
        self.event_log = log;
        self
    }
}
