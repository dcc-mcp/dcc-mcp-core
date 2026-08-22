//! Pure state and value types for the gateway capability index.
//!
//! These are the *read-side* shapes a REST or MCP handler sees when it
//! takes a snapshot of the capability index. [`CapabilityIndexState`]
//! owns the lock-free domain state and mutation rules. The gateway
//! crate wraps it in synchronization and keeps refresh coordination,
//! clocks, and async gates at the application boundary (issue #845).
//!
//! # Why split snapshot from index
//!
//! The snapshot is what every search / dispatch / diagnostics path
//! actually consumes; it is a small, immutable, `Clone`-cheap value
//! built atop `Arc<[CapabilityRecord]>`. Moving it here lets external
//! Rust tooling (CLI inspectors, integration tests, REST clients that
//! cache the most recent snapshot) work with the gateway index shape
//! without depending on the gateway crate's tokio / axum / parking_lot
//! footprint.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use uuid::Uuid;

use super::record::CapabilityRecord;

const MAX_INSTANCE_TOMBSTONES: usize = 256;

/// Stable fingerprint of one instance's contribution to the index.
///
/// The fingerprint is used by the gateway's refresh loop to
/// short-circuit rebuilds when the backend replied with the exact
/// same `tools/list` shape as the previous refresh — in that case
/// there is nothing to update and we can skip the full swap.
///
/// The representation is deliberately small: the builder computes a
/// content hash of the backend's tool list, and the index stores just
/// that hash so comparisons are O(1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct InstanceFingerprint(pub u64);

/// Compute a stable content fingerprint over every identity-relevant
/// field of `records`.
///
/// The union of fields hashed here is the canonical set — every field
/// whose change should cause the refresh loop to rebuild the instance
/// slice. The two historical private implementations in
/// `dcc-mcp-gateway`'s `builder.rs` and `refresh.rs` hashed different
/// subsets; this function is the single source of truth.
///
/// Callers that already hold a `BuildOutcome` may use its pre-computed
/// `fingerprint` field instead of calling this again.
#[must_use]
pub fn compute_fingerprint(records: &[CapabilityRecord]) -> InstanceFingerprint {
    let mut hasher = DefaultHasher::new();
    for r in records {
        r.tool_slug.hash(&mut hasher);
        r.skill_name.hash(&mut hasher);
        r.has_schema.hash(&mut hasher);
        r.summary.hash(&mut hasher);
        r.loaded.hash(&mut hasher);
        r.tool_group.hash(&mut hasher);
        r.annotations.hash(&mut hasher);
        r.metadata.hash(&mut hasher);
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
        r.rank_layer.hash(&mut hasher);
        r.rank_path_source.hash(&mut hasher);
    }
    InstanceFingerprint(hasher.finish())
}

/// Owned snapshot of the capability index returned to REST / MCP
/// callers.
///
/// Cloning an `IndexSnapshot` is cheap: the backing `Arc<[...]>`
/// shares the underlying allocation across every reader that took the
/// snapshot within the same window, so a `search_tools` call handling
/// a large `limit` does not pay for a deep copy.
#[derive(Debug, Clone, Default)]
pub struct IndexSnapshot {
    /// All live capability records, ordered by `(dcc_type, slug)` for
    /// a stable human-readable output — the gateway builder places
    /// them in that order on every swap so callers do not need to
    /// sort.
    pub records: Arc<[CapabilityRecord]>,
    /// Per-instance fingerprint seen at snapshot time. Included so
    /// diagnostics can trace which `refresh_instance` cycles produced
    /// which snapshot.
    pub fingerprints: HashMap<Uuid, InstanceFingerprint>,
}

/// Bounded lifecycle provenance for an instance removed from the live index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceTombstone {
    /// Removed gateway instance.
    pub instance_id: Uuid,
    /// Canonical DCC type associated with the last live record.
    pub dcc_type: String,
    /// Last observed lifecycle status, such as `exited` or `host-died`.
    pub previous_status: String,
}

#[derive(Debug)]
struct InstanceSlice {
    records: Arc<[CapabilityRecord]>,
    fingerprint: InstanceFingerprint,
}

/// Lock-free domain state behind the gateway's concurrent capability index.
///
/// This type deliberately knows nothing about async refreshes, wall clocks,
/// HTTP clients, or synchronization. Callers provide exclusive access and
/// use the returned `changed` flags to maintain their own cache generation.
#[derive(Debug, Default)]
pub struct CapabilityIndexState {
    per_instance: BTreeMap<Uuid, InstanceSlice>,
    tombstones: VecDeque<InstanceTombstone>,
    unloaded: Arc<[CapabilityRecord]>,
}

impl CapabilityIndexState {
    /// Replace an instance slice and report `(previous_fingerprint, changed)`.
    pub fn upsert_instance(
        &mut self,
        instance_id: Uuid,
        records: Vec<CapabilityRecord>,
        fingerprint: InstanceFingerprint,
    ) -> (Option<InstanceFingerprint>, bool) {
        if records.is_empty() {
            let previous = self
                .per_instance
                .remove(&instance_id)
                .map(|slice| slice.fingerprint);
            return (previous, previous.is_some());
        }

        self.tombstones.retain(|row| row.instance_id != instance_id);
        if let Some(current) = self.per_instance.get(&instance_id)
            && current.fingerprint == fingerprint
            && current.records.as_ref() == records.as_slice()
        {
            return (Some(current.fingerprint), false);
        }

        let previous = self
            .per_instance
            .insert(
                instance_id,
                InstanceSlice {
                    records: Arc::from(records),
                    fingerprint,
                },
            )
            .map(|slice| slice.fingerprint);
        (previous, true)
    }

    /// Remove an instance slice.
    pub fn remove_instance(&mut self, instance_id: Uuid) -> bool {
        self.per_instance.remove(&instance_id).is_some()
    }

    /// Remove an instance while retaining bounded lifecycle provenance.
    ///
    /// Returns `(was_live, changed)`. A later status for an existing
    /// tombstone changes provenance even when the instance is already absent.
    pub fn remove_instance_with_status(
        &mut self,
        instance_id: Uuid,
        previous_status: &str,
    ) -> (bool, bool) {
        let removed = self.per_instance.remove(&instance_id);
        let dcc_type = removed
            .as_ref()
            .and_then(|slice| slice.records.first())
            .map(|record| record.dcc_type.clone())
            .or_else(|| {
                self.tombstones
                    .iter()
                    .find(|row| row.instance_id == instance_id)
                    .map(|row| row.dcc_type.clone())
            });
        let Some(dcc_type) = dcc_type else {
            return (removed.is_some(), false);
        };

        self.tombstones.retain(|row| row.instance_id != instance_id);
        if self.tombstones.len() == MAX_INSTANCE_TOMBSTONES {
            self.tombstones.pop_front();
        }
        self.tombstones.push_back(InstanceTombstone {
            instance_id,
            dcc_type,
            previous_status: previous_status.to_string(),
        });
        (removed.is_some(), true)
    }

    /// Resolve the newest unique lifecycle row by DCC and UUID/prefix.
    #[must_use]
    pub fn instance_tombstone(
        &self,
        dcc_type: &str,
        instance_hint: &str,
    ) -> Option<InstanceTombstone> {
        let exact = Uuid::parse_str(instance_hint).ok();
        let prefix = instance_hint.to_ascii_lowercase();
        let mut matches = self.tombstones.iter().rev().filter(|row| {
            row.dcc_type.eq_ignore_ascii_case(dcc_type)
                && exact.map_or_else(
                    || {
                        row.instance_id
                            .simple()
                            .to_string()
                            .starts_with(prefix.as_str())
                    },
                    |id| row.instance_id == id,
                )
        });
        let row = matches.next()?.clone();
        matches.next().is_none().then_some(row)
    }

    /// Return indexed instance ids in stable UUID order.
    #[must_use]
    pub fn instance_ids(&self) -> Vec<Uuid> {
        self.per_instance.keys().copied().collect()
    }

    /// Build an immutable, deterministically ordered snapshot.
    #[must_use]
    pub fn snapshot(&self) -> IndexSnapshot {
        let loaded_count: usize = self
            .per_instance
            .values()
            .map(|slice| slice.records.len())
            .sum();
        let mut records = Vec::with_capacity(loaded_count + self.unloaded.len());
        let mut fingerprints = HashMap::with_capacity(self.per_instance.len());
        for (instance_id, slice) in &self.per_instance {
            fingerprints.insert(*instance_id, slice.fingerprint);
            records.extend_from_slice(&slice.records);
        }
        records.extend_from_slice(&self.unloaded);
        records.sort_by(|a, b| a.tool_slug.cmp(&b.tool_slug));
        IndexSnapshot {
            records: Arc::from(records),
            fingerprints,
        }
    }

    /// Return the fingerprint stored for one instance.
    #[must_use]
    pub fn fingerprint_for(&self, instance_id: Uuid) -> Option<InstanceFingerprint> {
        self.per_instance
            .get(&instance_id)
            .map(|slice| slice.fingerprint)
    }

    /// Count live records across all instances.
    #[must_use]
    pub fn total_records(&self) -> usize {
        self.per_instance
            .values()
            .map(|slice| slice.records.len())
            .sum()
    }

    /// Count tracked live instances.
    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.per_instance.len()
    }

    /// Replace unloaded-skill records, returning whether content changed.
    pub fn set_unloaded_records(&mut self, mut records: Vec<CapabilityRecord>) -> bool {
        records.sort_by(|a, b| a.tool_slug.cmp(&b.tool_slug));
        if self.unloaded.as_ref() == records.as_slice() {
            return false;
        }
        self.unloaded = Arc::from(records);
        true
    }

    /// Count unloaded-skill records.
    #[must_use]
    pub fn unloaded_count(&self) -> usize {
        self.unloaded.len()
    }
}

impl IndexSnapshot {
    /// Convenience predicate for diagnostics.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Resolve a capability record by its slug in O(n). The index is
    /// bounded (every live backend × ~tens of actions) so the linear
    /// scan is the right default; a hash map would add per-refresh
    /// cost without a proven win until indices exceed ~10 k records.
    #[must_use]
    pub fn find_by_slug(&self, slug: &str) -> Option<&CapabilityRecord> {
        self.records.iter().find(|r| r.tool_slug == slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(slug: &str) -> CapabilityRecord {
        CapabilityRecord::new(
            slug.to_owned(),
            "stub".into(),
            "stub".into(),
            None,
            "",
            vec![],
            "maya".into(),
            Uuid::nil(),
            false,
            false,
            None,
        )
    }

    #[test]
    fn fingerprint_default_is_zero() {
        assert_eq!(InstanceFingerprint::default(), InstanceFingerprint(0));
    }

    #[test]
    fn fingerprint_is_value_equal() {
        // The whole point of the fingerprint is to short-circuit
        // rebuilds via `==`; pin the structural-equality contract.
        assert_eq!(InstanceFingerprint(42), InstanceFingerprint(42));
        assert_ne!(InstanceFingerprint(42), InstanceFingerprint(43));
    }

    #[test]
    fn compute_fingerprint_is_deterministic() {
        let rec = make_record("maya.abcdef01.create_sphere");
        let fp_a = compute_fingerprint(std::slice::from_ref(&rec));
        let fp_b = compute_fingerprint(&[rec]);
        assert_eq!(fp_a, fp_b);
    }

    #[test]
    fn compute_fingerprint_changes_on_tool_slug() {
        let a = make_record("maya.abcdef01.create_sphere");
        let mut b = a.clone();
        b.tool_slug = "maya.abcdef01.create_cube".into();
        assert_ne!(compute_fingerprint(&[a]), compute_fingerprint(&[b]));
    }

    #[test]
    fn compute_fingerprint_changes_on_loaded() {
        let mut a = make_record("maya.abcdef01.t");
        a.loaded = true;
        let mut b = a.clone();
        b.loaded = false;
        assert_ne!(compute_fingerprint(&[a]), compute_fingerprint(&[b]));
    }

    #[test]
    fn compute_fingerprint_changes_on_annotations() {
        use crate::capability::record::CapabilityAnnotations;
        let a = make_record("maya.abcdef01.t");
        let mut b = a.clone();
        b.annotations = Some(CapabilityAnnotations {
            title: Some("Test".into()),
            read_only_hint: Some(true),
            destructive_hint: None,
            idempotent_hint: None,
            open_world_hint: None,
        });
        assert_ne!(compute_fingerprint(&[a]), compute_fingerprint(&[b]));
    }

    #[test]
    fn compute_fingerprint_changes_on_metadata() {
        use crate::capability::record::CapabilityMetadata;
        let a = make_record("maya.abcdef01.t");
        let mut b = a.clone();
        b.metadata = Some(CapabilityMetadata {
            affinity: None,
            execution: None,
            timeout_hint_secs: None,
            job_strategy: None,
            enforce_thread_affinity: None,
            risk: Some("mutation".into()),
            tool_role: None,
        });
        assert_ne!(compute_fingerprint(&[a]), compute_fingerprint(&[b]));
    }

    #[test]
    fn compute_fingerprint_empty_list_is_stable() {
        let fp = compute_fingerprint(&[]);
        assert_eq!(fp, compute_fingerprint(&[]));
    }

    #[test]
    fn snapshot_default_is_empty() {
        let snap = IndexSnapshot::default();
        assert!(snap.is_empty());
        assert!(snap.fingerprints.is_empty());
        assert_eq!(snap.records.len(), 0);
    }

    #[test]
    fn snapshot_find_by_slug_returns_first_match() {
        let snap = IndexSnapshot {
            records: Arc::from(vec![
                make_record("maya.abcdef01.create_sphere"),
                make_record("maya.abcdef01.create_cube"),
            ]),
            fingerprints: HashMap::new(),
        };
        let hit = snap.find_by_slug("maya.abcdef01.create_cube");
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().tool_slug, "maya.abcdef01.create_cube");
        assert!(snap.find_by_slug("maya.abcdef01.missing").is_none());
    }

    #[test]
    fn snapshot_clone_shares_records_allocation() {
        let snap = IndexSnapshot {
            records: Arc::from(vec![make_record("x.abcdef01.a")]),
            fingerprints: HashMap::new(),
        };
        let snap2 = snap.clone();
        // Arc is the cheap-clone contract callers rely on; verify the
        // backing allocation is shared, not deep-copied.
        assert!(Arc::ptr_eq(&snap.records, &snap2.records));
    }

    #[test]
    fn snapshot_carries_per_instance_fingerprints() {
        let iid = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let mut fingerprints = HashMap::new();
        fingerprints.insert(iid, InstanceFingerprint(0xdead_beef));

        let snap = IndexSnapshot {
            records: Arc::from(Vec::<CapabilityRecord>::new()),
            fingerprints,
        };
        assert_eq!(
            snap.fingerprints.get(&iid),
            Some(&InstanceFingerprint(0xdead_beef))
        );
    }

    #[test]
    fn state_upsert_snapshot_and_remove_are_consistent() {
        let maya = Uuid::from_u128(1);
        let photoshop = Uuid::from_u128(2);
        let mut state = CapabilityIndexState::default();
        let mut maya_record = make_record("maya.00000000.create_sphere");
        maya_record.instance_id = maya;
        let mut photoshop_record = make_record("photoshop.00000000.open_document");
        photoshop_record.instance_id = photoshop;
        photoshop_record.dcc_type = "photoshop".into();

        assert_eq!(
            state.upsert_instance(maya, vec![maya_record], InstanceFingerprint(10)),
            (None, true)
        );
        assert_eq!(
            state.upsert_instance(photoshop, vec![photoshop_record], InstanceFingerprint(20)),
            (None, true)
        );
        assert_eq!(state.instance_ids(), [maya, photoshop]);
        assert_eq!(state.snapshot().records.len(), 2);
        assert!(state.remove_instance(maya));
        assert_eq!(state.fingerprint_for(maya), None);
        assert_eq!(state.total_records(), 1);
    }

    #[test]
    fn identical_upsert_and_unloaded_replacement_are_noops() {
        let iid = Uuid::from_u128(3);
        let record = make_record("custom.00000000.inspect");
        let mut state = CapabilityIndexState::default();
        assert!(
            state
                .upsert_instance(iid, vec![record.clone()], InstanceFingerprint(1))
                .1
        );
        assert!(
            !state
                .upsert_instance(iid, vec![record.clone()], InstanceFingerprint(1))
                .1
        );
        assert!(state.set_unloaded_records(vec![record.clone()]));
        assert!(!state.set_unloaded_records(vec![record]));
    }

    #[test]
    fn tombstones_track_latest_status_and_clear_on_return() {
        let iid = Uuid::from_u128(4);
        let mut record = make_record("zbrush.00000000.get_scene");
        record.instance_id = iid;
        record.dcc_type = "zbrush".into();
        let mut state = CapabilityIndexState::default();
        state.upsert_instance(iid, vec![record.clone()], InstanceFingerprint(1));

        assert_eq!(
            state.remove_instance_with_status(iid, "exited"),
            (true, true)
        );
        assert_eq!(
            state
                .instance_tombstone("zbrush", &iid.to_string())
                .map(|row| row.previous_status),
            Some("exited".into())
        );
        assert_eq!(
            state.remove_instance_with_status(iid, "host-died"),
            (false, true)
        );
        state.upsert_instance(iid, vec![record], InstanceFingerprint(2));
        assert!(
            state
                .instance_tombstone("zbrush", &iid.to_string())
                .is_none()
        );
    }
}
