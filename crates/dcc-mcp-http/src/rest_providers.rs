//! Adapters that bridge `dcc-mcp-http` internal registries to the
//! `dcc-mcp-skill-rest` provider traits (#818 phase 2 bridge).
//!
//! These adapters are wired into [`SkillRestService`] inside
//! [`super::server`] so that `GET /v1/resources` and `GET /v1/prompts`
//! return real data instead of the default empty responses.
//!
//! `JobController` adapter is deferred until `dcc-mcp-skill-rest` ships
//! the `JobController` trait (PR #824, #818 phase 1b).
//!
//! Each adapter satisfies the DIP boundary in `dcc-mcp-skill-rest`:
//! the REST layer depends on the trait, not on these concrete types.

use std::sync::Arc;

use dcc_mcp_skill_rest::{
    PromptArgumentSpec, PromptContent, PromptGetResponse, PromptListEntry, PromptMessage,
    PromptProvider, ResourceContent, ResourceListEntry, ResourceProvider, ResourceReadResponse,
    ServiceError, ServiceErrorKind,
};
use dcc_mcp_skills::SkillCatalog;

// ── ResourceRegistryAdapter ───────────────────────────────────────────────

/// Bridges [`crate::resources::ResourceRegistry`] to
/// [`ResourceProvider`].
pub(crate) struct ResourceRegistryAdapter {
    registry: crate::resources::ResourceRegistry,
    catalog: Arc<SkillCatalog>,
}

impl ResourceRegistryAdapter {
    pub(crate) fn new(
        registry: crate::resources::ResourceRegistry,
        catalog: Arc<SkillCatalog>,
    ) -> Self {
        Self { registry, catalog }
    }

    fn sync(&self) {
        let catalog = self.catalog.clone();
        self.registry
            .sync_skill_resources(|visit| catalog.for_each_loaded_metadata(|md| visit(md)));
    }
}

impl ResourceProvider for ResourceRegistryAdapter {
    fn list(&self) -> Vec<ResourceListEntry> {
        self.sync();
        self.registry
            .list()
            .into_iter()
            .map(|r| ResourceListEntry {
                uri: r.uri,
                name: r.name,
                description: r.description,
                mime_type: r.mime_type,
            })
            .collect()
    }

    fn read(&self, uri: &str) -> Result<ResourceReadResponse, ServiceError> {
        self.sync();
        self.registry
            .read(uri)
            .map_err(|e| match e {
                crate::resources::ResourceError::NotFound(msg)
                | crate::resources::ResourceError::NotEnabled(msg) => {
                    ServiceError::new(ServiceErrorKind::NotFound, msg)
                }
                crate::resources::ResourceError::Read(msg) => {
                    ServiceError::new(ServiceErrorKind::Internal, msg)
                }
            })
            .map(|result| ResourceReadResponse {
                contents: result
                    .contents
                    .into_iter()
                    .map(|c| ResourceContent {
                        uri: c.uri,
                        mime_type: c.mime_type,
                        text: c.text,
                        blob: c.blob,
                    })
                    .collect(),
            })
    }
}

// ── PromptRegistryAdapter ─────────────────────────────────────────────────

/// Bridges [`crate::prompts::PromptRegistry`] to [`PromptProvider`].
pub(crate) struct PromptRegistryAdapter {
    registry: crate::prompts::PromptRegistry,
    catalog: Arc<SkillCatalog>,
}

impl PromptRegistryAdapter {
    pub(crate) fn new(
        registry: crate::prompts::PromptRegistry,
        catalog: Arc<SkillCatalog>,
    ) -> Self {
        Self { registry, catalog }
    }
}

impl PromptProvider for PromptRegistryAdapter {
    fn list(&self) -> Vec<PromptListEntry> {
        let catalog = self.catalog.clone();
        self.registry
            .list(|visit| catalog.for_each_loaded_metadata(|md| visit(md)))
            .into_iter()
            .map(|p| PromptListEntry {
                name: p.name,
                description: p.description,
                arguments: p
                    .arguments
                    .into_iter()
                    .map(|a| PromptArgumentSpec {
                        name: a.name,
                        description: a.description,
                        required: a.required,
                    })
                    .collect(),
                meta: p.meta,
            })
            .collect()
    }

    fn get(
        &self,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<PromptGetResponse, ServiceError> {
        // Convert JSON arguments Value into HashMap<String, String>
        let args: std::collections::HashMap<String, String> = arguments
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let catalog = self.catalog.clone();
        self.registry
            .get(name, &args, |visit| {
                catalog.for_each_loaded_metadata(|md| visit(md))
            })
            .map_err(|e| match e {
                crate::prompts::PromptError::NotFound(msg) => {
                    ServiceError::new(ServiceErrorKind::NotFound, msg)
                }
                crate::prompts::PromptError::MissingArg(arg) => ServiceError::new(
                    ServiceErrorKind::InvalidParams,
                    format!("missing required argument: {arg}"),
                ),
                crate::prompts::PromptError::Load(msg) => {
                    ServiceError::new(ServiceErrorKind::Internal, msg)
                }
            })
            .map(|result| PromptGetResponse {
                description: result.description,
                messages: result
                    .messages
                    .into_iter()
                    .map(|m| PromptMessage {
                        role: m.role,
                        content: match m.content {
                            dcc_mcp_jsonrpc::McpPromptContent::Text { text } => {
                                PromptContent::Text { text }
                            }
                        },
                    })
                    .collect(),
            })
    }

    fn diagnostics(&self) -> Option<serde_json::Value> {
        let catalog = self.catalog.clone();
        serde_json::to_value(
            self.registry
                .diagnostics(|visit| catalog.for_each_loaded_metadata(|md| visit(md))),
        )
        .ok()
    }
}
