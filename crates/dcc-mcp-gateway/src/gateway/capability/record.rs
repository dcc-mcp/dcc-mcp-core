//! Capability record — the unit of the gateway index.
//!
//! # Relocation notice (issue #845)
//!
//! All types and helpers in this module were migrated to the dedicated
//! [`dcc_mcp_gateway_core::capability`] module as part of the
//! `dcc-mcp-gateway` Clean-Architecture split (issue #845). This module
//! now re-exports them so existing call sites
//! (`crate::gateway::capability::record::*`,
//! `crate::gateway::capability::CapabilityRecord`, …) keep compiling
//! under their historical paths; the domain-level definitions live in
//! `dcc-mcp-gateway-core`.
//!
//! New code should depend on `dcc-mcp-gateway-core` directly when it
//! only needs the wire types (`POST /v1/search` deserialisation, etc.).
//!
//! See `dcc_mcp_gateway_core::capability` for the original documentation.

pub use dcc_mcp_gateway_core::capability::{
    CapabilityAnnotations, CapabilityMetadata, CapabilityRecord, SCHEMA_AVAILABLE,
    is_valid_dcc_bucket, parse_slug, tool_slug,
};
