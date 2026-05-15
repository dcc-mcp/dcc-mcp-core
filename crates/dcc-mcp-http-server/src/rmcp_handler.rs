//! rmcp [`ServerHandler`] adapter that bridges to our existing [`ServerState`].
//!
//! This module implements rmcp's `ServerHandler` trait by delegating each MCP
//! method to the appropriate existing subsystem (ToolRegistry, ToolDispatcher,
//! SkillCatalog, ResourceProvider, PromptProvider). It is the core adapter
//! enabling rmcp's transport to drive our DCC business logic without modifying
//! the business logic itself.
//!
//! # Gating
//!
//! This entire module is compiled only when the `rmcp-transport` feature is
//! enabled.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResult as RmcpCallToolResult, GetPromptRequestParams,
    GetPromptResult as RmcpGetPromptResult, Implementation,
    ListPromptsResult as RmcpListPromptsResult, ListResourcesResult as RmcpListResourcesResult,
    ListToolsResult, LoggingLevel, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResult as RmcpReadResourceResult, ServerCapabilities, ServerInfo,
    SetLevelRequestParams, SubscribeRequestParams, Tool as RmcpTool, ToolsCapability,
    UnsubscribeRequestParams,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::Value;
use tracing::{debug, warn};

use crate::mcp_tool_list_builder::{assemble_full_tool_list, slice_tools_page};
use crate::rmcp_adapter;
use crate::rmcp_providers::ProviderError;
pub use crate::rmcp_registry_context::RegistryContext;
use crate::rmcp_tool_call_dispatch::{call_meta_from_rmcp, dispatch_rmcp_tool_call};
use crate::server_state::ServerState;
use crate::session::SessionLogLevel;

/// Adapter that implements rmcp's [`ServerHandler`] trait by delegating to our
/// existing [`ServerState`].
///
/// Created per-session by the service factory closure passed to
/// `StreamableHttpService::new()`.
pub struct DccMcpHandler {
    state: ServerState,
    registry_context: Arc<RegistryContext>,
}

impl DccMcpHandler {
    /// Create a new handler instance backed by the given server state and
    /// registry context.
    #[must_use]
    pub fn new(state: ServerState, registry_context: Arc<RegistryContext>) -> Self {
        Self {
            state,
            registry_context,
        }
    }
}

// The rmcp ServerHandler trait uses `impl Future<...>` return types, so clippy's
// `manual_async_fn` suggestion doesn't apply — we must match the trait signature.
#[allow(clippy::manual_async_fn)]
impl ServerHandler for DccMcpHandler {
    fn get_info(&self) -> ServerInfo {
        let mut capabilities = ServerCapabilities::default();
        capabilities.tools = Some(ToolsCapability {
            list_changed: Some(true),
        });
        if self.state.enable_resources {
            capabilities.resources = Some(rmcp::model::ResourcesCapability {
                subscribe: Some(false),
                list_changed: Some(true),
            });
        }
        if self.state.enable_prompts {
            capabilities.prompts = Some(rmcp::model::PromptsCapability {
                list_changed: Some(false),
            });
        }
        capabilities.logging = Some(serde_json::Map::new());

        let mut info = ServerInfo::new(capabilities);
        info.server_info = Implementation::new(
            self.state.server_name.clone(),
            self.state.server_version.clone(),
        );
        info.instructions = Some(
            "Use search_skills to discover available DCC tools, \
             then load_skill to activate a skill before calling its tools."
                .to_string(),
        );
        info
    }

    // ── Tools ───────────────────────────────────────────────────────────────

    fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move {
            let session_id: Option<&str> = None;
            let include_output_schema = true;
            let full = assemble_full_tool_list(&self.state, include_output_schema, session_id);
            let cursor = request.as_ref().and_then(|p| p.cursor.as_deref());
            let (page, next_cursor) = slice_tools_page(full, cursor);
            let tools: Vec<RmcpTool> = page.iter().map(rmcp_adapter::tool_to_rmcp).collect();

            debug!(count = tools.len(), "rmcp: listed assembled tools");

            Ok(ListToolsResult {
                meta: None,
                next_cursor,
                tools,
            })
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<RmcpCallToolResult, McpError>> + Send + '_ {
        async move {
            let tool_name = request.name.as_ref();
            let arguments = request.arguments.map(Value::Object);

            debug!(tool = %tool_name, "rmcp: dispatching tool call");

            let call_meta = call_meta_from_rmcp(request.meta.as_ref());
            match dispatch_rmcp_tool_call(
                &self.state,
                &self.registry_context,
                None,
                tool_name,
                arguments,
                call_meta.as_ref(),
            )
            .await
            {
                Ok(result) => Ok(rmcp_adapter::call_result_to_rmcp(&result)),
                Err(msg) => Err(McpError::invalid_params(msg, None)),
            }
        }
    }

    // ── Resources ───────────────────────────────────────────────────────────

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<RmcpListResourcesResult, McpError>> + Send + '_ {
        async {
            let Some(provider) = self.registry_context.resource_provider.as_ref() else {
                return Ok(RmcpListResourcesResult {
                    meta: None,
                    next_cursor: None,
                    resources: vec![],
                });
            };

            let resources = provider.list_resources(&self.state.catalog);
            let rmcp_resources: Vec<_> = resources
                .iter()
                .map(rmcp_adapter::resource_to_rmcp)
                .collect();

            debug!(count = rmcp_resources.len(), "rmcp: listed resources");

            Ok(RmcpListResourcesResult {
                meta: None,
                next_cursor: None,
                resources: rmcp_resources,
            })
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<RmcpReadResourceResult, McpError>> + Send + '_ {
        async move {
            let Some(provider) = self.registry_context.resource_provider.as_ref() else {
                return Err(McpError::invalid_params("Resources not enabled", None));
            };

            match provider.read_resource(&request.uri, &self.state.catalog) {
                Ok(result) => Ok(rmcp_adapter::read_result_to_rmcp(&result)),
                Err(e) => {
                    warn!(uri = %request.uri, error = %e, "rmcp: resource read failed");
                    Err(provider_error_to_mcp(&e))
                }
            }
        }
    }

    fn subscribe(
        &self,
        _request: SubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), McpError>> + Send + '_ {
        async {
            // Subscriptions require session-ID mapping not yet wired.
            // Accept silently for now.
            debug!("rmcp: resources/subscribe acknowledged (no-op)");
            Ok(())
        }
    }

    fn unsubscribe(
        &self,
        _request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), McpError>> + Send + '_ {
        async {
            debug!("rmcp: resources/unsubscribe acknowledged (no-op)");
            Ok(())
        }
    }

    // ── Prompts ─────────────────────────────────────────────────────────────

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<RmcpListPromptsResult, McpError>> + Send + '_ {
        async {
            let Some(provider) = self.registry_context.prompt_provider.as_ref() else {
                return Ok(RmcpListPromptsResult {
                    meta: None,
                    next_cursor: None,
                    prompts: vec![],
                });
            };

            let prompts = provider.list_prompts(&self.state.catalog);
            let rmcp_prompts: Vec<_> = prompts.iter().map(rmcp_adapter::prompt_to_rmcp).collect();

            debug!(count = rmcp_prompts.len(), "rmcp: listed prompts");

            Ok(RmcpListPromptsResult {
                meta: None,
                next_cursor: None,
                prompts: rmcp_prompts,
            })
        }
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<RmcpGetPromptResult, McpError>> + Send + '_ {
        async move {
            let Some(provider) = self.registry_context.prompt_provider.as_ref() else {
                return Err(McpError::invalid_params("Prompts not enabled", None));
            };

            // Convert rmcp arguments (JsonObject) to HashMap<String, String>
            let args: HashMap<String, String> = request
                .arguments
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| {
                    let s = v
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| v.to_string());
                    (k, s)
                })
                .collect();

            match provider.get_prompt(&request.name, &args, &self.state.catalog) {
                Ok(result) => Ok(rmcp_adapter::get_prompt_result_to_rmcp(&result)),
                Err(e) => {
                    warn!(name = %request.name, error = %e, "rmcp: get_prompt failed");
                    Err(provider_error_to_mcp(&e))
                }
            }
        }
    }

    // ── Logging ─────────────────────────────────────────────────────────────

    fn set_level(
        &self,
        request: SetLevelRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), McpError>> + Send + '_ {
        async move {
            let level = logging_level_to_session(request.level);
            debug!(?level, "rmcp: set_level acknowledged");
            // Without per-session mapping we cannot route to a specific session.
            // Accept the request so clients don't error out.
            Ok(())
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Map rmcp [`LoggingLevel`] to our [`SessionLogLevel`].
fn logging_level_to_session(level: LoggingLevel) -> SessionLogLevel {
    match level {
        LoggingLevel::Debug => SessionLogLevel::Debug,
        LoggingLevel::Info | LoggingLevel::Notice => SessionLogLevel::Info,
        LoggingLevel::Warning => SessionLogLevel::Warning,
        LoggingLevel::Error
        | LoggingLevel::Critical
        | LoggingLevel::Alert
        | LoggingLevel::Emergency => SessionLogLevel::Error,
    }
}

/// Convert a [`ProviderError`] to an rmcp [`McpError`].
fn provider_error_to_mcp(e: &ProviderError) -> McpError {
    match e {
        ProviderError::NotFound(msg) => McpError::invalid_params(msg.clone(), None),
        ProviderError::NotEnabled(msg) => McpError::invalid_params(msg.clone(), None),
        ProviderError::MissingArg(msg) => {
            McpError::invalid_params(format!("missing required argument: {msg}"), None)
        }
        ProviderError::Internal(msg) => McpError::internal_error(msg.clone(), None),
    }
}
