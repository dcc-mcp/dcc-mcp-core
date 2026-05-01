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

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use parking_lot::RwLock;
use uuid::Uuid;

use super::record::CapabilityRecord;

/// Stable fingerprint of one instance's contribution to the index.
///
/// The fingerprint is used by [`super::refresh`] to short-circuit
/// rebuilds when the backend replied with the exact same
/// `tools/list` shape as the previous refresh — in that case there is
/// nothing to update and we can skip the full swap.
///
/// The representation is deliberately small: the builder computes a
/// content hash of the backend's tool list, and the index stores just
/// that hash so comparisons are O(1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct InstanceFingerprint(pub u64);

/// Owned snapshot of the index returned to REST / MCP callers.
///
/// Cloning an `IndexSnapshot` is cheap: the backing `Arc<[...]>`
/// shares the underlying allocation across every reader that took the
/// snapshot within the same window, so a `search_tools` call handling
/// a large `limit` does not pay for a deep copy.
#[derive(Debug, Clone, Default)]
pub struct IndexSnapshot {
    /// All live capability records, ordered by `(dcc_type, slug)` for
    /// a stable human-readable output — the builder places them in
    /// that order on every swap so callers do not need to sort.
    pub records: Arc<[CapabilityRecord]>,
    /// Per-instance fingerprint seen at snapshot time. Included so
    /// diagnostics can trace which `refresh_instance` cycles produced
    /// which snapshot.
    pub fingerprints: HashMap<Uuid, InstanceFingerprint>,
}

impl IndexSnapshot {
    /// Convenience predicate for diagnostics.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Resolve a capability record by its slug in O(n). The index is
    /// bounded (every live backend × ~tens of actions) so the linear
    /// scan is the right default; a hash map would add per-refresh
    /// cost without a proven win until indices exceed ~10 k records.
    pub fn find_by_slug(&self, slug: &str) -> Option<&CapabilityRecord> {
        self.records.iter().find(|r| r.tool_slug == slug)
    }
}

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
    pub fn snapshot(&self) -> IndexSnapshot {
        let guard = self.inner.read();
        let mut records: Vec<CapabilityRecord> =
            Vec::with_capacity(guard.per_instance.values().map(|s| s.records.len()).sum());
        let mut fingerprints: HashMap<Uuid, InstanceFingerprint> =
            HashMap::with_capacity(guard.per_instance.len());
        for (iid, slice) in guard.per_instance.iter() {
            fingerprints.insert(*iid, slice.fingerprint);
            records.extend_from_slice(&slice.records);
        }
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

    fn rec(dcc: &str, id: Uuid, tool: &str) -> CapabilityRecord {
        CapabilityRecord::new(
            tool_slug(dcc, &id, tool),
            tool.to_string(),
            None,
            "summary",
            Vec::new(),
            dcc.to_string(),
            id,
            false,
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
            vec![rec("maya", iid, "create_sphere"), rec("maya", iid, "open")],
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
        idx.upsert_instance(iid, vec![rec("maya", iid, "t")], InstanceFingerprint(1));
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
        idx.upsert_instance(a, vec![rec("maya", a, "t1")], InstanceFingerprint(1));
        idx.upsert_instance(b, vec![rec("blender", b, "t2")], InstanceFingerprint(2));
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
            vec![rec("blender", a, "z_action"), rec("blender", a, "a_action")],
            InstanceFingerprint(1),
        );
        idx.upsert_instance(b, vec![rec("maya", b, "m_action")], InstanceFingerprint(1));
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
        idx.upsert_instance(iid, vec![rec("python", iid, "foo")], InstanceFingerprint(9));
        assert_eq!(idx.fingerprint_for(iid), Some(InstanceFingerprint(9)));
        idx.upsert_instance(
            iid,
            vec![rec("python", iid, "bar")],
            InstanceFingerprint(10),
        );
        assert_eq!(idx.fingerprint_for(iid), Some(InstanceFingerprint(10)));
    }

    #[test]
    fn find_by_slug_matches_exactly() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        let records = vec![rec("maya", iid, "create_sphere"), rec("maya", iid, "open")];
        idx.upsert_instance(iid, records, InstanceFingerprint(1));
        let snap = idx.snapshot();
        let expected_slug = tool_slug("maya", &iid, "create_sphere");
        assert!(snap.find_by_slug(&expected_slug).is_some());
        assert!(snap.find_by_slug("maya.abcdef01.not_there").is_none());
    }
}
