//! Gateway contention event value types (issue #845).
//!
//! These are the wire-level records exposed through
//! `resources://gateway/events` and mirrored into metrics labels by the gateway
//! runtime. They live in the domain crate so admin clients and tests can parse
//! the event stream without depending on the HTTP gateway implementation.

use serde::{Deserialize, Serialize};

/// All contention-relevant event kinds the gateway can emit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// This gateway instance won the port election.
    ElectionWon,
    /// This gateway yielded the port to a higher-version challenger.
    VoluntaryYield,
    /// A ghost entry (dead-PID or stale) was reaped from the registry.
    GhostReaped,
    /// A backend instance is still booting (readiness probe → 503).
    ProbeBooting,
    /// A backend instance is unreachable (readiness probe failed).
    ProbeUnreachable,
    /// A backend instance was auto-deregistered after consecutive probe failures.
    AutoDeregister,
    /// A backend DCC host died while a gateway-routed call was in flight.
    HostDied,
    /// Operator-facing admin action (skill paths, etc.) — no Prometheus counter.
    OperatorNote,
}

impl EventKind {
    /// Return the string label used in the Prometheus `outcome`/`reason` label.
    #[must_use]
    pub fn as_label(&self) -> &'static str {
        match self {
            EventKind::ElectionWon => "won",
            EventKind::VoluntaryYield => "yielded",
            EventKind::GhostReaped => "ghost",
            EventKind::ProbeBooting => "booting",
            EventKind::ProbeUnreachable => "unreachable",
            EventKind::AutoDeregister => "probe_fail",
            EventKind::HostDied => "host_died",
            EventKind::OperatorNote => "operator",
        }
    }
}

/// A single contention event stored in the gateway ring buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContendEvent {
    /// ISO-8601 UTC timestamp (millisecond precision).
    pub timestamp: String,
    /// Event kind.
    pub event: EventKind,
    /// DCC type involved (`"maya"`, `"blender"`, `"__gateway__"`, …).
    pub dcc_type: String,
    /// Short, human-readable instance identifier (first 8 hex chars of the UUID).
    pub instance_id: String,
    /// Optional human-readable context (e.g. challenger version, failure count).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ContendEvent {
    /// Construct a new event with the current UTC timestamp (millisecond
    /// precision).
    #[must_use]
    pub fn new(
        event: EventKind,
        dcc_type: impl Into<String>,
        instance_id: impl Into<String>,
        reason: Option<String>,
    ) -> Self {
        let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        Self {
            timestamp,
            event,
            dcc_type: dcc_type.into(),
            instance_id: instance_id.into(),
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_labels_are_stable() {
        assert_eq!(EventKind::ElectionWon.as_label(), "won");
        assert_eq!(EventKind::VoluntaryYield.as_label(), "yielded");
        assert_eq!(EventKind::GhostReaped.as_label(), "ghost");
        assert_eq!(EventKind::ProbeBooting.as_label(), "booting");
        assert_eq!(EventKind::ProbeUnreachable.as_label(), "unreachable");
        assert_eq!(EventKind::AutoDeregister.as_label(), "probe_fail");
        assert_eq!(EventKind::HostDied.as_label(), "host_died");
        assert_eq!(EventKind::OperatorNote.as_label(), "operator");
    }

    #[test]
    fn contend_event_serializes_reason_only_when_present() {
        let event = ContendEvent {
            timestamp: "2026-05-11T00:00:00.000Z".to_owned(),
            event: EventKind::GhostReaped,
            dcc_type: "maya".to_owned(),
            instance_id: "abcdef01".to_owned(),
            reason: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"ghost_reaped\""));
        assert!(!json.contains("reason"));
    }

    #[test]
    fn contend_event_new_populates_fields() {
        let event = ContendEvent::new(
            EventKind::ProbeBooting,
            "photoshop",
            "abcdef01",
            Some("warming up".to_owned()),
        );
        assert_eq!(event.event, EventKind::ProbeBooting);
        assert_eq!(event.dcc_type, "photoshop");
        assert_eq!(event.instance_id, "abcdef01");
        assert_eq!(event.reason.as_deref(), Some("warming up"));
        assert!(event.timestamp.ends_with('Z'));
    }
}
