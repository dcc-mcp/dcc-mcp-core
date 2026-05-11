use dcc_mcp_http_types::config::{McpHttpConfig, ServerSpawnMode};

/// Build the Rust config backing `PyMcpHttpConfig.__new__`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_config(
    port: u16,
    server_name: Option<String>,
    server_version: Option<String>,
    enable_cors: bool,
    request_timeout_ms: u64,
    backend_timeout_ms: u64,
    enable_prometheus: bool,
    prometheus_basic_auth: Option<(String, String)>,
    gateway_async_dispatch_timeout_ms: u64,
    gateway_wait_terminal_timeout_ms: u64,
    gateway_route_ttl_secs: u64,
    gateway_max_routes_per_session: u64,
    shutdown_on_drop: bool,
) -> McpHttpConfig {
    let mut cfg = McpHttpConfig::default();
    cfg.server.port = port;
    if let Some(name) = server_name {
        cfg.server.server_name = name;
    }
    if let Some(ver) = server_version {
        cfg.server.server_version = ver;
    }
    cfg.server.enable_cors = enable_cors;
    cfg.server.request_timeout_ms = request_timeout_ms;
    cfg.gateway.gateway_port = 9765;
    cfg.gateway.backend_timeout_ms = backend_timeout_ms;
    cfg.telemetry.enable_prometheus = enable_prometheus;
    cfg.telemetry.prometheus_basic_auth = prometheus_basic_auth;
    cfg.gateway.gateway_async_dispatch_timeout_ms = gateway_async_dispatch_timeout_ms;
    cfg.gateway.gateway_wait_terminal_timeout_ms = gateway_wait_terminal_timeout_ms;
    cfg.gateway.gateway_route_ttl_secs = gateway_route_ttl_secs;
    cfg.gateway.gateway_max_routes_per_session = gateway_max_routes_per_session;
    cfg.features.shutdown_on_drop = shutdown_on_drop;
    // Issue #303: PyO3-embedded hosts (Maya on Windows etc.) cannot rely on
    // shared Tokio workers after `block_on` returns. Default to `Dedicated`.
    cfg.server.spawn_mode = ServerSpawnMode::Dedicated;
    cfg
}
