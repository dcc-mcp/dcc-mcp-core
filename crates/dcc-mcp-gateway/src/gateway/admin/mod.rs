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
//! | `GET /admin/api/logs`              | [`GatewayState::event_log`] | base |
//!
//! See `docs/guide/gateway-admin.md` for screenshots and configuration knobs.

pub mod activity;
mod agent_trace;
#[cfg(feature = "admin")]
pub mod analytics;
mod compact;
mod debug_response;
pub mod governance;
mod html;
#[cfg(feature = "admin")]
mod issue_report;
mod links;
mod skill_health;
pub mod sqlite_lane;
pub mod state;
pub mod stats;
pub mod trace;
mod trace_log;
mod traffic;
pub mod workers;
pub mod workflows;

#[cfg(feature = "admin")]
mod handlers;
#[cfg(feature = "admin")]
mod router;

pub use activity::{ActivityCorrelation, ActivityEvent, TaskSnapshot};
pub use dcc_mcp_db::{
    default_gateway_admin_sqlite_path as default_admin_db_path,
    resolve_gateway_admin_sqlite_path as resolve_admin_db_path,
};
pub use sqlite_lane::{AdminSqliteLane, AdminSqliteReader, read_custom_skill_paths_for_startup};
pub use state::{AdminAuditRecord, AdminAuditSink, AdminState, AuditLog, DurableAuditStore};
pub use stats::{GatewayStats, LatencyStats, StatsAggregator, StatsRange, TopEntry};
pub use trace::{DispatchTrace, TraceContext, TraceLog, TracePayload, TraceSpan};
pub use workers::build_workers_payload;
pub use workflows::{WorkflowDiscoverySummary, WorkflowStep, WorkflowView};

#[cfg(feature = "admin")]
pub use router::{build_admin_router, build_v1_debug_router};

#[cfg(test)]
mod tests;
