//! Admin UI axum router — registered only when the `admin` feature is enabled.

use axum::{Router, routing};

use super::handlers::{
    handle_admin_calls, handle_admin_health, handle_admin_instances, handle_admin_logs,
    handle_admin_stats, handle_admin_tools, handle_admin_trace_detail, handle_admin_traces,
    handle_admin_ui,
};
use super::state::AdminState;

/// Build the admin sub-router.
///
/// Mount this under `admin_path` (default `"/admin"`) on the main gateway
/// router when `admin_enabled = true`.
///
/// Routes provided:
/// - `GET  /`              → HTML dashboard
/// - `GET  /api/instances` → JSON instance list
/// - `GET  /api/tools`     → JSON tool list
/// - `GET  /api/calls`              → JSON recent calls
/// - `GET  /api/traces`             → JSON recent dispatch traces (Phase 2)
/// - `GET  /api/traces/{request_id}` → full trace waterfall for one call
/// - `GET  /api/stats?range=1h|24h|7d` → aggregated call statistics (Phase 3)
/// - `GET  /api/logs`               → JSON event log
/// - `GET  /api/health`             → JSON health summary
pub fn build_admin_router(state: AdminState) -> Router {
    Router::new()
        .route("/", routing::get(handle_admin_ui))
        .route("/api/instances", routing::get(handle_admin_instances))
        .route("/api/tools", routing::get(handle_admin_tools))
        .route("/api/calls", routing::get(handle_admin_calls))
        .route("/api/traces", routing::get(handle_admin_traces))
        .route(
            "/api/traces/{request_id}",
            routing::get(handle_admin_trace_detail),
        )
        .route("/api/logs", routing::get(handle_admin_logs))
        .route("/api/stats", routing::get(handle_admin_stats))
        .route("/api/health", routing::get(handle_admin_health))
        .with_state(state)
}
