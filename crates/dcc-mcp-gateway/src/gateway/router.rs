//! Gateway axum router builder.

use axum::{Router, routing};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use super::handlers::{
    handle_gateway_get, handle_gateway_mcp, handle_gateway_yield, handle_health, handle_instances,
    handle_proxy_dcc, handle_proxy_instance, handle_v1_call, handle_v1_context, handle_v1_describe,
    handle_v1_describe_path, handle_v1_healthz, handle_v1_openapi, handle_v1_readyz,
    handle_v1_search, handle_v1_skills,
};
use super::state::GatewayState;

/// Build the gateway `Router` with all discovery, SSE, REST, and proxy routes.
///
/// Routes:
/// - `GET  /health`             — liveness probe
/// - `GET  /instances`          — list all live instances (legacy alias)
/// - `GET  /mcp`                — SSE stream for MCP push notifications (Streamable HTTP spec)
/// - `POST /mcp`                — gateway MCP endpoint (meta-tools + Resources API)
/// - `POST /mcp/{instance_id}`  — proxy to a specific instance
/// - `POST /mcp/dcc/{dcc_type}` — proxy to the best instance of a type
/// - `POST /gateway/yield`      — ask this gateway to yield to a newer version
///
/// Dynamic-capability REST API (#654, introduced by #657):
/// - `GET  /v1/instances` — same payload as `/instances`
/// - `POST /v1/search`    — keyword + filter search over the capability index
/// - `POST /v1/describe`  — resolve one capability slug
/// - `POST /v1/call`      — invoke a backend action by slug
pub fn build_gateway_router(state: GatewayState) -> Router {
    Router::new()
        .route("/health", routing::get(handle_health))
        .route("/instances", routing::get(handle_instances))
        // GET /mcp → SSE stream; POST /mcp → JSON-RPC handler
        .route(
            "/mcp",
            routing::get(handle_gateway_get).post(handle_gateway_mcp),
        )
        .route("/mcp/{instance_id}", routing::post(handle_proxy_instance))
        .route("/mcp/dcc/{dcc_type}", routing::post(handle_proxy_dcc))
        .route("/gateway/yield", routing::post(handle_gateway_yield))
        // ── #654 dynamic-capability REST API ─────────────────────────
        .route("/v1/instances", routing::get(handle_instances))
        .route("/v1/healthz", routing::get(handle_v1_healthz))
        .route("/v1/readyz", routing::get(handle_v1_readyz))
        .route("/v1/openapi.json", routing::get(handle_v1_openapi))
        .route("/v1/skills", routing::get(handle_v1_skills))
        .route("/v1/search", routing::post(handle_v1_search))
        .route("/v1/describe", routing::post(handle_v1_describe))
        .route("/v1/tools/{slug}", routing::get(handle_v1_describe_path))
        .route("/v1/call", routing::post(handle_v1_call))
        .route("/v1/context", routing::get(handle_v1_context))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
}
