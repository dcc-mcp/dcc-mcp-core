//! Embedded frontend boundary for the DCC-MCP gateway admin dashboard.
//!
//! This crate deliberately owns the Vite/npm build script and the generated
//! dashboard payload. The gateway application depends on this crate only when
//! its `admin` feature is enabled, so non-admin gateway builds never execute a
//! Node.js toolchain.

#![forbid(unsafe_code)]

/// The Vite-built React admin dashboard HTML page.
#[cfg(feature = "embed")]
pub const ADMIN_HTML: &str = include_str!("generated/index.html");

/// Minimal fallback for direct builds that do not request embedded assets.
#[cfg(not(feature = "embed"))]
pub const ADMIN_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>DCC-MCP Gateway Admin</title></head><body><h1>DCC-MCP Gateway Admin</h1><p>The embedded admin UI is not available in this build.</p></body></html>"#;

#[cfg(test)]
mod tests {
    use super::ADMIN_HTML;

    #[test]
    fn admin_html_is_a_complete_document() {
        assert!(ADMIN_HTML.starts_with("<!doctype html>"));
        assert!(ADMIN_HTML.contains("DCC-MCP"));
        assert!(ADMIN_HTML.trim_end().ends_with("</html>"));
    }
}
