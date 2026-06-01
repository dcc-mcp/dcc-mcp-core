//! Pure value types describing the gateway capability index snapshot.
//!
//! These are the *read-side* shapes a REST or MCP handler sees when it
//! takes a snapshot of the capability index. The mutable
//! `CapabilityIndex` itself — which owns a `parking_lot::RwLock` and a
//! `BTreeMap` of per-instance state — stays in `dcc-mcp-gateway`
//! because it carries runtime state the domain layer has no business
//! holding (issue #845).
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

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use super::record::CapabilityRecord;

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
}
