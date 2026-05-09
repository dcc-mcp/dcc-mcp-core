//! Zero-build read-only admin web UI for dcc-mcp-gateway.
//!
//! Enabled via the `admin` Cargo feature.  When the feature is off, this module
//! exposes only the types needed for the gateway to compile without the UI.
//!
//! # Activation
//!
//! ```toml
//! # Cargo.toml
//! dcc-mcp-gateway = { features = ["admin"] }
//! ```
//!
//! ```rust,ignore
//! // GatewayConfig
//! GatewayConfig {
//!     admin_enabled: true,
//!     admin_path: "/admin".into(),
//!     ..Default::default()
//! }
//! ```
//!
//! Then open `http://localhost:9765/admin`.
//!
//! # Architecture
//!
//! The entire UI is a single inline HTML string (`admin/html.rs`) bundled into
//! the binary via a Rust `const`.  No `npm`, no CDN, no `build.rs`.
//! Vanilla JS polls the JSON API endpoints every 5 seconds.

mod html;
pub mod state;
pub mod trace;

#[cfg(feature = "admin")]
mod handlers;
#[cfg(feature = "admin")]
mod router;

pub use state::{AdminAuditRecord, AdminAuditSink, AdminState, AuditLog, EventLog};
pub use trace::{DispatchTrace, TraceLog, TracePayload, TraceSpan};

#[cfg(feature = "admin")]
pub use router::build_admin_router;

#[cfg(test)]
mod tests;
