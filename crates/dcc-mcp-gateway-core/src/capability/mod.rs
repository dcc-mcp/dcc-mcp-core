//! Capability records and search/index wire types.
//!
//! # Module map
//!
//! | Submodule    | What lives here                                              |
//! |--------------|--------------------------------------------------------------|
//! | [`record`]   | `CapabilityRecord`, slug encoding / parsing, validation      |
//! | [`search`]   | `SearchQuery`, `SearchHit`, `SearchPage`, `SearchMode`       |
//! | [`index`]    | `IndexSnapshot`, `InstanceFingerprint` — read-side snapshot  |
//!
//! The mutable `CapabilityIndex` (which owns a `parking_lot::RwLock`
//! and a `BTreeMap` of per-instance state) and the search ranking
//! function (which depends on the pluggable `Scorer` trait) both live
//! in `dcc-mcp-gateway` because they carry runtime state the domain
//! layer has no business holding. Only the *wire types* — query
//! parameters, result rows, pagination envelope, snapshot view — live
//! here so any REST client talking to the gateway can deserialise
//! responses without pulling the full gateway crate.

pub mod index;
pub mod record;
pub mod search;

// Re-export each submodule's public surface from the capability
// facade so historical paths (`dcc_mcp_gateway_core::capability::
// CapabilityRecord` etc.) keep working verbatim.
pub use index::{IndexSnapshot, InstanceFingerprint};
pub use record::{CapabilityRecord, SCHEMA_AVAILABLE, is_valid_dcc_bucket, parse_slug, tool_slug};
pub use search::{DEFAULT_LIMIT, MAX_LIMIT, SearchHit, SearchMode, SearchPage, SearchQuery};
