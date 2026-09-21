//! Persisted agent-feedback envelope for the gateway admin SQLite store (#2253-E1).
//!
//! The envelope mirrors the record the per-DCC JSONL mirror writes, so reading
//! from SQLite or from the JSONL fallback yields identical admin API entries.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Submission shape accepted by `POST /v1/feedback`.
///
/// Stored alongside the report so the admin API can tell Finding v1 rows apart
/// from legacy `FeedbackReport` rows without re-parsing the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackSubmissionKind {
    /// A validated `FindingV1` payload (`schema_version` present).
    Finding,
    /// A validated legacy `FeedbackReport` payload.
    Legacy,
}

impl FeedbackSubmissionKind {
    /// Stable string stored in the `kind` column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Finding => "finding",
            Self::Legacy => "legacy",
        }
    }
}

/// Feedback report row written to the `feedback_reports` table.
///
/// `report` is the full record the admin API returns: `id`, `timestamp`
/// (seconds, float) plus the report fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackReportRow {
    /// Correlation id minted by the gateway (`feedback_id` in the receipt).
    pub id: String,
    /// Submission timestamp in milliseconds since the Unix epoch (ordering key).
    pub timestamp_ms: i64,
    /// Wall-clock time the gateway persisted the row.
    pub recorded_at_ms: i64,
    /// RFC 3339 rendering of the submission timestamp.
    pub recorded_at: String,
    /// `finding` or `legacy`.
    pub kind: FeedbackSubmissionKind,
    /// `FindingV1::schema_version`; `0` for legacy reports.
    pub schema_version: i64,
    /// `FindingV1::fingerprint`; `None` for legacy reports.
    pub fingerprint: Option<String>,
    /// Lowercase severity, e.g. `blocked` or `degraded`.
    pub severity: String,
    /// Lowercase DCC type; `gateway` when the report has no DCC scope.
    pub dcc_type: String,
    /// Instance id when the report carries one.
    pub instance_id: Option<String>,
    /// Tool slug when the report carries one.
    pub tool_slug: Option<String>,
    /// The full feedback record exactly as written to the JSONL mirror.
    pub report: Value,
}
