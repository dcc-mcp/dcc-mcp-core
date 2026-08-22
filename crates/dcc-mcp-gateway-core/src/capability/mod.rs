//! Capability records, search, and refresh wire types.
//!
//! # Module map
//!
//! | Submodule          | What lives here                                              |
//! |--------------------|--------------------------------------------------------------|
//! | [`record`]         | `CapabilityRecord`, slug encoding / parsing, validation      |
//! | [`search`]         | `SearchQuery`, `SearchHit`, pagination — delegates to `dcc-mcp-gateway-search` |
//! | [`search_ranking`] | Re-exports scorers / [`SearchRecord`] from `dcc-mcp-gateway-search` |
//! | [`index`]          | lock-free index state, snapshots, fingerprints, tombstones  |
//! | [`builder`]        | Pure per-instance MCP tool-to-capability record builder      |
//! | [`refresh`]        | Pure refresh-slice assembly, no-op policy, and reason types  |
//!
//! The concurrent `CapabilityIndex` wrapper (which owns synchronization
//! and refresh gates) and the `refresh_instance` lifecycle driver (which
//! owns a `reqwest::Client`) live in `dcc-mcp-gateway` because they carry
//! runtime state the domain layer has no business holding. The pure
//! builder and *wire types* — query parameters, result rows, snapshot
//! view, builder output, and refresh-reason classification — live here
//! so any REST/admin client
//! talking to the gateway can deserialise domain payloads without pulling
//! the full gateway crate.
//!
//! The ranking strategies ship in the standalone [`dcc_mcp_gateway_search`]
//! crate so future search backends (BM25, embeddings, …) can evolve without
//! pulling the full gateway-core surface.

pub mod builder;
pub mod index;
pub mod record;
pub mod refresh;
pub mod search;
pub mod search_ranking;

// Re-export each submodule's public surface from the capability
// facade so historical paths (`dcc_mcp_gateway_core::capability::
// CapabilityRecord` etc.) keep working verbatim.
pub use builder::{
    BuildInput, BuildOutcome, backend_job_status_tool, build_records_from_backend,
    is_backend_job_tool,
};
pub use index::{
    CapabilityIndexState, IndexSnapshot, InstanceFingerprint, InstanceTombstone,
    compute_fingerprint,
};
pub use record::{
    CapabilityAnnotations, CapabilityGroupInfo, CapabilityMetadata, CapabilityRecord,
    SCHEMA_AVAILABLE, is_valid_dcc_bucket, parse_slug, tool_slug,
};
pub use refresh::{
    RefreshBuildOutcome, RefreshReason, UnloadedCapabilityHint, build_refresh_records,
};
pub use search::{
    DEFAULT_LIMIT, MAX_LIMIT, RANKER_VERSION, SearchHit, SearchMode, SearchPage, SearchQuery,
    search, search_page,
};
pub use search_ranking::{
    ExactScorer, FuzzyScorer, Scorer, ScorerFactory, SearchRecord, StrategyExactScorer,
    StrategyFuzzyScorer, StrategyScorer, SubstringScorer,
};
