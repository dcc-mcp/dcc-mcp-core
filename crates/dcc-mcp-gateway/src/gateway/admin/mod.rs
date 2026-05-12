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
//!
//! # Endpoints
//!
//! | Path | Source data | Phase |
//! |------|-------------|-------|
//! | `GET /admin/api/health`            | `GatewayState` | base |
//! | `GET /admin/api/instances`         | `GatewayState` registry | base |
//! | `GET /admin/api/tools`             | `CapabilityIndex` snapshot | base |
//! | `GET /admin/api/calls`             | [`AuditLog`] ring buffer | Phase 1 |
//! | `GET /admin/api/traces`            | [`TraceLog`] ring buffer | Phase 2 |
//! | `GET /admin/api/traces/{id}`       | [`TraceLog`] ring buffer | Phase 2 |
//! | `GET /admin/api/stats`             | [`StatsAggregator`] | Phase 3 |
//! | `GET /admin/api/workers`           | `GatewayState` registry | Phase 4 |
//! | `GET /admin/api/logs`              | [`EventLog`] | base |
//!
//! See `docs/guide/gateway-admin.md` for screenshots and configuration knobs.

mod html;
pub mod state;
pub mod stats;
pub mod trace;
pub mod workers;

#[cfg(feature = "admin")]
mod handlers;
#[cfg(feature = "admin")]
mod router;

pub use state::{
    AdminAuditRecord, AdminAuditSink, AdminState, AuditLog, DurableAuditStore, EventLog,
};
pub use stats::{GatewayStats, LatencyStats, StatsAggregator, StatsRange, TopEntry};
pub use trace::{DispatchTrace, TraceLog, TracePayload, TraceSpan};
pub use workers::build_workers_payload;

#[cfg(feature = "admin")]
pub use router::build_admin_router;

#[cfg(test)]
mod tests;
