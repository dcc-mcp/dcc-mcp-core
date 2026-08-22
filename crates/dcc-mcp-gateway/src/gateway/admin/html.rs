//! Standalone asset module — embedded admin UI frontend.
//!
//! Asset generation and embedding are owned by `dcc-mcp-gateway-admin`, keeping
//! the Node/Vite build boundary outside the gateway application crate.
//! When `feature = "admin"` is off, the binary only carries a tiny fallback page.
//!
//! The asset is served by `general::handle_admin_ui` through `ADMIN_HTML`.

/// The Vite-built React admin dashboard HTML page.
#[cfg(feature = "admin")]
pub use dcc_mcp_gateway_admin::ADMIN_HTML;

/// Minimal fallback used when the gateway is compiled without embedded admin assets.
#[cfg(not(feature = "admin"))]
pub const ADMIN_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>DCC-MCP Gateway Admin</title></head><body><h1>DCC-MCP Gateway Admin</h1><p>The embedded admin UI is not available in this build.</p></body></html>"#;
