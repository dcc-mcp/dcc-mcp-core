//! Shared [`AppState`] owned by every axum handler in [`crate::handler`].
//!
//! The lower-level runtime state (tool registry, sessions, jobs, executor,
//! in-flight requests, and cache generation) lives in
//! [`dcc_mcp_http_server::ServerState`].  This crate-level state keeps only the
//! application-layer objects that still belong to `dcc-mcp-http`: bridge,
//! resources, prompts, method routing, and readiness.

use std::sync::Arc;

use crate::{
    bridge_registry::BridgeRegistry, prompts::PromptRegistry, resources::ResourceRegistry,
};
use dcc_mcp_http_server::ServerState;
use dcc_mcp_skill_rest::{ReadinessProbe, StaticReadiness};

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
pub struct AppState {
    /// Runtime server state extracted into `dcc-mcp-http-server`.
    pub server: ServerState,
    /// Python / host bridge registry owned by the embedding HTTP crate.
    pub bridge_registry: BridgeRegistry,
    /// MCP Resources primitive registry.
    pub resources: ResourceRegistry,
    /// MCP Prompts primitive registry.
    pub prompts: PromptRegistry,
    /// Pluggable JSON-RPC method router (issue #492).
    pub method_router: Arc<super::router::MethodRouter>,
    /// Shared readiness probe gating DCC-touching `tools/call` dispatches.
    pub readiness: Arc<dyn ReadinessProbe>,
}

impl AppState {
    /// Build a default [`MethodRouter`](super::router::MethodRouter)
    /// pre-populated with every built-in MCP method (issue #492).
    pub fn default_method_router() -> Arc<super::router::MethodRouter> {
        Arc::new(super::router::MethodRouter::with_builtins())
    }

    /// Build the default [`ReadinessProbe`] — a `StaticReadiness`
    /// locked to the fully-ready state (issue #714).
    pub fn default_readiness() -> Arc<dyn ReadinessProbe> {
        Arc::new(StaticReadiness::fully_ready())
    }

    /// Register a custom [`MethodHandler`](super::router::MethodHandler)
    /// for `method`. Replaces any previously-registered handler for the
    /// same method (issue #492).
    pub fn register_method(
        &self,
        method: impl Into<String>,
        handler: Arc<dyn super::router::MethodHandler>,
    ) {
        self.method_router.register(method, handler);
    }
}
