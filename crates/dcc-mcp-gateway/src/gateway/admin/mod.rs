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
//! # Module layout (PIP-687 split)
//!
//! ```text
//! admin/
//! ├── domain/       # pure types, no I/O (trace types)
//! ├── application/  # orchestration/handler routing
//! ├── infra/        # SQLite reads, log reads, activity projection, integration config
//! ├── html.rs       # standalone asset module (feature-gated)
//! └── mod.rs        # re-exports only
//! ```
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

// ── New PIP-687 split submodules ──────────────────────────────────────────

/// Domain layer: pure types, no I/O.
pub mod domain;

/// Application layer: handler routing and orchestration.
pub mod application;

/// Infrastructure layer: SQLite, logs, activity projection, integration config.
pub mod infra;

// ── Backward-compat shim modules (content moved to domain/application/infra) ──

#[cfg(feature = "admin")]
pub mod activity;
#[cfg(feature = "admin")]
pub(crate) mod integrations;

// ── Remaining top-level modules ───────────────────────────────────────────

#[cfg(feature = "admin")]
mod agent_trace;
#[cfg(feature = "admin")]
pub mod analytics;
#[cfg(feature = "admin")]
mod compact;
#[cfg(feature = "admin")]
mod debug_response;
#[cfg(feature = "admin")]
mod events;
#[cfg(feature = "admin")]
mod general;
#[cfg(feature = "admin")]
pub mod governance;
#[cfg(feature = "admin")]
mod html;
#[cfg(feature = "admin")]
mod issue_report;
#[cfg(feature = "admin")]
mod links;
#[cfg(all(test, feature = "admin"))]
mod logs_tests;
#[cfg(feature = "admin")]
pub mod marketplace;
#[cfg(feature = "admin")]
mod memory;
#[cfg(feature = "admin")]
mod skill_health;
#[cfg(feature = "admin")]
mod skill_paths;
#[cfg(feature = "admin")]
pub mod skill_reload;
pub mod sqlite_lane;
pub mod state;
pub mod stats;
pub mod trace;
mod trace_log;
#[cfg(feature = "admin")]
mod traffic;
#[cfg(feature = "admin")]
mod update;
#[cfg(feature = "admin")]
mod wecom_response;
#[cfg(feature = "admin")]
mod wecom_url;
#[cfg(feature = "admin")]
pub mod workers;
#[cfg(feature = "admin")]
pub mod workflows;

#[cfg(all(test, feature = "admin"))]
mod analytics_tests;
#[cfg(all(test, feature = "admin"))]
mod basic_endpoint_tests;
#[cfg(feature = "admin")]
mod handlers;
#[cfg(all(test, feature = "admin"))]
mod instance_update_tests;
#[cfg(all(test, feature = "admin"))]
mod integration_tests;
#[cfg(feature = "admin")]
mod router;
#[cfg(feature = "admin")]
mod sessions;
#[cfg(all(test, feature = "admin"))]
mod skill_paths_tests;
#[cfg(all(test, feature = "admin"))]
mod stats_traces_tests;
#[cfg(all(test, feature = "admin"))]
#[allow(clippy::await_holding_lock)]
// Intentional: parking_lot Mutex for env-var test serialization
mod workflows_tests;

// ── Backward-compatible re-exports ────────────────────────────────────────

// DB helpers.
pub use dcc_mcp_db::{
    default_gateway_admin_sqlite_path as default_admin_db_path,
    resolve_gateway_admin_sqlite_path as resolve_admin_db_path,
};
pub use sqlite_lane::{AdminSqliteLane, AdminSqliteReader, read_custom_skill_paths_for_startup};
pub use state::{AdminAuditRecord, AdminAuditSink, AdminState, AuditLog, DurableAuditStore};
pub use stats::{
    GatewayStats, LatencyStats, StatsAggregator, StatsFilter, StatsRange, StatsStatus, TopEntry,
};

pub use trace::{DispatchTrace, TraceContext, TraceLog, TracePayload, TraceSpan};

#[cfg(feature = "admin")]
pub use workers::build_workers_payload;
#[cfg(feature = "admin")]
pub use workflows::{WorkflowDiscoverySummary, WorkflowStep, WorkflowView};

#[cfg(feature = "admin")]
pub use router::{build_admin_router, build_v1_debug_router};

#[cfg(all(test, feature = "admin"))]
mod marketplace_tests;
#[cfg(all(test, feature = "admin"))]
mod raw_proxy_lease_tests;
#[cfg(all(test, feature = "admin"))]
mod tests;
