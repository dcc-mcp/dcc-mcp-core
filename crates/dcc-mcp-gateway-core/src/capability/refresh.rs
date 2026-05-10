//! Refresh-cycle telemetry types (issue #845).
//!
//! The mutable refresh loop — `refresh_instance`, `remove_instance`,
//! and the I/O it drives via `reqwest` — stays in `dcc-mcp-gateway`
//! because it carries runtime state. Only the *enum that classifies
//! why a refresh ran* lives here, so external Rust tooling
//! (operator dashboards, CLI inspectors) can match on the reason
//! without depending on the gateway crate's tokio / reqwest /
//! parking_lot footprint.

use serde::{Deserialize, Serialize};

/// Why a refresh cycle is running.
///
/// Surfaced through `tracing::info!` so operators can correlate an
/// index update with the event that triggered it. Also serialised
/// onto the admin UI's `/admin/api/calls` event log when the call
/// happens to be a refresh-driven dispatch (issue #772).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshReason {
    /// Instance joined the registry for the first time.
    InstanceJoined,
    /// Instance is still live but sent a `tools/list_changed`
    /// notification (typically because a skill loaded/unloaded).
    ToolsListChanged,
    /// Background periodic refresh — catches any change that did
    /// not emit a push notification.
    Periodic,
}

impl RefreshReason {
    /// String label suitable for span tags. Returns the same
    /// snake_case form that the JSON wire emits, so log lines and
    /// JSON dumps are visually consistent.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InstanceJoined => "instance_joined",
            Self::ToolsListChanged => "tools_list_changed",
            Self::Periodic => "periodic",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_reason_as_str_is_stable() {
        // The strings end up in tracing spans + the admin UI's event
        // log. Pin them so a future variant rename cannot silently
        // break log scrapers / Grafana dashboards.
        assert_eq!(RefreshReason::InstanceJoined.as_str(), "instance_joined");
        assert_eq!(
            RefreshReason::ToolsListChanged.as_str(),
            "tools_list_changed"
        );
        assert_eq!(RefreshReason::Periodic.as_str(), "periodic");
    }

    #[test]
    fn refresh_reason_wire_matches_as_str() {
        // The JSON wire form must match `as_str()` so a single string
        // serves both log lines and JSON dumps.
        assert_eq!(
            serde_json::to_string(&RefreshReason::InstanceJoined).unwrap(),
            "\"instance_joined\""
        );
        assert_eq!(
            serde_json::to_string(&RefreshReason::ToolsListChanged).unwrap(),
            "\"tools_list_changed\""
        );
        assert_eq!(
            serde_json::to_string(&RefreshReason::Periodic).unwrap(),
            "\"periodic\""
        );

        let back: RefreshReason = serde_json::from_str("\"periodic\"").unwrap();
        assert_eq!(back, RefreshReason::Periodic);
    }
}
