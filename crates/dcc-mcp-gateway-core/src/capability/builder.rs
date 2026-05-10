//! Capability builder result types (issue #845).
//!
//! The actual `build_records_from_backend` builder lives in
//! `dcc-mcp-gateway` because it borrows the backend `tools/list`
//! response (an `&[McpTool]` from `dcc-mcp-jsonrpc`). Only the
//! *output* of the builder lives here, since [`BuildOutcome`] is a
//! pure value type that diagnostics and tests want to inspect
//! without spinning up the full gateway crate.

use super::index::InstanceFingerprint;
use super::record::CapabilityRecord;

/// Output of the capability builder.
///
/// Returned by `dcc_mcp_gateway::capability::builder::build_records_from_backend`
/// after it has filtered, slug-encoded, and fingerprinted one
/// instance's worth of `tools/list`.
///
/// The struct is intentionally inert — every field is publicly
/// readable so the index swap path can move records, fingerprint,
/// and skip-count out without an extra accessor surface.
#[derive(Debug, Clone, Default)]
pub struct BuildOutcome {
    /// Records ready to be stored in the index. Sorted by
    /// `tool_slug` so the merge inside the gateway's
    /// `CapabilityIndex::snapshot` stays cheap.
    pub records: Vec<CapabilityRecord>,
    /// Stable fingerprint of the input tool list; feed this straight
    /// into `CapabilityIndex::upsert_instance`.
    pub fingerprint: InstanceFingerprint,
    /// Number of input tools rejected (e.g. missing name, skill stub
    /// filtered out). Diagnostics-only.
    pub skipped: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_outcome_default_is_empty() {
        let o = BuildOutcome::default();
        assert!(o.records.is_empty());
        assert_eq!(o.fingerprint, InstanceFingerprint::default());
        assert_eq!(o.skipped, 0);
    }

    #[test]
    fn build_outcome_carries_diagnostics() {
        // `skipped` is the only counter that tells operators the
        // builder rejected input rows — pin the field-shape contract
        // so a future refactor cannot silently drop it.
        let o = BuildOutcome {
            records: vec![],
            fingerprint: InstanceFingerprint(0xc0ffee),
            skipped: 3,
        };
        assert_eq!(o.fingerprint, InstanceFingerprint(0xc0ffee));
        assert_eq!(o.skipped, 3);
    }
}
