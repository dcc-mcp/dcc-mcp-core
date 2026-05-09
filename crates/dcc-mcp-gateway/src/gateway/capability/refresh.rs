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
///
/// **Unloaded skills**: the backend's `POST /v1/search?loaded_only=false`
/// response carries two groups of hits — tools from *loaded* skills and
/// metadata stubs for *unloaded* skills (with `"loaded": false`).  The
/// unloaded group is forwarded to [`CapabilityIndex::set_unloaded_records`]
/// so that `search_tools` on the gateway side can surface them with a
/// `loaded: false` flag and a `next_step: load_skill` hint, enabling the
/// documented "discover → load → call" flow even when a skill has not been
/// activated on the backend yet.
pub async fn refresh_instance(
    index: &CapabilityIndex,
    http_client: &reqwest::Client,
    mcp_url: &str,
    instance_id: Uuid,
    dcc_type: &str,
    backend_timeout: Duration,
    reason: RefreshReason,
) -> bool {
    let (tools, unloaded_hints) = fetch_tools(http_client, mcp_url, backend_timeout).await;
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

    // Populate the unloaded-skill sentinel records so `search_tools`
    // surfaces tools from skills that are known to this backend but have
    // not been loaded yet.  Each hint triple is `(skill_name, tool_name,
    // summary)`.  The DCC type is the same as the refreshed instance's.
    //
    // Note: this call *replaces* the entire unloaded slice for every
    // refresh cycle.  That is intentional — after a `load_skill` the
    // backend no longer returns the newly-loaded tools with
    // `loaded=false`, so the stale sentinel rows are evicted naturally
    // on the next periodic or triggered refresh.
    if !unloaded_hints.is_empty() {
        let unloaded_records: Vec<super::record::CapabilityRecord> = unloaded_hints
            .into_iter()
            .filter_map(|(skill_name, tool_name, summary)| {
                if tool_name.is_empty() {
                    return None;
                }
                let mut rec = super::record::CapabilityRecord::from_skill_tool(
                    &skill_name,
                    &tool_name,
                    &summary,
                    dcc_type,
                );
                // Override the default nil instance_id with the real
                // instance so the gateway can route `load_skill` to the
                // correct backend when the agent acts on the hint.
                rec.instance_id = instance_id;
                Some(rec)
            })
            .collect();
        let unloaded_count = unloaded_records.len();
        index.set_unloaded_records(unloaded_records);
        tracing::debug!(
            instance = %instance_id,
            dcc = dcc_type,
            unloaded = unloaded_count,
            "capability index: populated unloaded-skill sentinel records",
        );
    } else {
        // No unloaded hints: clear any stale sentinel records that may
        // have been left from a previous refresh cycle when all skills
        // on this backend are now loaded.
        index.set_unloaded_records(Vec::new());
    }

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

    // ── unloaded record propagation (#858) ────────────────────────────────

    /// Verify that the unloaded hints from `fetch_tools` are forwarded to
    /// `CapabilityIndex::set_unloaded_records` so that `search_tools` on
    /// the gateway surfaces tools from skills that have not been loaded yet.
    ///
    /// This is a synchronous unit test that bypasses the async HTTP layer by
    /// exercising only the index-update logic directly — the async path is
    /// covered by the integration tests in `crates/dcc-mcp-http/tests/http/`.
    #[test]
    fn unloaded_hints_populate_index_unloaded_slot() {
        use crate::gateway::capability::{CapabilityRecord, search, search::SearchQuery};

        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(99);

        // Simulate the loaded-tool slice being upserted (one loaded tool).
        idx.upsert_instance(
            iid,
            vec![CapabilityRecord::new(
                crate::gateway::capability::tool_slug("maya", &iid, "project.save"),
                "project.save".into(),
                "project.save".into(),
                Some("maya-scene".into()),
                "save the current Maya scene",
                vec!["save".into()],
                "maya".into(),
                iid,
                false,
                true, // loaded
            )],
            InstanceFingerprint(42),
        );

        // Simulate what refresh_instance does with the unloaded hints.
        let unloaded_records: Vec<CapabilityRecord> = vec![(
            "maya-primitives",
            "maya-primitives.create_sphere",
            "Create a primitive sphere",
        )]
        .into_iter()
        .map(|(skill, tool, summary)| {
            let mut rec = CapabilityRecord::from_skill_tool(skill, tool, summary, "maya");
            rec.instance_id = iid; // override nil with real instance
            rec
        })
        .collect();
        idx.set_unloaded_records(unloaded_records);

        let snap = idx.snapshot();
        assert_eq!(
            snap.records.len(),
            2,
            "snapshot must include both loaded and unloaded records"
        );

        // search_tools with no filters must surface the unloaded tool.
        let hits = search::search(
            &snap,
            &SearchQuery {
                query: "create sphere".into(),
                dcc_type: Some("maya".into()),
                ..Default::default()
            },
        );
        assert!(
            !hits.is_empty(),
            "search_tools must find the unloaded create_sphere tool"
        );
        let sphere_hit = hits
            .iter()
            .find(|h| h.record.backend_tool.contains("create_sphere"));
        assert!(sphere_hit.is_some(), "create_sphere must appear in results");
        assert!(
            !sphere_hit.unwrap().record.loaded,
            "unloaded hit must have loaded=false"
        );

        // The loaded tool must also still be findable.
        let save_hits = search::search(
            &snap,
            &SearchQuery {
                query: "save scene".into(),
                dcc_type: Some("maya".into()),
                ..Default::default()
            },
        );
        assert!(
            save_hits.iter().any(|h| h.record.loaded),
            "loaded tools must still appear in results"
        );
    }
}
