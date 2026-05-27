//! Source-compatibility facade for the bare-name resolver.
//!
//! The implementation lives in [`dcc_mcp_gateway_core::naming::bare`]
//! (and its sibling `observability` module). This module preserves the
//! historical `crate::gateway::namespace::{BareNameInput,
//! resolve_bare_names, warn_skill_qualified_once}` import paths so
//! adapter code does not have to change.

pub use dcc_mcp_gateway_core::naming::{
    BareNameInput, resolve_bare_names, warn_skill_qualified_once,
};
