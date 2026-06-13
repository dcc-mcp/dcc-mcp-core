//! Built admin UI assets embedded in the gateway binary.

/// The Vite-built React admin dashboard HTML page.
#[cfg(feature = "admin")]
pub const ADMIN_HTML: &str = include_str!("generated/index.html");

/// Minimal fallback used when the gateway is compiled without embedded admin assets.
#[cfg(not(feature = "admin"))]
pub const ADMIN_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>DCC-MCP Gateway Admin</title></head><body><h1>DCC-MCP Gateway Admin</h1><p>The embedded admin UI is not available in this build.</p></body></html>"#;
