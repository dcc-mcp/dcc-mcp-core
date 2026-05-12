//! Built admin UI assets embedded in the gateway binary.

/// The Vite-built React admin dashboard HTML page.
#[cfg(feature = "admin")]
pub const ADMIN_HTML: &str = include_str!("generated/index.html");
