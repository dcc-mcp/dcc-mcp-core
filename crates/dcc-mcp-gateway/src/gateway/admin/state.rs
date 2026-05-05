//! Shared state for the admin UI handlers.

use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::Mutex;
use serde_json::Value;

use crate::gateway::state::GatewayState;

/// Minimal audit record that the admin UI consumes.
///
/// This mirrors `dcc_mcp_actions::pipeline::audit::AuditRecord` but is
/// defined here so `dcc-mcp-gateway` does not need to depend on
/// `dcc-mcp-actions` (which is an optional downstream crate).
#[derive(Debug, Clone)]
pub struct AdminAuditRecord {
    pub timestamp: SystemTime,
    pub action: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Append-only ring buffer for gateway event log entries.
///
/// Each entry is a free-form JSON object emitted by gateway internals.
pub type EventLog = Mutex<Vec<Value>>;

/// Append-only audit log shared with the admin UI.
pub type AuditLog = Mutex<Vec<AdminAuditRecord>>;

/// State injected into every admin handler via axum's `State` extractor.
#[derive(Clone)]
pub struct AdminState {
    /// The underlying gateway state (registry, capability index, …).
    pub gateway: GatewayState,
    /// Optional audit log (populated when AuditMiddleware is in use).
    pub audit_log: Option<Arc<AuditLog>>,
    /// Gateway event log ring buffer.
    pub event_log: Arc<EventLog>,
    /// Gateway start time (used to compute uptime).
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
