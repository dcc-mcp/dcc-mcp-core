//! Shared state for the admin UI handlers.

use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::Mutex;
use serde_json::Value;

use crate::gateway::middleware::{AuditEntry, AuditSink};
use crate::gateway::state::GatewayState;

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
/// (issue #864).
///
/// `start_gateway_tasks` constructs this sink, wraps it in an
/// [`AuditMiddleware`], prepends the middleware to the chain, and hands the
/// same `Arc<AuditLog>` to [`AdminState::with_audit_log`] — closing the link
/// between the middleware and the `/admin/api/calls` endpoint.
pub struct AdminAuditSink {
    log: Arc<AuditLog>,
    capacity: usize,
}

impl AdminAuditSink {
    pub fn new(log: Arc<AuditLog>, capacity: usize) -> Self {
        Self { log, capacity }
    }
}

impl AuditSink for AdminAuditSink {
    fn record(&self, entry: AuditEntry) {
        let record = AdminAuditRecord {
            timestamp: entry.timestamp,
            action: entry.tool_slug.unwrap_or_else(|| entry.method.clone()),
            dcc_type: entry.dcc_type,
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
    }
}

/// State injected into every admin handler via axum's `State` extractor.
#[derive(Clone)]
pub struct AdminState {
    pub gateway: GatewayState,
    pub audit_log: Option<Arc<AuditLog>>,
    pub event_log: Arc<EventLog>,
    pub started_at: SystemTime,
}

impl AdminState {
    pub fn new(gateway: GatewayState) -> Self {
        Self {
            gateway,
            audit_log: None,
            event_log: Arc::new(Mutex::new(Vec::new())),
            started_at: SystemTime::now(),
        }
    }

    pub fn with_audit_log(mut self, log: Arc<AuditLog>) -> Self {
        self.audit_log = Some(log);
        self
    }

    pub fn with_event_log(mut self, log: Arc<EventLog>) -> Self {
        self.event_log = log;
        self
    }
}
