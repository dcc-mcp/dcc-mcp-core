//! Auth configuration compatibility facade for OpenAPI-to-MCP forwarding.
//!
//! `AuthKind` and `AuthConfig` live in `dcc-mcp-gateway-core::openapi`
//! (issue #845). The gateway runtime re-exports them here so existing
//! `dcc_mcp_gateway::gateway::openapi::auth::*` call sites keep compiling.

pub use dcc_mcp_gateway_core::openapi::{AuthConfig, AuthKind};
