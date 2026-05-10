//! Capability records and search wire types.
//!
//! # Module map
//!
//! | Submodule    | What lives here                                        |
//! |--------------|--------------------------------------------------------|
//! | [`record`]   | `CapabilityRecord`, slug encoding / parsing, validation|
//! | [`search`]   | `SearchQuery`, `SearchHit`, `SearchPage`, `SearchMode` |
//!
//! The search function that joins a `SearchQuery` against a live
//! capability index lives in `dcc-mcp-gateway` because it needs the
//! `IndexSnapshot` runtime type. Only the *wire types* — query
//! parameters, result rows, pagination envelope — live here so any
//! REST client talking to the gateway can deserialise responses
//! without pulling the full gateway crate.

pub mod record;
pub mod search;

// Re-export the record module's public surface from the capability
// facade so historical paths (`dcc_mcp_gateway_core::capability::
// CapabilityRecord` etc.) keep working verbatim.
pub use record::{CapabilityRecord, SCHEMA_AVAILABLE, is_valid_dcc_bucket, parse_slug, tool_slug};

pub use search::{DEFAULT_LIMIT, MAX_LIMIT, SearchHit, SearchMode, SearchPage, SearchQuery};
