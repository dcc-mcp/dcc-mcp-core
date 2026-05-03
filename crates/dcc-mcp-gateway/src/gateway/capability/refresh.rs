//! Lifecycle glue between the backend registry and the capability
//! index.
//!
//! The refresh layer is intentionally thin: it delegates the pure
//! work of "given a backend tools/list, produce records" to
//! [`super::builder::build_records_from_backend`] and only adds the
//! I/O — `fetch_tools` on demand, and tracing on lifecycle
//! transitions.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::gateway::backend_client::fetch_tools;

use super::builder::{BuildInput, build_records_from_backend};
use super::index::CapabilityIndex;

/// Why a refresh cycle is running. Surfaced through `tracing::info!`
/// so operators can correlate an index update with the event that
/// triggered it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// String label suitable for span tags.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InstanceJoined => "instance_joined",
            Self::ToolsListChanged => "tools_list_changed",
            Self::Periodic => "periodic",
        }
    }
}

/// Refresh one instance's slice of the index by fetching its current
/// `tools/list` from `mcp_url`.
///
/// Returns `true` when the index was actually updated (new, removed,
/// or changed fingerprint), `false` when the fingerprint matched and
/// the write was short-circuited. The bool is surfaced so diagnostics
/// can count "no-op refreshes" without sampling tracing spans.
pub async fn refresh_instance(
    index: &CapabilityIndex,
    http_client: &reqwest::Client,
    mcp_url: &str,
    instance_id: Uuid,
    dcc_type: &str,
    backend_timeout: Duration,
    reason: RefreshReason,
) -> bool {
    let tools = fetch_tools(http_client, mcp_url, backend_timeout).await;
    let outcome = build_records_from_backend(BuildInput {
        instance_id,
        dcc_type,
        backend_tools: &tools,
    });

    // Short-circuit when nothing changed. This is the common path —
    // most periodic refreshes find an identical tool list.
    if let Some(prev) = index.fingerprint_for(instance_id)
        && prev == outcome.fingerprint
        && !outcome.records.is_empty()
    {
        tracing::trace!(
            instance = %instance_id,
            reason = reason.as_str(),
            records = outcome.records.len(),
            "capability index: no-op refresh (fingerprint unchanged)",
        );
        return false;
    }

    let records_len = outcome.records.len();
    let prev = index.upsert_instance(instance_id, outcome.records, outcome.fingerprint);
    tracing::info!(
        instance = %instance_id,
        dcc = dcc_type,
        reason = reason.as_str(),
        records = records_len,
        skipped = outcome.skipped,
        fingerprint_changed = ?prev.map(|f| f != outcome.fingerprint),
        "capability index: refreshed",
    );
    true
}

/// Drop every record for `instance_id`. Safe to call even if the
/// instance was never indexed.
pub fn remove_instance(index: &Arc<CapabilityIndex>, instance_id: Uuid) -> bool {
    let removed = index.remove_instance(instance_id);
    if removed {
        tracing::info!(
            instance = %instance_id,
            "capability index: dropped instance",
        );
    }
    removed
}

#[cfg(test)]
mod unit_tests {
    use super::super::index::InstanceFingerprint;
    use super::*;

    #[test]
    fn refresh_reason_label_is_stable() {
        // Diagnostic tooling and span tags depend on these strings.
        assert_eq!(RefreshReason::InstanceJoined.as_str(), "instance_joined");
        assert_eq!(
            RefreshReason::ToolsListChanged.as_str(),
            "tools_list_changed",
        );
        assert_eq!(RefreshReason::Periodic.as_str(), "periodic");
    }

    #[test]
    fn remove_missing_instance_is_noop() {
        let idx = Arc::new(CapabilityIndex::new());
        assert!(!remove_instance(&idx, Uuid::from_u128(1)));
    }

    #[test]
    fn remove_existing_instance_returns_true() {
        let idx = Arc::new(CapabilityIndex::new());
        let iid = Uuid::from_u128(1);
        idx.upsert_instance(
            iid,
            vec![crate::gateway::capability::CapabilityRecord::new(
                crate::gateway::capability::tool_slug("maya", &iid, "a"),
                "a".into(),
                "a".into(),
                None,
                "",
                vec![],
                "maya".into(),
                iid,
                false, // has_schema
                true,  // loaded
            )],
            InstanceFingerprint(1),
        );
        assert!(remove_instance(&idx, iid));
        assert_eq!(idx.instance_count(), 0);
    }
}
