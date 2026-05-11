//! Resource URI encoding/decoding compatibility facade (#732, #845).
//!
//! The pure gateway resource URI prefix contract now lives in
//! `dcc-mcp-gateway-core::resource_uri` so consumers can encode/decode gateway
//! resource URIs without depending on the gateway runtime crate. This module
//! re-exports the historical `crate::gateway::namespace::resource_uri` path.

pub use dcc_mcp_gateway_core::resource_uri::{decode_resource_uri, encode_resource_uri};
