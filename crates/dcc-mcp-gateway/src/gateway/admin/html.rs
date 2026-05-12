//! Inline HTML dashboard for the admin UI.

/// The admin dashboard HTML page (inline CSS + vanilla JS, no external deps).
#[cfg(feature = "admin")]
pub const ADMIN_HTML: &str = include_str!("index.html");
