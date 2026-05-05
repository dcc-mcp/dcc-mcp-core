//! Gateway contention event log (issue #766).
//!
//! Two complementary observability surfaces:
//!
//! 1. **MCP resource `resources://gateway/events`** — an append-only JSONL ring
//!    buffer (bounded to [`EventLog::CAPACITY`] entries) that clients can read
//!    via a `resources/read` call.  Each line is a JSON object with:
//!    `timestamp`, `event`, `dcc_type`, `instance_id`, `reason`.
//!
//! 2. **Prometheus counters** (compiled only with the `prometheus` feature) —
//!    three monotonic counters exposed on the `/metrics` endpoint:
//!    - `dcc_mcp_gateway_elections_total{outcome="won|yielded|lost"}`
//!    - `dcc_mcp_gateway_evictions_total{reason="stale|ghost|probe_fail"}`
//!    - `dcc_mcp_gateway_probes_total{outcome="ready|booting|unreachable"}`
//!
//!    Label cardinality is bounded: no free-form `instance_id` labels are used.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

// ── Public event types ──────────────────────────────────────────────────────

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
}

impl EventKind {
    /// Return the string label used in the Prometheus `outcome`/`reason` label.
    pub fn as_label(&self) -> &'static str {
        match self {
            EventKind::ElectionWon => "won",
            EventKind::VoluntaryYield => "yielded",
            EventKind::GhostReaped => "ghost",
            EventKind::ProbeBooting => "booting",
            EventKind::ProbeUnreachable => "unreachable",
            EventKind::AutoDeregister => "probe_fail",
        }
    }
}

/// A single contention event stored in the ring buffer.
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
    pub fn new(
        event: EventKind,
        dcc_type: impl Into<String>,
        instance_id: impl Into<String>,
        reason: Option<String>,
    ) -> Self {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        ContendEvent {
            timestamp: ts,
            event,
            dcc_type: dcc_type.into(),
            instance_id: instance_id.into(),
            reason,
        }
    }
}

// ── Ring buffer ─────────────────────────────────────────────────────────────

/// Bounded, append-only JSONL event log stored in memory.
///
/// Older events are silently dropped when the buffer is full.
pub struct EventLog {
    inner: Mutex<VecDeque<ContendEvent>>,
}

impl EventLog {
    /// Maximum number of events kept in the ring buffer.
    pub const CAPACITY: usize = 1_000;

    pub fn new() -> Self {
        EventLog {
            inner: Mutex::new(VecDeque::with_capacity(Self::CAPACITY)),
        }
    }

    /// Append `event` to the ring buffer, evicting the oldest entry when full.
    pub fn push(&self, event: ContendEvent) {
        let mut buf = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if buf.len() >= Self::CAPACITY {
            buf.pop_front();
        }
        buf.push_back(event);
    }

    /// Render all events as a JSONL string (one JSON object per line).
    pub fn as_jsonl(&self) -> String {
        let buf = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        buf.iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Number of events currently stored.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// `true` when the buffer holds no events.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new()
    }
}

// ── Prometheus counters ─────────────────────────────────────────────────────

// Process-wide singletons via std::sync::OnceLock: registered exactly once per
// process lifetime. This prevents `AlreadyReg` panics when multiple Gateway
// instances are created in the same process (e.g., Python integration tests).
#[cfg(feature = "prometheus")]
fn elections_counter() -> &'static prometheus::CounterVec {
    static CELL: std::sync::OnceLock<prometheus::CounterVec> = std::sync::OnceLock::new();
    CELL.get_or_init(|| {
        prometheus::register_counter_vec!(
            prometheus::opts!(
                "dcc_mcp_gateway_elections_total",
                "Gateway port-election outcomes"
            ),
            &["outcome"]
        )
        .expect("dcc_mcp_gateway_elections_total registration must succeed")
    })
}

#[cfg(feature = "prometheus")]
fn evictions_counter() -> &'static prometheus::CounterVec {
    static CELL: std::sync::OnceLock<prometheus::CounterVec> = std::sync::OnceLock::new();
    CELL.get_or_init(|| {
        prometheus::register_counter_vec!(
            prometheus::opts!(
                "dcc_mcp_gateway_evictions_total",
                "Gateway registry-eviction events"
            ),
            &["reason"]
        )
        .expect("dcc_mcp_gateway_evictions_total registration must succeed")
    })
}

#[cfg(feature = "prometheus")]
fn probes_counter() -> &'static prometheus::CounterVec {
    static CELL: std::sync::OnceLock<prometheus::CounterVec> = std::sync::OnceLock::new();
    CELL.get_or_init(|| {
        prometheus::register_counter_vec!(
            prometheus::opts!(
                "dcc_mcp_gateway_probes_total",
                "Gateway backend readiness-probe outcomes"
            ),
            &["outcome"]
        )
        .expect("dcc_mcp_gateway_probes_total registration must succeed")
    })
}

/// Thin wrapper around the three gateway contention counters.
///
/// The underlying `CounterVec`s are process-wide singletons (via
/// `std::sync::OnceLock`). Multiple `GatewayMetrics` instances in the same
/// process safely share the same counters without re-registration.
#[cfg(feature = "prometheus")]
pub struct GatewayMetrics;

#[cfg(feature = "prometheus")]
impl GatewayMetrics {
    /// Ensure counters are registered (initialization happens at most once
    /// per process via `OnceLock::get_or_init`).
    pub fn new() -> Self {
        let _ = elections_counter();
        let _ = evictions_counter();
        let _ = probes_counter();
        GatewayMetrics
    }

    pub fn inc_election(&self, outcome: &str) {
        elections_counter().with_label_values(&[outcome]).inc();
    }

    pub fn inc_eviction(&self, reason: &str) {
        evictions_counter().with_label_values(&[reason]).inc();
    }

    pub fn inc_probe(&self, outcome: &str) {
        probes_counter().with_label_values(&[outcome]).inc();
    }
}

#[cfg(feature = "prometheus")]
impl Default for GatewayMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// ── Convenience: record an event into both surfaces ─────────────────────────

/// Record a contention event into the `EventLog` and (if compiled with the
/// `prometheus` feature) increment the corresponding counter.
///
/// # Arguments
/// * `log`     — shared event-log ring buffer from `GatewayState`
/// * `metrics` — optional Prometheus counter facade (None when feature is off)
/// * `event`   — what happened
/// * `dcc_type`  — DCC type string for the affected instance
/// * `instance_id` — short (8-char) id of the affected instance
/// * `reason`  — optional free-form context string (not used as a label)
pub fn record_event(
    log: &EventLog,
    #[cfg(feature = "prometheus")] metrics: &GatewayMetrics,
    event: EventKind,
    dcc_type: impl Into<String>,
    instance_id: impl Into<String>,
    reason: Option<String>,
) {
    #[cfg(feature = "prometheus")]
    {
        match event {
            EventKind::ElectionWon | EventKind::VoluntaryYield => {
                metrics.inc_election(event.as_label());
            }
            EventKind::GhostReaped => {
                metrics.inc_eviction(event.as_label());
            }
            EventKind::ProbeBooting => {
                metrics.inc_probe(event.as_label());
            }
            EventKind::ProbeUnreachable => {
                metrics.inc_probe(event.as_label());
                metrics.inc_eviction("probe_fail");
            }
            EventKind::AutoDeregister => {
                metrics.inc_eviction(event.as_label());
            }
        }
    }

    log.push(ContendEvent::new(event, dcc_type, instance_id, reason));
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_bounded_to_capacity() {
        let log = EventLog::new();
        for i in 0..EventLog::CAPACITY + 10 {
            log.push(ContendEvent::new(
                EventKind::GhostReaped,
                "maya",
                format!("dead{i:04}"),
                None,
            ));
        }
        assert_eq!(
            log.len(),
            EventLog::CAPACITY,
            "ring buffer must not grow beyond CAPACITY"
        );
    }

    #[test]
    fn as_jsonl_produces_one_line_per_event() {
        let log = EventLog::new();
        log.push(ContendEvent::new(
            EventKind::ElectionWon,
            "blender",
            "abcd1234",
            None,
        ));
        log.push(ContendEvent::new(
            EventKind::ProbeBooting,
            "maya",
            "ef012345",
            Some("backend still initialising".into()),
        ));

        let jsonl = log.as_jsonl();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["event"], "election_won");
        assert_eq!(first["dcc_type"], "blender");
        assert_eq!(first["instance_id"], "abcd1234");
        assert!(
            first.get("reason").is_none(),
            "reason must be absent when None"
        );

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["event"], "probe_booting");
        assert_eq!(second["reason"], "backend still initialising");
    }

    #[test]
    fn empty_log_returns_empty_string() {
        let log = EventLog::new();
        assert!(log.as_jsonl().is_empty());
    }

    #[test]
    fn oldest_entry_evicted_when_full() {
        let log = EventLog::new();
        // Fill the buffer.
        for i in 0..EventLog::CAPACITY {
            log.push(ContendEvent::new(
                EventKind::GhostReaped,
                "maya",
                format!("id{i:04}"),
                None,
            ));
        }
        // Push one more — id0000 must be evicted.
        log.push(ContendEvent::new(
            EventKind::ElectionWon,
            "gateway",
            "newentry",
            None,
        ));

        let jsonl = log.as_jsonl();
        assert!(
            !jsonl.contains("\"id0000\""),
            "oldest entry must be evicted from the ring buffer"
        );
        assert!(
            jsonl.contains("\"newentry\""),
            "newest entry must be present after eviction"
        );
    }
}
