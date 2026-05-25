//! Concrete implementations of the rmcp provider traits for the HTTP layer.
//!
//! These bridge the `ResourceRegistry` and `PromptRegistry` owned by
//! [`crate::handler::AppState`] into the trait-object interface expected by
//! [`dcc_mcp_http_server::rmcp_handler::DccMcpHandler`].

use std::collections::HashMap;
use std::sync::Arc;

use dcc_mcp_http_server::rmcp_providers::{PromptProvider, ProviderError, ResourceProvider};
use dcc_mcp_jsonrpc::{GetPromptResult, McpPrompt, McpResource, ReadResourceResult};
use dcc_mcp_skills::SkillCatalog;

use crate::prompts::{PromptError, PromptRegistry};
use crate::resources::{ResourceError, ResourceRegistry};

// ── ResourceProvider ────────────────────────────────────────────────────────

/// Wraps [`ResourceRegistry`] to implement [`ResourceProvider`].
pub struct ResourceRegistryProvider {
    pub registry: ResourceRegistry,
}

impl ResourceProvider for ResourceRegistryProvider {
    fn list_resources(&self, catalog: &Arc<SkillCatalog>) -> Vec<McpResource> {
        self.registry.sync_skill_resources(|visit| {
            catalog.for_each_loaded_metadata(|md| visit(md));
        });
        self.registry.list()
    }

    fn read_resource(
        &self,
        uri: &str,
        catalog: &Arc<SkillCatalog>,
    ) -> Result<ReadResourceResult, ProviderError> {
        self.registry.sync_skill_resources(|visit| {
            catalog.for_each_loaded_metadata(|md| visit(md));
        });
        self.registry.read(uri).map_err(|e| match e {
            ResourceError::NotFound(m) => ProviderError::NotFound(m),
            ResourceError::NotEnabled(m) => ProviderError::NotEnabled(m),
            ResourceError::Read(m) => ProviderError::Internal(m),
        })
    }
}

// ── PromptProvider ──────────────────────────────────────────────────────────

/// Wraps [`PromptRegistry`] to implement [`PromptProvider`].
pub struct PromptRegistryProvider {
    pub registry: PromptRegistry,
}

impl PromptProvider for PromptRegistryProvider {
    fn list_prompts(&self, catalog: &Arc<SkillCatalog>) -> Vec<McpPrompt> {
        self.registry.list(|visit| {
            catalog.for_each_loaded_metadata(|md| visit(md));
        })
    }

    fn prompt_diagnostics(&self, catalog: &Arc<SkillCatalog>) -> Option<serde_json::Value> {
        serde_json::to_value(self.registry.diagnostics(|visit| {
            catalog.for_each_loaded_metadata(|md| visit(md));
        }))
        .ok()
    }

    fn get_prompt(
        &self,
        name: &str,
        args: &HashMap<String, String>,
        catalog: &Arc<SkillCatalog>,
    ) -> Result<GetPromptResult, ProviderError> {
        self.registry
            .get(name, args, |visit| {
                catalog.for_each_loaded_metadata(|md| visit(md));
            })
            .map_err(|e| match e {
                PromptError::NotFound(m) => ProviderError::NotFound(m),
                PromptError::MissingArg(m) => ProviderError::MissingArg(m),
                PromptError::Load(m) => ProviderError::Internal(m),
            })
    }
}
