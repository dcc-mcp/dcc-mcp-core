//! Audit middleware — records all dispatched actions to an in-memory log.

use parking_lot::Mutex;
use serde_json::Value;

use crate::dispatcher::{DispatchError, DispatchResult};

use super::{ActionMiddleware, MiddlewareContext};

/// Audit log entry produced by [`AuditMiddleware`].
#[derive(Debug, Clone)]
pub struct AuditRecord {
    /// Timestamp when the action was dispatched.
    pub timestamp: std::time::SystemTime,
    /// Action name.
    pub action: String,
    /// Input parameters (cloned from context).
    pub params: Value,
    /// Whether the dispatch succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
    /// Output payload if succeeded (first 256 chars as string).
    pub output_preview: Option<String>,
}

/// Audit middleware — records all dispatched actions to an in-memory log.
///
/// In production, replace the internal Vec with a persistent store
/// (database, file, OTLP span) by wrapping `AuditMiddleware` or
/// implementing a custom `ActionMiddleware`.
pub struct AuditMiddleware {
    records: Mutex<Vec<AuditRecord>>,
    /// Whether to include input parameters in audit records (may be sensitive).
    pub record_params: bool,
}

impl AuditMiddleware {
    /// Create a new audit middleware.
    #[must_use]
    pub fn new() -> Self {
        Self {
            records: Mutex::new(Vec::new()),
            record_params: true,
        }
    }

    /// Get a snapshot of all audit records.
    #[must_use]
    pub fn records(&self) -> Vec<AuditRecord> {
        self.records.lock().clone()
    }

    /// Get the number of recorded entries.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.records.lock().len()
    }

    /// Clear all audit records.
    pub fn clear(&self) {
        self.records.lock().clear();
    }

    /// Get audit records for a specific action.
    #[must_use]
    pub fn records_for_action(&self, action: &str) -> Vec<AuditRecord> {
        self.records
            .lock()
            .iter()
            .filter(|r| r.action == action)
            .cloned()
            .collect()
    }
}

impl Default for AuditMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionMiddleware for AuditMiddleware {
    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        result: Result<&DispatchResult, &DispatchError>,
    ) {
        let record = AuditRecord {
            timestamp: std::time::SystemTime::now(),
            action: ctx.action.clone(),
            params: if self.record_params {
                ctx.params.clone()
            } else {
                Value::Null
            },
            success: result.is_ok(),
            error: result.err().map(|e| e.to_string()),
            output_preview: result.ok().map(|r| {
                let s = r.output.to_string();
                if s.len() > 256 {
                    format!("{}...", &s[..256])
                } else {
                    s
                }
            }),
        };

        self.records.lock().push(record);
    }

    fn name(&self) -> &'static str {
        "audit"
    }
}
