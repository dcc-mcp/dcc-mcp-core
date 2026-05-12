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
    /// Wall-clock time when the call completed.
    pub timestamp: SystemTime,
    /// Stable request id used to correlate with traces.
    pub request_id: String,
    /// JSON-RPC / MCP method name.
    pub method: Option<String>,
    /// Target backend instance id, if resolved.
    pub instance_id: Option<String>,
    /// Originating MCP session id, if any.
    pub session_id: Option<String>,
    /// Tool slug or MCP method name.
    pub action: String,
    /// DCC type of the target backend (e.g. `"maya"`).
    pub dcc_type: Option<String>,
    /// Whether the call succeeded (`true`) or returned an error (`false`).
    pub success: bool,
    /// Error preview when `success == false`; otherwise `None`.
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
    /// Build a sink that pushes audit records into `log`, capped at `capacity`
    /// entries (oldest evicted first).
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
            request_id: entry.request_id.clone(),
            method: Some(entry.method.clone()),
            instance_id: entry.instance_id.clone(),
            session_id: entry.session_id.clone(),
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
    /// Live gateway state — registry, capability index, server metadata.
    pub gateway: GatewayState,
    /// Audit log ring buffer — `None` until `with_audit_log` is called.
    pub audit_log: Option<Arc<AuditLog>>,
    /// Phase 2 trace log — `None` until `with_trace_log` is called.
    pub trace_log: Option<Arc<TraceLog>>,
    /// Phase 3 stats aggregator — `None` until `with_trace_log` is called.
    pub stats: Option<Arc<StatsAggregator>>,
    /// Append-only event log shared with the gateway core.
    pub event_log: Arc<EventLog>,
    /// Wall-clock time the gateway started, used for the Health card uptime.
    pub started_at: SystemTime,
}

impl AdminState {
    /// Build an [`AdminState`] backed by the live `GatewayState`. Audit /
    /// trace / stats logs default to `None`; attach them via the
    /// `with_*` builders before mounting the admin router.
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

    /// Attach the [`AuditLog`] that `GET /admin/api/calls` reads from.
    pub fn with_audit_log(mut self, log: Arc<AuditLog>) -> Self {
        self.audit_log = Some(log);
        self
    }

    /// Attach the Phase 2 [`TraceLog`]. Implicitly bootstraps a
    /// [`StatsAggregator`] (Phase 3) over the same log so the admin
    /// router can serve `GET /admin/api/stats` without extra wiring.
    pub fn with_trace_log(mut self, log: Arc<TraceLog>) -> Self {
        // Phase 3: auto-create StatsAggregator when a TraceLog is attached.
        self.stats = Some(Arc::new(StatsAggregator::new(log.clone())));
        self.trace_log = Some(log);
        self
    }

    /// Replace the default empty [`EventLog`] with one shared with the
    /// gateway core (so `GET /admin/api/logs` surfaces gateway events).
    pub fn with_event_log(mut self, log: Arc<EventLog>) -> Self {
        self.event_log = log;
        self
    }
}
