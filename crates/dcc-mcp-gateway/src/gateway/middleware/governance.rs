//! Read-only governance descriptors for gateway middleware.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bounded, serialisable view of one middleware control.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MiddlewareGovernanceControl {
    /// Stable middleware kind, for example `audit`, `quota`, or `redaction`.
    pub kind: String,
    /// Whether the middleware observes, mutates, or rejects requests.
    pub mode: String,
    /// Human-readable summary safe for operator UIs.
    pub summary: String,
    /// Small structured details. Must not include raw request bodies or secrets.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub config: Value,
}

impl MiddlewareGovernanceControl {
    pub fn new(
        kind: impl Into<String>,
        mode: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            mode: mode.into(),
            summary: summary.into(),
            config: Value::Null,
        }
    }

    #[must_use]
    pub fn with_config(mut self, config: Value) -> Self {
        self.config = config;
        self
    }
}

/// Snapshot of the ordered middleware chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MiddlewareGovernanceSnapshot {
    pub before_count: usize,
    pub after_count: usize,
    pub controls: Vec<MiddlewareGovernanceControl>,
}
