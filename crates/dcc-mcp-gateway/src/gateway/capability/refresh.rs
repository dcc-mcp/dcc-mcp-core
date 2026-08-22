//! Lifecycle glue between the backend registry and the capability
//! index.
//!
//! The refresh layer is intentionally thin: it fetches backend state,
//! delegates deterministic record building and refresh-slice assembly to
//! `dcc-mcp-gateway-core`, then applies the result to the concurrent index.
//! Resilience, diagnostics, metrics, and lifecycle tracing stay here.
//!
//! # Wire type relocation (issue #845)
//!
//! Pure refresh types and assembly rules live in
//! [`dcc_mcp_gateway_core::capability::refresh`] so tests and tooling do
//! not depend on this crate's Tokio/Reqwest footprint. `RefreshReason` is
//! re-exported below to keep its historical path working unchanged.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use dcc_mcp_gateway_core::capability::build_refresh_records;

use crate::gateway::backend_client::try_fetch_tools;
use crate::gateway::instance_diagnostics::InstanceDiagnosticsStore;
use crate::gateway::resilience::GatewayResilienceState;

use super::builder::{
    BuildInput, backend_job_status_tool, build_records_from_backend, is_backend_job_tool,
};
use super::index::CapabilityIndex;

pub use dcc_mcp_gateway_core::capability::refresh::RefreshReason;

const PROFILING_TARGET: &str = "dcc_mcp::profiling";

/// Refresh one instance's slice of the index by fetching its current
/// `tools/list` from `mcp_url`.
///
/// Returns `true` when the index was actually updated (new, removed,
/// or changed fingerprint), `false` when the fingerprint matched and
/// the write was short-circuited — OR when the backend was unreachable
/// (error path preserves the existing index to avoid losing previously
/// discovered tools for this instance).
///
/// **Error safety**: when `try_fetch_tools` fails (network error, HTTP
/// error, etc.) the function returns `false` without touching the index
/// at all. This prevents a transient backend failure from wiping the
/// instance's tool records (issue #1659).
///
/// **Unloaded skills**: the backend's `POST /v1/search?loaded_only=false`
/// response carries two groups of hits — tools from *loaded* skills and
/// metadata stubs for *unloaded* skills (with `"loaded": false`). The
/// unloaded group is stored in this instance's own slice with the real
/// instance UUID in both `instance_id` and `tool_slug`, so same-DCC
/// multi-instance gateways do not collapse every hint into one global
/// `dcc.00000000.*` row.
#[allow(clippy::too_many_arguments)]
pub async fn refresh_instance(
    index: &CapabilityIndex,
    http_client: &reqwest::Client,
    resilience: &GatewayResilienceState,
    mcp_url: &str,
    instance_id: Uuid,
    dcc_type: &str,
    backend_timeout: Duration,
    reason: RefreshReason,
    diag_store: Option<&InstanceDiagnosticsStore>,
) -> bool {
    let (mut tools, unloaded_hints) =
        match try_fetch_tools(http_client, resilience, mcp_url, backend_timeout).await {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!(
                    instance = %instance_id,
                    dcc = dcc_type,
                    error = %e,
                    "fetch_tools failed during refresh_instance; preserving existing index"
                );
                crate::gateway::metrics::record_gateway_backend_error_kind("fetch_tools");
                if let Some(store) = diag_store {
                    store.record_call_error(instance_id, "fetch_tools", &e);
                }
                // Return early — do NOT upsert empty records. An empty upsert
                // would delete this instance's entire slice from the index,
                // losing all previously discovered tools (issue #1659).
                return false;
            }
        };
    if !tools.iter().any(|tool| is_backend_job_tool(&tool.name)) {
        tools.push(backend_job_status_tool());
    }
    let outcome = tracing::trace_span!(
        target: PROFILING_TARGET,
        "capability.discovery.build_records",
        instance = %instance_id,
        tools = tools.len(),
    )
    .in_scope(|| {
        build_records_from_backend(BuildInput {
            instance_id,
            dcc_type,
            backend_tools: &tools,
        })
    });
    let refresh = tracing::trace_span!(
        target: PROFILING_TARGET,
        "capability.discovery.fingerprint",
        instance = %instance_id,
        loaded = outcome.records.len(),
        unloaded_hints = unloaded_hints.len(),
    )
    .in_scope(|| build_refresh_records(outcome, unloaded_hints, instance_id, dcc_type));

    // Short-circuit when nothing changed. This is the common path —
    // most periodic refreshes find an identical tool list.
    let previous = index.fingerprint_for(instance_id);
    if refresh.is_unchanged(previous) {
        tracing::trace!(
            instance = %instance_id,
            reason = reason.as_str(),
            records = refresh.records.len(),
            "capability index: no-op refresh (fingerprint unchanged)",
        );
        return false;
    }

    let records_len = refresh.records.len();
    let fingerprint = refresh.fingerprint;
    let loaded_count = refresh.loaded;
    let unloaded_count = refresh.unloaded;
    let skipped_count = refresh.skipped;
    let records = refresh.records;
    let prev = tracing::trace_span!(
        target: PROFILING_TARGET,
        "capability.discovery.upsert",
        instance = %instance_id,
        records = records_len,
    )
    .in_scope(|| index.upsert_instance(instance_id, records, fingerprint));

    tracing::info!(
        instance = %instance_id,
        dcc = dcc_type,
        reason = reason.as_str(),
        records = records_len,
        loaded = loaded_count,
        unloaded = unloaded_count,
        skipped = skipped_count,
        fingerprint_changed = ?prev.map(|f| f != fingerprint),
        "capability index: refreshed",
    );
    true
}

/// Drop every record for `instance_id`. Safe to call even if the
/// instance was never indexed.
pub fn remove_instance(index: &Arc<CapabilityIndex>, instance_id: Uuid) -> bool {
    remove_instance_with_status(index, instance_id, "deregistered")
}

/// Drop every record and retain the last known lifecycle state.
pub fn remove_instance_with_status(
    index: &Arc<CapabilityIndex>,
    instance_id: Uuid,
    previous_status: &str,
) -> bool {
    let removed = index.remove_instance_with_status(instance_id, previous_status);
    if removed {
        tracing::info!(
            instance = %instance_id,
            previous_status,
            "capability index: dropped instance",
        );
    }
    removed
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use dcc_mcp_gateway_core::capability::{
        BuildOutcome, CapabilityRecord, UnloadedCapabilityHint, compute_fingerprint,
        index::InstanceFingerprint,
    };

    fn build_unloaded_records(
        hints: Vec<UnloadedCapabilityHint>,
        instance_id: Uuid,
        dcc_type: &str,
    ) -> Vec<CapabilityRecord> {
        build_refresh_records(BuildOutcome::default(), hints, instance_id, dcc_type).records
    }

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
                rank_layer: None,
                rank_path_source: None,
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
                    rank_layer: None,
                    rank_path_source: None,
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
