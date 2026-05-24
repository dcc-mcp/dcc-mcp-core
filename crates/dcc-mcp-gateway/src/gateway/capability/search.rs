//! Capability search compatibility facade (issue #845).
//!
//! Search wire types and the pure ranking loop now live in
//! `dcc-mcp-gateway-core::capability::search` so domain ranking can be used
//! without depending on the gateway runtime crate.  This module re-exports the
//! same public surface from the historical `crate::gateway::capability::search`
//! path for source compatibility.

pub use dcc_mcp_gateway_core::capability::search::{
    DEFAULT_LIMIT, MAX_LIMIT, RANKER_VERSION, SearchHit, SearchMode, SearchPage, SearchQuery,
    search, search_page,
};
