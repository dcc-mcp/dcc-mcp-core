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
//! The runtime UI asset is exported by `dcc-mcp-gateway-admin`. That crate owns
//! the trace domain, compact, link,
//! issue-report, statistics, analytics, governance, artifact, activity, task-outcome,
//! debug-bundle, postmortem, agent-trace packet, memory-summary, experiment, skill-path, and traffic projections,
//! Vite/npm build boundary, and embedded asset; this module owns the
//! gateway-specific data-source/HTTP adapters and API handlers.
//!
//! # Module layout (PIP-687 split)
//!
//! ```text
//! admin/
//! ├── domain/       # pure types, no I/O (trace types)
//! ├── application/  # orchestration/handler routing
//! ├── infra/        # SQLite reads, log reads, projection adapters, integration config
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

/// Infrastructure layer: SQLite, logs, projection adapters, integration config.
pub mod infra;

// ── Gateway adapters retained at the admin boundary ───────────────────────

#[cfg(feature = "admin")]
pub mod activity;
#[cfg(feature = "admin")]
// ── Remaining top-level modules ───────────────────────────────────────────
#[cfg(feature = "admin")]
#[cfg(all(test, feature = "admin"))]
mod logs_tests;
pub mod sqlite_lane;
pub mod state;
pub mod trace;

#[cfg(all(test, feature = "admin"))]
mod analytics_tests;
#[cfg(all(test, feature = "admin"))]
mod basic_endpoint_tests;
#[cfg(all(test, feature = "admin-persist-sqlite"))]
mod experiments_tests;
#[cfg(feature = "admin")]
#[cfg(all(test, feature = "admin"))]
mod instance_update_tests;
#[cfg(all(test, feature = "admin"))]
mod integration_tests;
#[cfg(all(test, feature = "admin-persist-sqlite"))]
mod recordings_tests;
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
#[cfg(feature = "admin")]
pub(crate) use application::experiments;
#[cfg(feature = "admin")]
pub use application::{
    analytics, artifacts, governance, marketplace, sessions, skill_reload, workers, workflows,
};
pub use dcc_mcp_db::{
    default_gateway_admin_sqlite_path as default_admin_db_path,
    resolve_gateway_admin_sqlite_path as resolve_admin_db_path,
};
pub use infra::stats;
pub use infra::{
    AdminAuditSink, GatewayStats, LatencyStats, StatsAggregator, StatsFilter, StatsRange,
    StatsStatus, TopEntry,
};
pub use sqlite_lane::{AdminSqliteLane, AdminSqliteReader, read_custom_skill_paths_for_startup};
pub use state::{AdminAuditRecord, AdminState, AuditLog, DurableAuditStore};
pub use trace::{DispatchTrace, TraceContext, TraceLog, TracePayload, TraceSpan};

#[cfg(feature = "admin")]
pub use application::workers::build_workers_payload;
#[cfg(feature = "admin")]
pub use application::workflows::{WorkflowDiscoverySummary, WorkflowStep, WorkflowView};

#[cfg(feature = "admin")]
pub use application::router::{build_admin_router, build_v1_debug_router};

#[cfg(all(test, feature = "admin"))]
mod marketplace_tests;
#[cfg(all(test, feature = "admin"))]
mod raw_proxy_lease_tests;
#[cfg(all(test, feature = "admin"))]
mod tests;
#[cfg(all(test, feature = "admin"))]
mod ui_control_dispatch_tests;
