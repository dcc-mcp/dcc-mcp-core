//! Structured audit sink used by every call path (#660).
//!
//! Every `/v1/call` invocation emits exactly one [`AuditEvent`] once
//! the handler resolves — success *or* failure. The trait is
//! deliberately small (one async-free method) so it composes cleanly
//! with every logging backend: tracing macros, OpenTelemetry spans,
//! write-ahead log, or an in-memory `Vec` for tests.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Outcome discriminator. `failure` carries the failing
/// [`super::errors::ServiceErrorKind`] so downstream dashboards can
/// group by failure class.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "detail")]
pub enum AuditOutcome {
    Success,
    Failure(String),
}

/// Single audit record. Fields are stable — add new optional fields,
/// never rename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Correlation id, usually propagated from the `X-Request-Id`
    /// header; falls back to a freshly-allocated UUID otherwise.
    pub request_id: String,
    /// Wall-clock timestamp the event was recorded at.
    pub at: DateTime<Utc>,
    /// Tool slug that was invoked (`""` for search/describe).
    pub slug: String,
    /// HTTP route that produced the event (e.g. `"POST /v1/call"`).
    pub route: String,
    /// Authenticated subject, as reported by the auth gate.
    pub subject: String,
    /// Outcome of the request.
    pub outcome: AuditOutcome,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

/// Pluggable sink. One call per successful *or* failed request.
///
/// Implementations **must not** block for long — the sink is called on
/// the axum worker that is about to return a response to the client.
pub trait AuditSink: Send + Sync {
    fn record(&self, event: AuditEvent);
}

/// Drop-on-the-floor sink — the default when the embedder does not
/// wire a real one in.
#[derive(Debug, Clone, Default)]
pub struct NoopAuditSink;

impl AuditSink for NoopAuditSink {
    fn record(&self, _event: AuditEvent) {}
}

/// Collects events in memory. Used by the test-suite to assert
/// audit-trail invariants without depending on a disk writer.
#[derive(Debug, Clone, Default)]
pub struct VecAuditSink {
    inner: Arc<Mutex<Vec<AuditEvent>>>,
}

impl VecAuditSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clone of all events seen so far, in insertion order.
    #[must_use]
    pub fn events(&self) -> Vec<AuditEvent> {
        self.inner.lock().clone()
    }

    /// Number of events recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

impl AuditSink for VecAuditSink {
    fn record(&self, event: AuditEvent) {
        self.inner.lock().push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> AuditEvent {
        AuditEvent {
            request_id: "r-1".into(),
            at: Utc::now(),
            slug: "maya.create_sphere".into(),
            route: "POST /v1/call".into(),
            subject: "local".into(),
            outcome: AuditOutcome::Success,
            duration_ms: 12,
        }
    }

    #[test]
    fn vec_sink_records_in_order() {
        let sink = VecAuditSink::new();
        let mut e1 = sample_event();
        e1.request_id = "r-1".into();
        let mut e2 = sample_event();
        e2.request_id = "r-2".into();
        sink.record(e1);
        sink.record(e2);
        let events = sink.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].request_id, "r-1");
        assert_eq!(events[1].request_id, "r-2");
    }

    #[test]
    fn noop_sink_never_panics() {
        let sink = NoopAuditSink;
        sink.record(sample_event());
    }

    #[test]
    fn outcome_round_trips_through_json() {
        let evt = AuditEvent {
            outcome: AuditOutcome::Failure("unknown-slug".into()),
            ..sample_event()
        };
        let json = serde_json::to_string(&evt).unwrap();
        let back: AuditEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.outcome, AuditOutcome::Failure(k) if k == "unknown-slug"));
    }
}
