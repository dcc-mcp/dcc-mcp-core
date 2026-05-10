//! Error types for the MCP HTTP server.
//!
//! # Relocation notice (issue #852)
//!
//! [`HttpError`] / [`HttpResult`] were migrated to the dedicated
//! [`dcc_mcp_http_types::error`] module as part of the `dcc-mcp-http`
//! Clean-Architecture split. This module now re-exports them so existing
//! call sites (`crate::error::HttpError`, `crate::error::HttpResult`)
//! keep compiling under their historical paths; the canonical
//! definitions live in `dcc-mcp-http-types`.
//!
//! New code should depend on `dcc-mcp-http-types` directly when it only
//! needs the error surface.

pub use dcc_mcp_http_types::error::{HttpError, HttpResult};
