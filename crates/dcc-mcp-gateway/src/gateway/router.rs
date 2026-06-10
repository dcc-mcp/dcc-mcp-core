//! Gateway axum router builder.

use axum::{Router, middleware, routing};
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use super::caller_attribution::caller_attribution_middleware;
use super::handlers::{
    handle_gateway_get, handle_gateway_mcp, handle_gateway_yield, handle_health, handle_instances,
    handle_proxy_dcc, handle_proxy_instance, handle_v1_call, handle_v1_call_batch,
    handle_v1_context, handle_v1_dcc_instance_call, handle_v1_dcc_instance_describe,
    handle_v1_dcc_instance_stop, handle_v1_describe, handle_v1_describe_path, handle_v1_docs,
    handle_v1_healthz, handle_v1_instances_deregister, handle_v1_instances_heartbeat,
    handle_v1_instances_register, handle_v1_list_skills, handle_v1_load_skill, handle_v1_openapi,
    handle_v1_readyz, handle_v1_search, handle_v1_skills, handle_v1_unload_skill,
    handle_v1_update_check, handle_v1_update_download,
};
use super::http_limits::rate_limit_middleware;
use super::resilience::gateway_limits;
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
/// - `POST /v1/instances/register` — register a remote DCC instance by MCP URL
/// - `POST /v1/instances/heartbeat` — refresh a remote registration TTL
/// - `POST /v1/instances/deregister` — remove a remote registration
/// - `GET  /v1/context` — aggregate snapshot plus `instances` (live rows, same shape as `/v1/instances`)
/// - `POST /v1/search`    — keyword + filter search over the capability index
/// - `POST /v1/describe`  — resolve one capability slug
/// - `POST /v1/call`      — invoke a backend action by slug
/// - `POST /v1/dcc/{dcc_type}/instances/{instance_id}/call` — same routing as `/v1/call`, but
///   `dcc_type` + `instance_id` (UUID or ≥4-char hex prefix) come from the path and the JSON
///   body carries `backend_tool` (+ optional `arguments` / `meta`) instead of a dotted `tool_slug`
/// - `GET /v1/dcc/{dcc_type}/instances/{instance_id}/describe?backend_tool=...` — same payload as
///   `GET /v1/tools/{slug}` after composing the dotted `tool_slug` (aliases `tool`, `action` query keys)
/// - `POST /v1/dcc/{dcc_type}/instances/{instance_id}/stop` — guarded safe-stop callback for
///   test-owned instances that advertise `safe_stop_url` metadata
/// - `POST /v1/call_batch` — ordered multi-invocation (same contract as MCP `call_tools`)
///
/// Admin UI (#772, `admin` feature):
/// - `GET  /admin`              — HTML dashboard
/// - `GET  /admin/api/*`        — JSON API endpoints
pub fn build_gateway_router(mut state: GatewayState) -> Router {
    state.debug_routes_enabled = false;
    let limits = gateway_limits();
    let mut r = build_base_router(state);
    r = r.layer(RequestBodyLimitLayer::new(limits.body_max_bytes));
    r = r.layer(middleware::from_fn(caller_attribution_middleware));
    if limits.rate_limit_per_minute_per_ip > 0 {
        r = r.layer(middleware::from_fn(rate_limit_middleware));
    }
    r.layer(TraceLayer::new_for_http()).layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    )
}

/// Build the gateway router, optionally attaching the admin UI sub-router.
///
/// When `admin_state` is `Some`, the admin routes are mounted at `admin_path`.
/// This is called from `start_gateway_tasks` when `admin_enabled = true` and
/// the `admin` feature is compiled in.
pub fn build_gateway_router_with_admin(
    state: GatewayState,
    #[cfg(feature = "admin")] admin_state: Option<super::admin::state::AdminState>,
    #[cfg(feature = "admin")] admin_path: &str,
) -> Router {
    #[cfg(feature = "admin")]
    let mut state = state;
    #[cfg(feature = "admin")]
    {
        state.debug_routes_enabled = admin_state.is_some();
    }
    let limits = gateway_limits();
    let mut router = build_base_router(state);
    router = router.layer(RequestBodyLimitLayer::new(limits.body_max_bytes));
    router = router.layer(middleware::from_fn(caller_attribution_middleware));
    if limits.rate_limit_per_minute_per_ip > 0 {
        router = router.layer(middleware::from_fn(rate_limit_middleware));
    }

    // ── #772 admin UI (opt-in feature + runtime flag) ─────────────────────
    #[cfg(feature = "admin")]
    let router = if let Some(admin_st) = admin_state {
        let debug_router = super::admin::build_v1_debug_router(admin_st.clone());
        let admin_router = super::admin::build_admin_router(admin_st);
        // nest adds the prefix; requests to e.g. `/admin/api/health` are
        // forwarded to the sub-router as `/api/health`.
        tracing::info!("Admin UI mounted at {admin_path}");
        router.nest(admin_path, admin_router).merge(debug_router)
    } else {
        router
    };

    router.layer(TraceLayer::new_for_http()).layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    )
}

fn build_base_router(state: GatewayState) -> Router {
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
        .route(
            "/v1/instances/register",
            routing::post(handle_v1_instances_register),
        )
        .route(
            "/v1/instances/heartbeat",
            routing::post(handle_v1_instances_heartbeat),
        )
        .route(
            "/v1/instances/deregister",
            routing::post(handle_v1_instances_deregister),
        )
        .route("/v1/healthz", routing::get(handle_v1_healthz))
        .route("/v1/readyz", routing::get(handle_v1_readyz))
        .route("/v1/openapi.json", routing::get(handle_v1_openapi))
        .route("/docs", routing::get(handle_v1_docs))
        .route("/v1/skills", routing::get(handle_v1_skills))
        .route("/v1/list_skills", routing::post(handle_v1_list_skills))
        .route("/v1/search", routing::post(handle_v1_search))
        .route("/v1/load_skill", routing::post(handle_v1_load_skill))
        .route("/v1/unload_skill", routing::post(handle_v1_unload_skill))
        .route("/v1/describe", routing::post(handle_v1_describe))
        .route("/v1/tools/{slug}", routing::get(handle_v1_describe_path))
        .route(
            "/v1/dcc/{dcc_type}/instances/{instance_id}/call",
            routing::post(handle_v1_dcc_instance_call),
        )
        .route(
            "/v1/dcc/{dcc_type}/instances/{instance_id}/describe",
            routing::get(handle_v1_dcc_instance_describe),
        )
        .route(
            "/v1/dcc/{dcc_type}/instances/{instance_id}/stop",
            routing::post(handle_v1_dcc_instance_stop),
        )
        .route("/v1/call", routing::post(handle_v1_call))
        .route("/v1/call_batch", routing::post(handle_v1_call_batch))
        .route("/v1/context", routing::get(handle_v1_context))
        // ── #1505 gateway-controlled binary updates ──────────────────────
        .route(
            "/v1/update/check",
            routing::get(handle_v1_update_check),
        )
        .route(
            "/v1/update/download/{binary_name}",
            routing::get(handle_v1_update_download),
        )
        .with_state(state)
}
