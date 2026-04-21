//! Audit logging for sandbox action execution.
//!
//! Every action dispatched through the sandbox produces an [`AuditEntry`]
//! regardless of success or failure.  The [`AuditLog`] stores entries
//! in-memory and allows filtering / export.  For production use the caller
//! can flush entries to a persistent store at any time.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

// ── Outcome ───────────────────────────────────────────────────────────────────

/// Whether an audited action succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    /// Action completed without error.
    Success,
    /// Action was blocked by policy (denied action, path, read-only, etc.).
    Denied {
        /// Human-readable reason from the policy engine.
        reason: String,
    },
    /// Action was allowed but returned an execution error.
    Error {
        /// Error description.
        message: String,
    },
    /// Action was terminated due to timeout.
    Timeout,
}

// ── AuditEntry ────────────────────────────────────────────────────────────────

/// A single audit record produced by the sandbox for one action invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Monotonic timestamp: milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// Optional caller identity (e.g., agent ID, username).
    pub actor: Option<String>,
    /// Name of the action that was invoked.
    pub action: String,
    /// Snapshot of the input parameters (serialised JSON).
    pub params_json: String,
    /// How long the action ran before finishing or being cancelled.
    pub duration_ms: u64,
    /// Final outcome.
    pub outcome: AuditOutcome,
}

impl AuditEntry {
    /// Create a new entry stamped at the current wall-clock time.
    pub fn new(
        actor: Option<String>,
        action: impl Into<String>,
        params_json: impl Into<String>,
        duration: Duration,
        outcome: AuditOutcome,
    ) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            timestamp_ms,
            actor,
            action: action.into(),
            params_json: params_json.into(),
            duration_ms: duration.as_millis() as u64,
            outcome,
        }
    }

    /// Return `true` if this entry records a successful execution.
    pub fn is_success(&self) -> bool {
        self.outcome == AuditOutcome::Success
    }

    /// Return `true` if this entry records a policy denial.
    pub fn is_denied(&self) -> bool {
        matches!(self.outcome, AuditOutcome::Denied { .. })
    }
}

// ── AuditLog ──────────────────────────────────────────────────────────────────

/// Thread-safe in-memory audit log.
///
/// Wrap with `Arc<AuditLog>` to share across threads / tasks.
///
/// An optional Tokio `broadcast` channel is exposed via [`AuditLog::watch`]
/// so observers (e.g. the MCP `audit://` resource producer introduced in
/// issue #350) can receive a notification each time [`AuditLog::record`]
/// appends an entry.
#[derive(Debug, Clone)]
pub struct AuditLog {
    entries: Arc<Mutex<Vec<AuditEntry>>>,
    append_tx: broadcast::Sender<()>,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLog {
    /// Create an empty audit log.
    pub fn new() -> Self {
        let (append_tx, _) = broadcast::channel(16);
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
            append_tx,
        }
    }

    /// Append a new audit entry.
    ///
    /// Fires a notification on the broadcast channel returned by
    /// [`AuditLog::watch`] (best-effort — dropped when there are no
    /// subscribers).
    pub fn record(&self, entry: AuditEntry) {
        let mut guard = self.entries.lock();
        guard.push(entry);
        drop(guard);
        // Best-effort: ignore send error when no subscribers are attached.
        let _ = self.append_tx.send(());
    }

    /// Subscribe to append events.
    ///
    /// Each successful [`AuditLog::record`] call fires a `()` on the
    /// returned receiver. Useful for the MCP `audit://recent` resource
    /// producer which emits `notifications/resources/updated` when a new
    /// entry lands. The channel has a bounded capacity — slow consumers
    /// see lagged recv errors which they should treat as a hint to poll.
    pub fn watch(&self) -> broadcast::Receiver<()> {
        self.append_tx.subscribe()
    }

    /// Return all recorded entries (cloned).
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.lock().clone()
    }

    /// Return the total number of recorded entries.
    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    /// Return `true` when no entries have been recorded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drain and return all entries, leaving the log empty.
    pub fn drain(&self) -> Vec<AuditEntry> {
        let mut guard = self.entries.lock();
        std::mem::take(&mut *guard)
    }

    /// Return only the entries whose outcome is [`AuditOutcome::Success`].
    pub fn successes(&self) -> Vec<AuditEntry> {
        self.entries()
            .into_iter()
            .filter(|e| e.is_success())
            .collect()
    }

    /// Return only the entries whose outcome is [`AuditOutcome::Denied`].
    pub fn denials(&self) -> Vec<AuditEntry> {
        self.entries()
            .into_iter()
            .filter(|e| e.is_denied())
            .collect()
    }

    /// Return entries for a specific action name.
    pub fn entries_for_action(&self, action: &str) -> Vec<AuditEntry> {
        self.entries()
            .into_iter()
            .filter(|e| e.action == action)
            .collect()
    }

    /// Serialise all entries to a JSON array string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.entries())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(action: &str, outcome: AuditOutcome) -> AuditEntry {
        AuditEntry::new(
            Some("test-actor".to_string()),
            action,
            "{}",
            Duration::from_millis(10),
            outcome,
        )
    }

    mod test_audit_entry {
        use super::*;

        #[test]
        fn is_success_returns_true_for_success_outcome() {
            let e = make_entry("op", AuditOutcome::Success);
            assert!(e.is_success());
            assert!(!e.is_denied());
        }

        #[test]
        fn is_denied_returns_true_for_denied_outcome() {
            let e = make_entry(
                "op",
                AuditOutcome::Denied {
                    reason: "policy".to_string(),
                },
            );
            assert!(e.is_denied());
            assert!(!e.is_success());
        }

        #[test]
        fn timestamp_is_nonzero() {
            let e = make_entry("op", AuditOutcome::Success);
            assert!(e.timestamp_ms > 0);
        }

        #[test]
        fn duration_is_recorded() {
            let e = AuditEntry::new(
                None,
                "op",
                "{}",
                Duration::from_millis(42),
                AuditOutcome::Success,
            );
            assert_eq!(e.duration_ms, 42);
        }
    }

    mod test_audit_log {
        use super::*;

        #[test]
        fn new_log_is_empty() {
            let log = AuditLog::new();
            assert!(log.is_empty());
            assert_eq!(log.len(), 0);
        }

        #[test]
        fn record_increases_len() {
            let log = AuditLog::new();
            log.record(make_entry("op_a", AuditOutcome::Success));
            log.record(make_entry("op_b", AuditOutcome::Timeout));
            assert_eq!(log.len(), 2);
        }

        #[test]
        fn successes_filters_correctly() {
            let log = AuditLog::new();
            log.record(make_entry("op_a", AuditOutcome::Success));
            log.record(make_entry(
                "op_b",
                AuditOutcome::Denied {
                    reason: "x".to_string(),
                },
            ));
            assert_eq!(log.successes().len(), 1);
            assert_eq!(log.denials().len(), 1);
        }

        #[test]
        fn entries_for_action_filters_by_name() {
            let log = AuditLog::new();
            log.record(make_entry("op_a", AuditOutcome::Success));
            log.record(make_entry("op_b", AuditOutcome::Success));
            log.record(make_entry("op_a", AuditOutcome::Timeout));
            let a_entries = log.entries_for_action("op_a");
            assert_eq!(a_entries.len(), 2);
        }

        #[test]
        fn drain_empties_log() {
            let log = AuditLog::new();
            log.record(make_entry("op", AuditOutcome::Success));
            let drained = log.drain();
            assert_eq!(drained.len(), 1);
            assert!(log.is_empty());
        }

        #[test]
        fn to_json_produces_valid_json() {
            let log = AuditLog::new();
            log.record(make_entry("op", AuditOutcome::Success));
            let json = log.to_json().expect("serialization failed");
            assert!(json.starts_with('['));
            assert!(json.contains("op"));
        }

        #[test]
        fn clone_shares_underlying_data() {
            let log = AuditLog::new();
            let cloned = log.clone();
            log.record(make_entry("op", AuditOutcome::Success));
            // Both the original and the clone reference the same Arc
            assert_eq!(cloned.len(), 1);
        }
    }
}
