//! Mounts the rmcp-backed MCP endpoint at `/mcp-next` (spike).
//!
//! This module creates a [`StreamableHttpService`] backed by our
//! [`DccMcpHandler`] and attaches it to the axum router as a nested service.
//! The existing `/mcp` endpoint is entirely unaffected.
//!
//! # Usage (called from `server/mod.rs` behind `#[cfg(feature = "rmcp-transport")]`)
//!
//! ```ignore
//! router = rmcp_mount::attach_rmcp_endpoint(router, app_state);
//! ```

use std::sync::Arc;

use axum::Router;
use dcc_mcp_http_server::rmcp_handler::{DccMcpHandler, RegistryContext};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tracing::info;

use crate::handler::AppState;

/// Attach the rmcp spike endpoint at `/mcp-next`.
///
/// The endpoint handles `initialize`, `tools/list`, `tools/call`, `resources/*`,
/// and `prompts/*` MCP methods via the [`DccMcpHandler`] adapter. Sessions are
/// managed by rmcp's [`LocalSessionManager`].
///
/// The router passed in is already state-erased (`Router<()>`) because
/// `.with_state()` was called earlier in the builder chain.
pub fn attach_rmcp_endpoint(router: Router, app_state: &AppState) -> Router {
    let server_state = app_state.server.clone();
    let resource_registry =
        Arc::new(app_state.resources.clone()) as Arc<dyn std::any::Any + Send + Sync>;
    let prompt_registry =
        Arc::new(app_state.prompts.clone()) as Arc<dyn std::any::Any + Send + Sync>;

    let registry_context = Arc::new(RegistryContext {
        resource_registry: Some(resource_registry),
        prompt_registry: Some(prompt_registry),
    });

    let session_manager = Arc::new(LocalSessionManager::default());

    let mut config = StreamableHttpServerConfig::default();
    config.stateful_mode = true;
    // Allow any host during spike (production should restrict this)
    config.allowed_hosts = vec![];

    let service = StreamableHttpService::new(
        move || {
            Ok(DccMcpHandler::new(
                server_state.clone(),
                registry_context.clone(),
            ))
        },
        session_manager,
        config,
    );

    info!("rmcp spike endpoint mounted at /mcp-next");

    router.nest_service("/mcp-next", service)
}
