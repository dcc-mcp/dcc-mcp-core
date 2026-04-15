//! Gateway axum router builder.

use axum::{Router, routing};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use super::handlers::{
    handle_gateway_mcp, handle_gateway_yield, handle_health, handle_instances, handle_proxy_dcc,
    handle_proxy_instance,
};
use super::state::GatewayState;

/// Build the gateway `Router` with all discovery and proxy routes.
///
/// Routes:
/// - `GET  /health`             — liveness probe
/// - `GET  /instances`          — list all live instances (REST)
/// - `POST /mcp`                — gateway MCP endpoint (meta-tools)
/// - `POST /mcp/{instance_id}`  — proxy to a specific instance
/// - `POST /mcp/dcc/{dcc_type}` — proxy to the best instance of a type
/// - `POST /gateway/yield`      — ask this gateway to yield to a newer version
pub fn build_gateway_router(state: GatewayState) -> Router {
    Router::new()
        .route("/health", routing::get(handle_health))
        .route("/instances", routing::get(handle_instances))
        .route("/mcp", routing::post(handle_gateway_mcp))
        .route("/mcp/{instance_id}", routing::post(handle_proxy_instance))
        .route("/mcp/dcc/{dcc_type}", routing::post(handle_proxy_dcc))
        .route("/gateway/yield", routing::post(handle_gateway_yield))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
}
