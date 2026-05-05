//! Admin UI axum router — registered only when the `admin` feature is enabled.

use axum::{Router, routing};

use super::handlers::{
    handle_admin_calls, handle_admin_health, handle_admin_instances, handle_admin_logs,
    handle_admin_tools, handle_admin_ui,
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
/// - `GET  /api/calls`     → JSON recent calls
/// - `GET  /api/logs`      → JSON event log
/// - `GET  /api/health`    → JSON health summary
pub fn build_admin_router(state: AdminState) -> Router {
    Router::new()
        .route("/", routing::get(handle_admin_ui))
        .route("/api/instances", routing::get(handle_admin_instances))
        .route("/api/tools", routing::get(handle_admin_tools))
        .route("/api/calls", routing::get(handle_admin_calls))
        .route("/api/logs", routing::get(handle_admin_logs))
        .route("/api/health", routing::get(handle_admin_health))
        .with_state(state)
}
