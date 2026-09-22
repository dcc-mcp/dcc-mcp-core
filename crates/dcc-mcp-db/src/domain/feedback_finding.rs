//! Value objects for the `feedback_findings` dedup table (no I/O).
//!
//! One row per `(repo, fingerprint)` pair: repeated reports of the same finding
//! collapse into a single row and bump [`FeedbackFindingRow::occurrence_count`]
//! instead of creating near-identical rows.
//!
//! Distinct from `feedback_reports` (#2253-E1), which is the durable
//! per-submission log keyed by the gateway-minted `feedback_id`.

use serde::{Deserialize, Serialize};

/// A feedback report that has not been persisted yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackFindingInsert {
    /// Routed owning repository (`owner/repo`), or `""` when unrouted.
    pub repo: String,
    /// Stable finding fingerprint (`sha256:<64 hex>`).
    pub fingerprint: String,
    /// Canonical public GitHub issues URL for the routed repo, when known.
    pub issues_url: Option<String>,
    /// Why this route was chosen (`adapter_phase`, `core_error_kind`, …).
    pub route_rationale: Option<String>,
    pub dcc_type: String,
    pub phase: String,
    pub severity: String,
    pub observed_at_ms: i64,
    /// Full serialized Finding v1 payload (or legacy report) for this report.
    pub report_json: String,
}

/// A persisted `feedback_reports` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackFindingRow {
    pub id: i64,
    pub repo: String,
    pub fingerprint: String,
    pub issues_url: Option<String>,
    pub route_rationale: Option<String>,
    pub dcc_type: String,
    pub phase: String,
    pub severity: String,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    /// Number of times this exact `(repo, fingerprint)` has been reported.
    pub occurrence_count: i64,
    pub report_json: String,
}
