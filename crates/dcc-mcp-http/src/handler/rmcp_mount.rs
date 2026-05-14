//! Mounts the rmcp-backed MCP endpoint at `/mcp-next` (spike).
//!
//! This module creates a [`StreamableHttpService`] backed by our
//! [`DccMcpHandler`] and attaches it to the axum router as a nested service.
//! The existing `/mcp` endpoint is entirely unaffected.
//!
//! # Usage (called from `server/mod.rs` behind `#[cfg(feature = "rmcp-transport")]`)
//!
//! ```ignore
//! router = rmcp_mount::attach_rmcp_endpoint(router, &server_state);
//! ```

use std::sync::Arc;

use axum::Router;
use dcc_mcp_http_server::rmcp_handler::DccMcpHandler;
use dcc_mcp_http_server::server_state::ServerState;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tracing::info;

/// Attach the rmcp spike endpoint at `/mcp-next`.
///
/// The endpoint handles `initialize`, `tools/list`, `tools/call` and all other
/// MCP methods via the [`DccMcpHandler`] adapter. Sessions are managed by
/// rmcp's [`LocalSessionManager`].
///
/// The router passed in is already state-erased (`Router<()>`) because
/// `.with_state()` was called earlier in the builder chain.
pub fn attach_rmcp_endpoint(router: Router, server_state: &ServerState) -> Router {
    let state = server_state.clone();

    let session_manager = Arc::new(LocalSessionManager::default());

    let mut config = StreamableHttpServerConfig::default();
    config.stateful_mode = true;
    // Allow any host during spike (production should restrict this)
    config.allowed_hosts = vec![];

    let service = StreamableHttpService::new(
        move || Ok(DccMcpHandler::new(state.clone())),
        session_manager,
        config,
    );

    info!("rmcp spike endpoint mounted at /mcp-next");

    router.nest_service("/mcp-next", service)
}
