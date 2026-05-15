//! Shared context for [`crate::rmcp_handler::DccMcpHandler`] — wired from
//! `dcc-mcp-http::handler::rmcp_mount`.

use std::sync::Arc;

use dcc_mcp_skill_rest::ReadinessProbe;

use crate::rmcp_providers::{PromptProvider, ResourceProvider};

/// Context carrying providers and cross-cutting hooks used by the rmcp adapter.
#[derive(Clone)]
pub struct RegistryContext {
    /// Resource provider (list + read). `None` if resources are disabled.
    pub resource_provider: Option<Arc<dyn ResourceProvider>>,
    /// Prompt provider (list + get). `None` if prompts are disabled.
    pub prompt_provider: Option<Arc<dyn PromptProvider>>,
    /// Readiness gate for DCC-touching registry tool dispatches (issue #714).
    pub readiness: Arc<dyn ReadinessProbe>,
    /// Invalidate prompts / broadcast after `load_skill` / `unload_skill` / tool groups.
    pub on_skill_catalog_mutated: Arc<dyn Fn() + Send + Sync>,
}
