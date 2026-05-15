//! Mounts the rmcp-backed MCP endpoint.
//!
//! This module creates a [`StreamableHttpService`] backed by our
//! [`DccMcpHandler`] and attaches it to the axum router as a nested service.
//!
//! # Usage (called from `server/mod.rs` behind `#[cfg(feature = "rmcp-transport")]`)
//!
//! ```ignore
//! router = rmcp_mount::attach_rmcp_endpoint(router, app_state);
//! ```

use std::sync::Arc;

use axum::Router;
use dcc_mcp_http_server::rmcp_handler::{DccMcpHandler, RegistryContext};
use dcc_mcp_jsonrpc::NotificationBuilder;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tracing::info;

use super::rmcp_providers_impl::{PromptRegistryProvider, ResourceRegistryProvider};
use crate::handler::AppState;

/// Attach the rmcp endpoint at `/mcp`.
///
/// The endpoint handles all MCP methods (`initialize`, `tools/list`,
/// `tools/call`, `resources/*`, `prompts/*`, `logging/setLevel`) via the
/// [`DccMcpHandler`] adapter. Sessions are managed by rmcp's
/// [`LocalSessionManager`].
///
/// The router passed in is already state-erased (`Router<()>`) because
/// `.with_state()` was called earlier in the builder chain.
pub fn attach_rmcp_endpoint(router: Router, app_state: &AppState) -> Router {
    let server_state = app_state.server.clone();

    // Build provider trait objects that bridge registries into the handler.
    let resource_provider: Option<Arc<dyn dcc_mcp_http_server::rmcp_providers::ResourceProvider>> =
        if app_state.server.enable_resources {
            Some(Arc::new(ResourceRegistryProvider {
                registry: app_state.resources.clone(),
            }))
        } else {
            None
        };

    let prompt_provider: Option<Arc<dyn dcc_mcp_http_server::rmcp_providers::PromptProvider>> =
        if app_state.server.enable_prompts {
            Some(Arc::new(PromptRegistryProvider {
                registry: app_state.prompts.clone(),
            }))
        } else {
            None
        };

    let prompts = app_state.prompts.clone();
    let server_hook = app_state.server.clone();
    let enable_prompt_broadcast = app_state.server.enable_prompts;

    let on_skill_catalog_mutated: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        prompts.invalidate();
        if !enable_prompt_broadcast {
            return;
        }
        let event = NotificationBuilder::new("notifications/prompts/list_changed")
            .with_empty_params()
            .as_sse_event();
        for sid in server_hook.sessions.all_ids() {
            server_hook.sessions.push_event(&sid, event.clone());
        }
    });

    let registry_context = Arc::new(RegistryContext {
        resource_provider,
        prompt_provider,
        readiness: app_state.readiness.clone(),
        on_skill_catalog_mutated,
    });

    let session_manager = Arc::new(LocalSessionManager::default());

    let mut config = StreamableHttpServerConfig::default();
    // Stateless + JSON-direct mode: each request is independent and
    // responses are plain application/json (no SSE framing). This is
    // compliant with MCP Streamable HTTP spec (2025-06-18) and matches
    // the DCC embedding scenario where the server has at most one active
    // client and does not need cross-request session state.
    config.stateful_mode = false;
    config.json_response = true;
    // Allow any host (production should restrict via reverse proxy).
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

    info!("rmcp MCP endpoint mounted at /mcp");

    router.nest_service("/mcp", service)
}
