//! Thread-safe capability index store.
//!
//! The index is organised **by instance** so a single backend losing
//! its SSE heartbeat can be evicted in O(live actions) without
//! touching any other backend's records. Readers take a cheap
//! `parking_lot::RwLock` read guard on the whole map; writers lock the
//! whole map too but only for the O(n) bulk replace of a single
//! instance's slice.
//!
//! The external callers never hold a lock guard across an `.await`
//! point — every public method returns an owned snapshot or a closure
//! result so the lock never escapes the call stack.
//!
//! # Read-side wire types (issue #845)
//!
//! [`InstanceFingerprint`] and [`IndexSnapshot`] were migrated to
//! [`dcc_mcp_gateway_core::capability::index`] so external Rust
//! tooling can consume the snapshot shape without depending on this
//! crate's tokio / axum / parking_lot footprint. They are re-exported
//! below to keep the historical
//! `crate::gateway::capability::index::{InstanceFingerprint,
//! IndexSnapshot}` paths working unchanged.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use parking_lot::RwLock;
use uuid::Uuid;

use super::record::CapabilityRecord;

pub use dcc_mcp_gateway_core::capability::index::{IndexSnapshot, InstanceFingerprint};

/// The canonical gateway-scoped capability index.
///
/// One `CapabilityIndex` is owned by `GatewayState` and shared with
/// every REST / MCP handler through `Arc<CapabilityIndex>`. Refresh
/// loops write through the same handle.
pub struct CapabilityIndex {
    /// Per-instance records, keyed by the UUID that
    /// [`dcc_mcp_transport`] assigns to each `ServiceEntry`.
    ///
    /// A `BTreeMap` keeps the ordering stable without paying for a
    /// `HashMap::iter` resort on every snapshot build — the index is
    /// small enough that the log-n insert cost is noise.
    inner: RwLock<InnerState>,
}

#[derive(Default)]
struct InnerState {
    per_instance: BTreeMap<Uuid, InstanceSlice>,
    /// Records built from unloaded skill metadata (discovered but not
    /// yet loaded). These are indexed so `search_tools` can find
    /// skills that aren't connected yet.
    unloaded: Arc<[CapabilityRecord]>,
}

struct InstanceSlice {
    records: Arc<[CapabilityRecord]>,
    fingerprint: InstanceFingerprint,
}

impl CapabilityIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(InnerState::default()),
        }
    }

    /// Replace every record owned by `instance_id` with `records`,
    /// returning the previous fingerprint (if any) so refresh loops
    /// can log transitions.
    ///
    /// Passing an empty `records` slice **removes** the instance from
    /// the index — the caller is expected to use
    /// [`Self::remove_instance`] in that case for clarity, but we
    /// accept an empty slice defensively so a racy refresh cannot
    /// leave dangling rows.
    pub fn upsert_instance(
        &self,
        instance_id: Uuid,
        records: Vec<CapabilityRecord>,
        fingerprint: InstanceFingerprint,
    ) -> Option<InstanceFingerprint> {
        let mut guard = self.inner.write();
        if records.is_empty() {
            return guard
                .per_instance
                .remove(&instance_id)
                .map(|s| s.fingerprint);
        }
        guard
            .per_instance
            .insert(
                instance_id,
                InstanceSlice {
                    records: Arc::from(records),
                    fingerprint,
                },
            )
            .map(|s| s.fingerprint)
    }

    /// Drop every record belonging to `instance_id`. Returns `true`
    /// if the instance was present.
    pub fn remove_instance(&self, instance_id: Uuid) -> bool {
        let mut guard = self.inner.write();
        guard.per_instance.remove(&instance_id).is_some()
    }

    /// Take an owned snapshot of the whole index. Intended for REST /
    /// MCP handlers that want a single stable view across a search /
    /// describe / call sequence.
    ///
    /// Includes both loaded (from live backends) and unloaded (from
    /// skill metadata) records so `search_tools` can discover skills
    /// that aren't connected yet.
    pub fn snapshot(&self) -> IndexSnapshot {
        let guard = self.inner.read();
        let loaded_count: usize = guard.per_instance.values().map(|s| s.records.len()).sum();
        let unloaded_count = guard.unloaded.len();
        let mut records: Vec<CapabilityRecord> = Vec::with_capacity(loaded_count + unloaded_count);
        let mut fingerprints: HashMap<Uuid, InstanceFingerprint> =
            HashMap::with_capacity(guard.per_instance.len());
        for (iid, slice) in guard.per_instance.iter() {
            fingerprints.insert(*iid, slice.fingerprint);
            records.extend_from_slice(&slice.records);
        }
        // Append unloaded skill records so they appear in search results.
        records.extend_from_slice(&guard.unloaded);
        // Stable order: by slug — the builder already sorts per-
        // instance, so this sort is effectively a merge of sorted
        // runs and stays cheap.
        records.sort_by(|a, b| a.tool_slug.cmp(&b.tool_slug));
        IndexSnapshot {
            records: Arc::from(records),
            fingerprints,
        }
    }

    /// Return the fingerprint previously stored for `instance_id`, if
    /// any. Used by the refresh loop to short-circuit when the
    /// backend reports an identical `tools/list` shape.
    pub fn fingerprint_for(&self, instance_id: Uuid) -> Option<InstanceFingerprint> {
        self.inner
            .read()
            .per_instance
            .get(&instance_id)
            .map(|s| s.fingerprint)
    }

    /// Count live records across every instance; diagnostics-only.
    pub fn total_records(&self) -> usize {
        self.inner
            .read()
            .per_instance
            .values()
            .map(|s| s.records.len())
            .sum()
    }

    /// Count tracked instances; diagnostics-only.
    pub fn instance_count(&self) -> usize {
        self.inner.read().per_instance.len()
    }

    /// Replace the unloaded-skill records with `records`.
    ///
    /// Called by the gateway refresh loop (or a dedicated skill-
    /// watcher task) after scanning the [`SkillCatalog`] for
    /// discovered-but-not-loaded skills.
    ///
    /// The caller is responsible for converting [`SkillMetadata`] to
    /// [`CapabilityRecord`] (using [`CapabilityRecord::from_skill_tool`])
    /// to avoid a direct dependency from `dcc-mcp-gateway` on
    /// `dcc-mcp-models`.
    pub fn set_unloaded_records(&self, records: Vec<CapabilityRecord>) {
        let mut guard = self.inner.write();
        // Keep unloaded records sorted so `snapshot()` doesn't need
        // to re-sort them separately.
        let mut sorted = records;
        sorted.sort_by(|a, b| a.tool_slug.cmp(&b.tool_slug));
        guard.unloaded = Arc::from(sorted);
    }

    /// Number of unloaded skill records currently indexed; diagnostics-only.
    pub fn unloaded_count(&self) -> usize {
        self.inner.read().unloaded.len()
    }
}

impl Default for CapabilityIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CapabilityIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapabilityIndex")
            .field("instances", &self.instance_count())
            .field("records", &self.total_records())
            .finish()
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use crate::gateway::capability::record::tool_slug;
    use crate::gateway::capability::search::{SearchQuery, search};

    fn rec(dcc: &str, id: Uuid, tool: &str, loaded: bool) -> CapabilityRecord {
        CapabilityRecord::new(
            tool_slug(dcc, &id, tool),
            tool.to_string(),
            tool.to_string(),
            None,
            "summary",
            Vec::new(),
            dcc.to_string(),
            id,
            false, // has_schema
            loaded,
            None,
        )
    }

    #[test]
    fn empty_index_has_zero_records() {
        let idx = CapabilityIndex::new();
        assert!(idx.snapshot().is_empty());
        assert_eq!(idx.total_records(), 0);
        assert_eq!(idx.instance_count(), 0);
    }

    #[test]
    fn upsert_then_snapshot_contains_records() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(0xabcdef01_2345_6789_abcd_ef0123456789);
        idx.upsert_instance(
            iid,
            vec![
                rec("maya", iid, "create_sphere", true),
                rec("maya", iid, "open", true),
            ],
            InstanceFingerprint(42),
        );
        let snap = idx.snapshot();
        assert_eq!(snap.records.len(), 2);
        assert_eq!(snap.fingerprints.get(&iid), Some(&InstanceFingerprint(42)));
    }

    #[test]
    fn upsert_with_empty_records_removes_instance() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        idx.upsert_instance(
            iid,
            vec![rec("maya", iid, "t", true)],
            InstanceFingerprint(1),
        );
        assert_eq!(idx.instance_count(), 1);
        // An empty upsert must not leave a half-populated slice.
        let prev = idx.upsert_instance(iid, Vec::new(), InstanceFingerprint(0));
        assert_eq!(prev, Some(InstanceFingerprint(1)));
        assert_eq!(idx.instance_count(), 0);
    }

    #[test]
    fn remove_instance_only_drops_its_own_records() {
        let idx = CapabilityIndex::new();
        let a = Uuid::from_u128(0xaaaa_aaaa_0000_0000_0000_0000_0000_0001);
        let b = Uuid::from_u128(0xbbbb_bbbb_0000_0000_0000_0000_0000_0001);
        idx.upsert_instance(a, vec![rec("maya", a, "t1", true)], InstanceFingerprint(1));
        idx.upsert_instance(
            b,
            vec![rec("blender", b, "t2", true)],
            InstanceFingerprint(2),
        );
        assert!(idx.remove_instance(a));
        let snap = idx.snapshot();
        assert_eq!(snap.records.len(), 1);
        assert_eq!(snap.records[0].dcc_type, "blender");
    }

    #[test]
    fn snapshot_order_is_stable_across_instance_merges() {
        // The REST/MCP wrappers show `records` to the user, so the
        // order must not depend on map iteration quirks.
        let idx = CapabilityIndex::new();
        let a = Uuid::from_u128(0x1111_1111_0000_0000_0000_0000_0000_0001);
        let b = Uuid::from_u128(0x2222_2222_0000_0000_0000_0000_0000_0001);
        idx.upsert_instance(
            a,
            vec![
                rec("blender", a, "z_action", true),
                rec("blender", a, "a_action", true),
            ],
            InstanceFingerprint(1),
        );
        idx.upsert_instance(
            b,
            vec![rec("maya", b, "m_action", true)],
            InstanceFingerprint(1),
        );
        let s1 = idx.snapshot();
        let s2 = idx.snapshot();
        let names: Vec<&str> = s1.records.iter().map(|r| r.tool_slug.as_str()).collect();
        let names2: Vec<&str> = s2.records.iter().map(|r| r.tool_slug.as_str()).collect();
        assert_eq!(names, names2, "snapshot order must be deterministic");
    }

    #[test]
    fn fingerprint_for_returns_last_upsert() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(7);
        assert_eq!(idx.fingerprint_for(iid), None);
        idx.upsert_instance(
            iid,
            vec![rec("python", iid, "foo", true)],
            InstanceFingerprint(9),
        );
        assert_eq!(idx.fingerprint_for(iid), Some(InstanceFingerprint(9)));
        idx.upsert_instance(
            iid,
            vec![rec("python", iid, "bar", true)],
            InstanceFingerprint(10),
        );
        assert_eq!(idx.fingerprint_for(iid), Some(InstanceFingerprint(10)));
    }

    #[test]
    fn find_by_slug_matches_exactly() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        let records = vec![
            rec("maya", iid, "create_sphere", true),
            rec("maya", iid, "open", true),
        ];
        idx.upsert_instance(iid, records, InstanceFingerprint(1));
        let snap = idx.snapshot();
        let expected_slug = tool_slug("maya", &iid, "create_sphere");
        assert!(snap.find_by_slug(&expected_slug).is_some());
        assert!(snap.find_by_slug("maya.abcdef01.not_there").is_none());
    }

    // ========================================================================
    // Unloaded skill records (issue #677)
    // ========================================================================

    #[test]
    fn unloaded_records_appear_in_snapshot() {
        let idx = CapabilityIndex::new();
        // No loaded instances yet.
        assert!(idx.snapshot().is_empty());
        assert_eq!(idx.unloaded_count(), 0);

        // Add unloaded skill records.
        let unloaded = vec![
            CapabilityRecord::from_skill_tool(
                "maya-geometry",
                "create_sphere",
                "Create a sphere in Maya",
                "maya",
                None,
            ),
            CapabilityRecord::from_skill_tool(
                "maya-geometry",
                "create_cube",
                "Create a cube in Maya",
                "maya",
                None,
            ),
        ];
        idx.set_unloaded_records(unloaded);
        assert_eq!(idx.unloaded_count(), 2);

        // Snapshot must include unloaded records.
        let snap = idx.snapshot();
        assert_eq!(snap.records.len(), 2);
        // Unloaded records have loaded=false.
        assert!(!snap.records[0].loaded);
        assert_eq!(snap.records[0].skill_name.as_deref(), Some("maya-geometry"));
    }

    #[test]
    fn snapshot_merges_loaded_and_unloaded() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        // Add a loaded instance.
        idx.upsert_instance(
            iid,
            vec![rec("maya", iid, "export_fbx", true)],
            InstanceFingerprint(1),
        );
        // Add unloaded skill records.
        let unloaded = vec![CapabilityRecord::from_skill_tool(
            "maya-animation",
            "set_keyframe",
            "Set a keyframe",
            "maya",
            None,
        )];
        idx.set_unloaded_records(unloaded);

        let snap = idx.snapshot();
        assert_eq!(snap.records.len(), 2);
        // Verify both loaded and unloaded are present.
        let loaded: Vec<&CapabilityRecord> = snap.records.iter().filter(|r| r.loaded).collect();
        let unloaded: Vec<&CapabilityRecord> = snap.records.iter().filter(|r| !r.loaded).collect();
        assert_eq!(loaded.len(), 1);
        assert_eq!(unloaded.len(), 1);
    }

    #[test]
    fn search_finds_unloaded_skills() {
        let idx = CapabilityIndex::new();
        // Only unloaded records.
        let unloaded = vec![
            CapabilityRecord::from_skill_tool(
                "maya-geometry",
                "create_sphere",
                "Create a sphere",
                "maya",
                None,
            ),
            CapabilityRecord::from_skill_tool(
                "blender-geometry",
                "create_cube",
                "Create a cube",
                "blender",
                None,
            ),
        ];
        idx.set_unloaded_records(unloaded);

        // Search should find unloaded skills.
        let snap = idx.snapshot();
        let hits = search(
            &snap,
            &SearchQuery {
                query: "create".into(),
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 2);

        // Filter by dcc_type should work for unloaded too.
        let hits = search(
            &snap,
            &SearchQuery {
                query: "create".into(),
                dcc_type: Some("maya".into()),
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.dcc_type, "maya");
    }

    #[test]
    fn loaded_only_filter_excludes_unloaded() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        // Add both loaded and unloaded.
        idx.upsert_instance(
            iid,
            vec![rec("maya", iid, "export_fbx", true)],
            InstanceFingerprint(1),
        );
        let unloaded = vec![CapabilityRecord::from_skill_tool(
            "maya-geometry",
            "create_sphere",
            "Create a sphere",
            "maya",
            None,
        )];
        idx.set_unloaded_records(unloaded);

        let snap = idx.snapshot();
        // `loaded_only: Some(true)` must exclude unloaded records.
        // Both records are maya DCC, so the filter is the only thing
        // distinguishing them.
        let hits = search(
            &snap,
            &SearchQuery {
                query: String::new(), // empty query = browse all records
                dcc_type: Some("maya".into()),
                loaded_only: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].record.loaded);

        // Without loaded_only, both are visible.
        let hits_all = search(
            &snap,
            &SearchQuery {
                query: String::new(),
                dcc_type: Some("maya".into()),
                ..Default::default()
            },
        );
        assert_eq!(hits_all.len(), 2);
    }

    // ── Property-based tests (#846) ────────────────────────────────────────
    //
    // These verify the registry-style "laws" called out in #846 for the
    // CapabilityIndex, which is the gateway-side analogue of a Registry
    // trait:
    //
    //   * upsert(id, recs) then fingerprint_for(id) returns Some(fp)
    //   * remove(id) then fingerprint_for(id) returns None
    //   * snapshot order is independent of upsert order
    //   * total_records == sum of per-instance record counts
    //
    // Adding these property tests stakes a flag for the wider trait-law
    // work; per-trait property tests for ValidationStrategy / DccAdapter
    // depend on the trait splits in #843 and follow in later PRs.

    use proptest::prelude::*;

    /// Generate a `Uuid` from arbitrary bytes — a uniform sample of the
    /// id space lets proptest cover the BTreeMap ordering without
    /// special-casing low ids.
    fn arb_uuid() -> impl Strategy<Value = Uuid> {
        any::<u128>().prop_map(Uuid::from_u128)
    }

    /// Generate a `(dcc_type, tool_name)` pair using the same restricted
    /// alphabet the `tool_slug` helper expects.
    fn arb_tool() -> impl Strategy<Value = (String, String)> {
        ("[a-z]{1,8}", "[a-z_][a-z0-9_]{0,15}").prop_map(|(d, t)| (d, t))
    }

    /// Build a deterministic vector of records for one instance.
    fn arb_records(iid: Uuid) -> impl Strategy<Value = Vec<CapabilityRecord>> {
        proptest::collection::vec(arb_tool(), 1..6).prop_map(move |tools| {
            tools
                .into_iter()
                .map(|(dcc, tool)| rec(&dcc, iid, &tool, true))
                .collect()
        })
    }

    proptest! {
        /// Registry law: `upsert(id, recs)` followed by
        /// `fingerprint_for(id)` returns `Some(fp)`. Empty `recs`
        /// is the documented "remove" path and is excluded by
        /// generating non-empty vectors.
        #[test]
        fn prop_upsert_then_fingerprint_for_returns_some(
            iid in arb_uuid(),
            fp in any::<u64>(),
        ) {
            let idx = CapabilityIndex::new();
            let recs = vec![rec("maya", iid, "t", true)];
            let prev = idx.upsert_instance(iid, recs, InstanceFingerprint(fp));
            prop_assert_eq!(prev, None);
            prop_assert_eq!(idx.fingerprint_for(iid), Some(InstanceFingerprint(fp)));
        }

        /// Registry law: `remove(id)` followed by `fingerprint_for(id)`
        /// returns `None`. Removing an absent id is a no-op (returns false).
        #[test]
        fn prop_remove_then_fingerprint_for_returns_none(
            iid in arb_uuid(),
            fp in any::<u64>(),
        ) {
            let idx = CapabilityIndex::new();
            idx.upsert_instance(
                iid,
                vec![rec("maya", iid, "t", true)],
                InstanceFingerprint(fp),
            );
            prop_assert!(idx.remove_instance(iid));
            prop_assert_eq!(idx.fingerprint_for(iid), None);
            // Removing a second time is a no-op.
            prop_assert!(!idx.remove_instance(iid));
        }

        /// Registry law: `upsert(id, recs)` then `find_by_slug(slug)`
        /// resolves every slug carried by `recs`.
        #[test]
        fn prop_upsert_then_find_by_slug_resolves_every_record(
            iid in arb_uuid(),
            recs in arb_uuid().prop_flat_map(arb_records),
            fp in any::<u64>(),
        ) {
            let idx = CapabilityIndex::new();
            // Re-key the records so they share `iid` (the strategy
            // returned them keyed to its own internal uuid).
            let recs: Vec<CapabilityRecord> = recs
                .into_iter()
                .map(|r| {
                    let mut r2 = r.clone();
                    r2.instance_id = iid;
                    r2.tool_slug = tool_slug(&r2.dcc_type, &iid, &r2.backend_tool);
                    r2
                })
                .collect();
            let slugs: Vec<String> = recs.iter().map(|r| r.tool_slug.clone()).collect();
            idx.upsert_instance(iid, recs, InstanceFingerprint(fp));
            let snap = idx.snapshot();
            for slug in &slugs {
                prop_assert!(
                    snap.find_by_slug(slug).is_some(),
                    "slug {} must resolve after upsert",
                    slug,
                );
            }
        }

        /// Snapshot order is a pure function of the slugs in the index —
        /// independent of upsert order. Re-inserting the same instances
        /// in reversed order must yield the same snapshot record sequence.
        #[test]
        fn prop_snapshot_order_is_independent_of_upsert_order(
            ids in proptest::collection::vec(arb_uuid(), 1..5),
        ) {
            // Ensure unique ids — duplicates would shadow the second
            // upsert and skew the comparison.
            let mut uniq = ids.clone();
            uniq.sort();
            uniq.dedup();
            prop_assume!(uniq.len() == ids.len());

            let payloads: Vec<Vec<CapabilityRecord>> = ids
                .iter()
                .enumerate()
                .map(|(n, id)| vec![rec("maya", *id, &format!("t{n}"), true)])
                .collect();

            let idx_a = CapabilityIndex::new();
            for (id, recs) in ids.iter().zip(payloads.iter()) {
                idx_a.upsert_instance(*id, recs.clone(), InstanceFingerprint(0));
            }
            let snap_a = idx_a.snapshot();

            let idx_b = CapabilityIndex::new();
            for (id, recs) in ids.iter().rev().zip(payloads.iter().rev()) {
                idx_b.upsert_instance(*id, recs.clone(), InstanceFingerprint(0));
            }
            let snap_b = idx_b.snapshot();

            let names_a: Vec<&str> = snap_a.records.iter().map(|r| r.tool_slug.as_str()).collect();
            let names_b: Vec<&str> = snap_b.records.iter().map(|r| r.tool_slug.as_str()).collect();
            prop_assert_eq!(names_a, names_b);
        }

        /// Accounting law: `total_records` equals the sum of per-instance
        /// record counts after a sequence of upserts.
        #[test]
        fn prop_total_records_matches_sum_of_upsert_sizes(
            sizes in proptest::collection::vec(1usize..6, 1..5),
        ) {
            let idx = CapabilityIndex::new();
            let mut expected = 0;
            for (n, k) in sizes.iter().enumerate() {
                let id = Uuid::from_u128(0xa000_0000_0000_0000_0000_0000_0000_0000 + n as u128);
                let recs: Vec<CapabilityRecord> = (0..*k)
                    .map(|j| rec("maya", id, &format!("t{n}_{j}"), true))
                    .collect();
                expected += k;
                idx.upsert_instance(id, recs, InstanceFingerprint(0));
            }
            prop_assert_eq!(idx.total_records(), expected);
            prop_assert_eq!(idx.instance_count(), sizes.len());
        }
    }
}
