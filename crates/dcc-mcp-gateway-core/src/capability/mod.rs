//! Capability records, search, and refresh wire types.
//!
//! # Module map
//!
//! | Submodule          | What lives here                                              |
//! |--------------------|--------------------------------------------------------------|
//! | [`record`]         | `CapabilityRecord`, slug encoding / parsing, validation      |
//! | [`search`]         | `SearchQuery`, `SearchHit`, `SearchPage`, `SearchMode`, pure ranking |
//! | [`search_ranking`] | `Scorer` / `StrategyScorer` traits + built-in implementations |
//! | [`index`]          | `IndexSnapshot`, `InstanceFingerprint` — read-side snapshot  |
//! | [`builder`]        | `BuildOutcome` — output of the per-instance record builder   |
//! | [`refresh`]        | `RefreshReason` — why a refresh cycle is running             |
//!
//! The mutable `CapabilityIndex` (which owns a `parking_lot::RwLock`
//! and a `BTreeMap` of per-instance state), the `build_records_from_backend`
//! builder (which borrows backend `&[McpTool]`), and the
//! `refresh_instance` lifecycle driver (which owns a `reqwest::Client`)
//! all live in `dcc-mcp-gateway` because they carry runtime state the
//! domain layer has no business holding. The *wire types* — query
//! parameters, result rows, snapshot view, builder output, and
//! refresh-reason classification — plus pure search ranking live here
//! so any REST/admin client talking to the gateway can deserialise and
//! rank responses without pulling the full gateway crate.
//!
//! The ranking strategies in [`search_ranking`] also live here even
//! though they are behaviour rather than wire types: they are pure
//! CPU code (no async, no IO, no runtime state) and are part of the
//! search contract — the same query against the same snapshot must
//! produce the same ordering for every consumer of the domain, not
//! just the gateway binary.

pub mod builder;
pub mod index;
pub mod record;
pub mod refresh;
pub mod search;
pub mod search_ranking;

// Re-export each submodule's public surface from the capability
// facade so historical paths (`dcc_mcp_gateway_core::capability::
// CapabilityRecord` etc.) keep working verbatim.
pub use builder::BuildOutcome;
pub use index::{IndexSnapshot, InstanceFingerprint};
pub use record::{CapabilityRecord, SCHEMA_AVAILABLE, is_valid_dcc_bucket, parse_slug, tool_slug};
pub use refresh::RefreshReason;
pub use search::{
    DEFAULT_LIMIT, MAX_LIMIT, SearchHit, SearchMode, SearchPage, SearchQuery, search, search_page,
};
pub use search_ranking::{
    ExactScorer, FuzzyScorer, Scorer, ScorerFactory, StrategyExactScorer, StrategyFuzzyScorer,
    StrategyScorer, SubstringScorer,
};
