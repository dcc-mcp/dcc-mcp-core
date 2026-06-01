//! Lifecycle glue between the backend registry and the capability
//! index.
//!
//! The refresh layer is intentionally thin: it delegates the pure
//! work of "given a backend tools/list, produce records" to
//! [`super::builder::build_records_from_backend`] and only adds the
//! I/O — `fetch_tools` on demand, and tracing on lifecycle
//! transitions.
//!
//! # Wire type relocation (issue #845)
//!
//! [`RefreshReason`] was migrated to
//! [`dcc_mcp_gateway_core::capability::refresh`] so external Rust
//! tooling (admin dashboards, CLI inspectors, log scrapers) can
//! match on the reason without depending on this crate's tokio /
//! reqwest footprint. Re-exported below to keep the historical
//! `crate::gateway::capability::refresh::RefreshReason` path working
//! unchanged.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::gateway::backend_client::{UnloadedCapabilityHint, fetch_tools};

use super::builder::{BuildInput, build_records_from_backend};
use super::index::{CapabilityIndex, InstanceFingerprint};
use super::record::{CapabilityRecord, tool_slug};

pub use dcc_mcp_gateway_core::capability::refresh::RefreshReason;

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
/// metadata stubs for *unloaded* skills (with `"loaded": false`). The
/// unloaded group is stored in this instance's own slice with the real
/// instance UUID in both `instance_id` and `tool_slug`, so same-DCC
/// multi-instance gateways do not collapse every hint into one global
/// `dcc.00000000.*` row.
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
    let mut records = outcome.records;
    let loaded_records_len = records.len();
    let mut unloaded_records = build_unloaded_records(unloaded_hints, instance_id, dcc_type);
    let unloaded_count = unloaded_records.len();
    records.append(&mut unloaded_records);
    records.sort_by(|a, b| a.tool_slug.cmp(&b.tool_slug));
    let fingerprint = compute_fingerprint(&records);

    // Short-circuit when nothing changed. This is the common path —
    // most periodic refreshes find an identical tool list.
    if let Some(prev) = index.fingerprint_for(instance_id)
        && prev == fingerprint
        && !records.is_empty()
    {
        tracing::trace!(
            instance = %instance_id,
            reason = reason.as_str(),
            records = records.len(),
            "capability index: no-op refresh (fingerprint unchanged)",
        );
        return false;
    }

    let records_len = records.len();
    let prev = index.upsert_instance(instance_id, records, fingerprint);

    tracing::info!(
        instance = %instance_id,
        dcc = dcc_type,
        reason = reason.as_str(),
        records = records_len,
        loaded = loaded_records_len,
        unloaded = unloaded_count,
        skipped = outcome.skipped,
        fingerprint_changed = ?prev.map(|f| f != fingerprint),
        "capability index: refreshed",
    );
    true
}

fn build_unloaded_records(
    unloaded_hints: Vec<UnloadedCapabilityHint>,
    instance_id: Uuid,
    dcc_type: &str,
) -> Vec<CapabilityRecord> {
    unloaded_hints
        .into_iter()
        .filter_map(|hint| {
            if hint.tool_name.is_empty() {
                return None;
            }
            let mut rec = CapabilityRecord::from_skill_tool(
                &hint.skill_name,
                &hint.tool_name,
                &hint.summary,
                dcc_type,
                hint.tool_group.clone(),
            )
            .with_available_groups(hint.available_groups)
            .with_search_tokens(hint.search_tokens);
            rec.instance_id = instance_id;
            rec.tool_slug = tool_slug(dcc_type, &instance_id, &hint.tool_name);
            Some(rec)
        })
        .collect()
}

fn compute_fingerprint(records: &[CapabilityRecord]) -> InstanceFingerprint {
    let mut hasher = DefaultHasher::new();
    for r in records {
        r.tool_slug.hash(&mut hasher);
        r.has_schema.hash(&mut hasher);
        r.summary.hash(&mut hasher);
        r.loaded.hash(&mut hasher);
        r.tool_group.hash(&mut hasher);
        for group in &r.available_groups {
            group.name.hash(&mut hasher);
            group.default_active.hash(&mut hasher);
            group.active.hash(&mut hasher);
        }
        for t in &r.tags {
            t.hash(&mut hasher);
        }
        for t in &r.search_tokens {
            t.hash(&mut hasher);
        }
    }
    InstanceFingerprint(hasher.finish())
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
                None,
            )],
            InstanceFingerprint(1),
        );
        assert!(remove_instance(&idx, iid));
        assert_eq!(idx.instance_count(), 0);
    }

    // ── unloaded record propagation (#858) ────────────────────────────────

    /// Verify that unloaded hints are stored with the owning instance's
    /// routing slug so same-DCC multi-instance search does not collapse
    /// to a single `dcc.00000000.*` row.
    ///
    /// This is a synchronous unit test that bypasses the async HTTP layer by
    /// exercising only the index-update logic directly — the async path is
    /// covered by the integration tests in `crates/dcc-mcp-http/tests/http/`.
    #[test]
    fn unloaded_hints_use_instance_scoped_slugs() {
        use crate::gateway::backend_client::UnloadedCapabilityHint;
        use crate::gateway::capability::{CapabilityRecord, search, search::SearchQuery};

        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(0x0000_0063_0000_0000_0000_0000_0000_0001);

        // Simulate the loaded-tool slice being upserted (one loaded tool).
        idx.upsert_instance(
            iid,
            vec![CapabilityRecord::new(
                crate::gateway::capability::tool_slug("maya", &iid, "project_save"),
                "project_save".into(),
                "project_save".into(),
                Some("maya-scene".into()),
                "save the current Maya scene",
                vec!["save".into()],
                "maya".into(),
                iid,
                false,
                true, // loaded
                None,
            )],
            InstanceFingerprint(42),
        );

        let mut records = idx.snapshot().records.to_vec();
        records.extend(build_unloaded_records(
            vec![UnloadedCapabilityHint {
                skill_name: "maya-primitives".to_string(),
                tool_name: "maya_primitives__create_sphere".to_string(),
                summary: "Create a primitive sphere".to_string(),
                search_tokens: Vec::new(),
                available_groups: Vec::new(),
                tool_group: None,
            }],
            iid,
            "maya",
        ));
        let fp = compute_fingerprint(&records);
        idx.upsert_instance(iid, records, fp);

        let snap = idx.snapshot();
        assert_eq!(
            snap.records.len(),
            2,
            "snapshot must include both loaded and unloaded records"
        );
        assert!(
            snap.records
                .iter()
                .all(|r| r.tool_slug.contains(".00000063.")),
            "instance-scoped records must use the real UUID prefix; got {:?}",
            snap.records
                .iter()
                .map(|r| r.tool_slug.as_str())
                .collect::<Vec<_>>()
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

    #[test]
    fn unloaded_hints_from_two_maya_instances_remain_distinct() {
        use crate::gateway::backend_client::UnloadedCapabilityHint;

        let a = Uuid::from_u128(0xaaaa_0000_0000_0000_0000_0000_0000_0001);
        let b = Uuid::from_u128(0xbbbb_0000_0000_0000_0000_0000_0000_0001);
        let idx = CapabilityIndex::new();

        for iid in [a, b] {
            let records = build_unloaded_records(
                vec![UnloadedCapabilityHint {
                    skill_name: "maya-primitives".to_string(),
                    tool_name: "maya_primitives__create_sphere".to_string(),
                    summary: "Create a primitive sphere".to_string(),
                    search_tokens: Vec::new(),
                    available_groups: Vec::new(),
                    tool_group: None,
                }],
                iid,
                "maya",
            );
            let fp = compute_fingerprint(&records);
            idx.upsert_instance(iid, records, fp);
        }

        let snap = idx.snapshot();
        assert_eq!(snap.records.len(), 2);
        let slugs: Vec<&str> = snap.records.iter().map(|r| r.tool_slug.as_str()).collect();
        assert_eq!(
            slugs,
            vec![
                "maya.aaaa0000.maya_primitives__create_sphere",
                "maya.bbbb0000.maya_primitives__create_sphere",
            ]
        );
    }
}
