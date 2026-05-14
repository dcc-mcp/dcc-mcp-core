//! rmcp [`ServerHandler`] adapter that bridges to our existing [`ServerState`].
//!
//! This module implements rmcp's `ServerHandler` trait by delegating each MCP
//! method to the appropriate existing subsystem (ToolRegistry, ToolDispatcher,
//! SkillCatalog). It is the core adapter enabling rmcp's transport to drive our
//! DCC business logic without modifying the business logic itself.
//!
//! # Gating
//!
//! This entire module is compiled only when the `rmcp-transport` feature is
//! enabled.

use std::future::Future;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResult as RmcpCallToolResult, Content, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool as RmcpTool,
    ToolsCapability,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::Value;
use tracing::{debug, warn};

use crate::server_state::ServerState;

/// Registry context carrying type-erased references to ResourceRegistry and PromptRegistry.
///
/// Stored as Any to avoid circular dependency on dcc-mcp-http.
/// rmcp_mount.rs (in dcc-mcp-http) creates these and passes them to the handler factory.
pub struct RegistryContext {
    /// Type-erased Arc<ResourceRegistry> or None
    pub resource_registry: Option<Arc<dyn std::any::Any + Send + Sync>>,
    /// Type-erased Arc<PromptRegistry> or None
    pub prompt_registry: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

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
    /// Create a new handler instance backed by the given server state and registry context.
    ///
    /// The registry_context carries type-erased Arc pointers to ResourceRegistry and
    /// PromptRegistry from dcc-mcp-http to avoid a direct crate dependency.
    pub fn new(state: ServerState, registry_context: Arc<RegistryContext>) -> Self {
        Self {
            state,
            registry_context,
        }
    }
}

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

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async {
            // Gather tools from the registry (all enabled actions)
            let actions = self.state.registry.list_actions_enabled(None);

            let tools: Vec<RmcpTool> = actions
                .iter()
                .map(|meta| {
                    let input_schema: Arc<rmcp::model::JsonObject> = match &meta.input_schema {
                        Value::Object(map) => Arc::new(map.clone()),
                        _ => Arc::new(
                            serde_json::json!({"type": "object", "properties": {}})
                                .as_object()
                                .unwrap()
                                .clone(),
                        ),
                    };

                    RmcpTool::new(meta.name.clone(), meta.description.clone(), input_schema)
                })
                .collect();

            debug!(count = tools.len(), "rmcp: listed tools from registry");

            Ok(ListToolsResult {
                meta: None,
                next_cursor: None,
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
            let arguments: Value = request
                .arguments
                .map(Value::Object)
                .unwrap_or(Value::Object(serde_json::Map::new()));

            debug!(tool = %tool_name, "rmcp: dispatching tool call");

            // Dispatch through the existing synchronous dispatcher.
            // Note: The dispatcher is sync; we call it from the async context.
            // For main-thread affinity tools, the existing executor pattern would
            // need to be wired in Phase 2. For the spike, we dispatch directly.
            let result = self.state.dispatcher.dispatch(tool_name, arguments);

            match result {
                Ok(dispatch_result) => {
                    // Convert the raw Value output to a CallToolResult
                    let content = vec![Content::text(dispatch_result.output.to_string())];

                    let mut out = RmcpCallToolResult::default();
                    out.content = content;
                    out.structured_content = Some(dispatch_result.output);
                    Ok(out)
                }
                Err(e) => {
                    warn!(tool = %tool_name, error = %e, "rmcp: tool dispatch failed");
                    // Return error as tool result (not an RPC error)
                    let mut out = RmcpCallToolResult::default();
                    out.content = vec![Content::text(e.to_string())];
                    out.is_error = Some(true);
                    Ok(out)
                }
            }
        }
    }
}
