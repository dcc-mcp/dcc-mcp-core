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

#![allow(deprecated)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CustomRequest, CustomResult, ErrorCode,
    GetPromptRequestParams, GetPromptResponse, GetSkillRequestParams, Implementation,
    InitializeRequestParams, InitializeResult, ListPromptsResult as RmcpListPromptsResult,
    ListResourcesRequestMethod, ListResourcesResult as RmcpListResourcesResult,
    ListSkillsResult as RmcpListSkillsResult, ListToolsResult, LoggingLevel, MetaObject,
    PaginatedRequestParams, ProtocolVersion, ReadResourceDirectoryRequestMethod,
    ReadResourceDirectoryRequestParams,
    ReadResourceDirectoryResult as RmcpReadResourceDirectoryResult, ReadResourceRequestMethod,
    ReadResourceRequestParams, ReadResourceResponse, ServerConfig, SetLevelRequestParams,
    SubscribeRequestParams, Tool as RmcpTool, UnsubscribeRequestParams,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::Value;
use tracing::{debug, warn};

use crate::mcp_tool_list_builder::{assemble_full_tool_list, slice_tools_page};
use crate::rmcp_adapter;
use crate::rmcp_initialize::{build_initialize_result, build_initialize_result_from_value};
use crate::rmcp_providers::ProviderError;
pub use crate::rmcp_registry_context::RegistryContext;
use crate::rmcp_tool_call_dispatch::{call_meta_from_rmcp, dispatch_rmcp_tool_call};
use crate::server_state::ServerState;
use crate::session::SessionLogLevel;
use dcc_mcp_jsonrpc::RESOURCE_NOT_ENABLED_ERROR;

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

    fn merge_call_meta(
        primary: Option<dcc_mcp_jsonrpc::CallToolMeta>,
        fallback: Option<dcc_mcp_jsonrpc::CallToolMeta>,
    ) -> Option<dcc_mcp_jsonrpc::CallToolMeta> {
        match (primary, fallback) {
            (None, None) => None,
            (Some(p), None) => Some(p),
            (None, Some(f)) => Some(f),
            (Some(mut p), Some(f)) => {
                if p.progress_token.is_none() {
                    p.progress_token = f.progress_token;
                }
                match (&mut p.dcc, f.dcc) {
                    (None, fd) => p.dcc = fd,
                    (Some(pd), Some(fd)) => {
                        if !pd.r#async {
                            pd.r#async = fd.r#async;
                        }
                        if pd.parent_job_id.is_none() {
                            pd.parent_job_id = fd.parent_job_id;
                        }
                    }
                    _ => {}
                }
                Some(p)
            }
        }
    }
}

// The rmcp ServerHandler trait uses `impl Future<...>` return types, so clippy's
// `manual_async_fn` suggestion doesn't apply — we must match the trait signature.
#[allow(clippy::manual_async_fn)]
impl ServerHandler for DccMcpHandler {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(crate::rmcp_initialize::server_capabilities(
            &self.state,
            false,
            self.registry_context.skill_provider.is_some(),
        ));
        info.server_info = Implementation::new(
            self.state.server_name.clone(),
            self.state.server_version.clone(),
        );
        info.instructions = Some(
            "Use search_tools or search_skills for compact discovery, \
             get_skill_info for selected schemas, then load_skill only when needed. \
             tools/list is paginated; follow nextCursor if you list it. \
             When the Skills extension is declared, use skills/list and skills/get \
             for workflow instructions and resources/read for supporting files."
                .to_string(),
        );
        info
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![
            ProtocolVersion::V_2026_07_28,
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_03_26,
        ])
    }

    fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<InitializeResult, McpError>> + Send + '_ {
        async move {
            if context.peer.peer_info().is_none() {
                context.peer.set_peer_info(request.clone());
            }
            let sid = self.state.sessions.create();
            let _ = self.state.sessions.mark_initialized(&sid);
            Ok(build_initialize_result(
                &self.state,
                &sid,
                &request,
                self.registry_context.skill_provider.is_some(),
            ))
        }
    }

    fn on_custom_request(
        &self,
        request: CustomRequest,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CustomResult, McpError>> + Send + '_ {
        async move {
            if request.method == "initialize" {
                let sid = self.state.sessions.create();
                let _ = self.state.sessions.mark_initialized(&sid);
                let result = build_initialize_result_from_value(
                    &self.state,
                    &sid,
                    request.params.as_ref(),
                    self.registry_context.skill_provider.is_some(),
                );
                let value = serde_json::to_value(result)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                return Ok(CustomResult(value));
            }
            if request.method == "server/discover" {
                return Ok(CustomResult(self.server_discover_result()));
            }
            if matches!(
                request.method.as_str(),
                "skills/list" | "skills/get" | "resources/directory/read"
            ) {
                return self.handle_skills_request(request).await;
            }
            Err(McpError::new(
                ErrorCode::METHOD_NOT_FOUND,
                request.method,
                None,
            ))
        }
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

            let mut result = ListToolsResult::with_all_items(tools);
            result.next_cursor = next_cursor;
            Ok(result)
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResponse, McpError>> + Send + '_ {
        async move {
            let tool_name = request.name.as_ref();
            let mut arguments = request.arguments.map(Value::Object);

            debug!(tool = %tool_name, "rmcp: dispatching tool call");

            #[cfg(feature = "prometheus")]
            let prom_start = std::time::Instant::now();

            // Back-compat shim: some JSON-RPC clients still send `_meta`
            // nested under `arguments` instead of top-level rmcp `meta`.
            let legacy_meta = match arguments.as_mut() {
                Some(Value::Object(obj)) => obj
                    .remove("_meta")
                    .and_then(|v| serde_json::from_value(v).ok()),
                _ => None,
            };
            let call_meta =
                Self::merge_call_meta(call_meta_from_rmcp(request.meta.as_ref()), legacy_meta);
            let dispatch_result = dispatch_rmcp_tool_call(
                &self.state,
                &self.registry_context,
                None,
                tool_name,
                arguments,
                call_meta.as_ref(),
            )
            .await;

            #[cfg(feature = "prometheus")]
            if let Some(exporter) = self.state.prometheus.as_ref() {
                let status = match &dispatch_result {
                    Ok(result) if result.is_error => "error",
                    Ok(_) => "success",
                    Err(_) => "error",
                };
                exporter.record_tool_call(tool_name, status, prom_start.elapsed());
            }

            match dispatch_result {
                Ok(result) => {
                    if let Some(err) = rmcp_adapter::protocol_error_from_call_result(&result) {
                        return Err(err);
                    }
                    Ok(rmcp_adapter::call_result_to_rmcp(&result).into())
                }
                Err(msg) => Err(McpError::invalid_params(msg, None)),
            }
        }
    }

    // ── Skills extension ────────────────────────────────────────────────────

    fn list_skills(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<RmcpListSkillsResult, McpError>> + Send + '_ {
        async move {
            let Some(provider) = self.registry_context.skill_provider.as_ref() else {
                return Err(McpError::method_not_found::<
                    rmcp::model::ListSkillsRequestMethod,
                >());
            };

            let mut result = provider.list_skills(&self.state.catalog);
            let offset = self.decode_request_cursor_from_option(
                request.as_ref().and_then(|params| params.cursor.as_deref()),
            )?;
            if offset > result.skills.len() {
                return Err(McpError::invalid_params(
                    "cursor is past the end of the skill listing",
                    None,
                ));
            }
            let end = (offset + 32).min(result.skills.len());
            result.next_cursor =
                (end < result.skills.len()).then(|| dcc_mcp_jsonrpc::encode_cursor(end));
            result.skills = result.skills[offset..end].to_vec();

            rmcp_adapter::list_skills_result_to_rmcp(&result)
                .map_err(|error| McpError::internal_error(error, None))
        }
    }

    fn get_skill(
        &self,
        request: GetSkillRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::GetSkillResult, McpError>> + Send + '_ {
        async move {
            let Some(provider) = self.registry_context.skill_provider.as_ref() else {
                return Err(McpError::method_not_found::<
                    rmcp::model::GetSkillRequestMethod,
                >());
            };
            provider
                .get_skill(&request.uri, &self.state.catalog)
                .map_err(|error| provider_error_to_mcp(&error))
                .and_then(|result| {
                    rmcp_adapter::get_skill_result_to_rmcp(&result)
                        .map_err(|error| McpError::internal_error(error, None))
                })
        }
    }

    fn read_resource_directory(
        &self,
        request: ReadResourceDirectoryRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<RmcpReadResourceDirectoryResult, McpError>> + Send + '_ {
        async move {
            let Some(provider) = self.registry_context.skill_provider.as_ref() else {
                return Err(McpError::method_not_found::<
                    ReadResourceDirectoryRequestMethod,
                >());
            };

            let mut result = provider
                .read_skill_directory(&request.uri, &self.state.catalog)
                .map_err(|error| provider_error_to_mcp(&error))?;
            let offset = self.decode_request_cursor_from_option(request.cursor.as_deref())?;
            if offset > result.resources.len() {
                return Err(McpError::invalid_params(
                    "cursor is past the end of the directory listing",
                    None,
                ));
            }
            let end = (offset + 128).min(result.resources.len());
            result.next_cursor =
                (end < result.resources.len()).then(|| dcc_mcp_jsonrpc::encode_cursor(end));
            result.resources = result.resources[offset..end].to_vec();

            Ok(rmcp_adapter::read_resource_directory_result_to_rmcp(
                &result,
            ))
        }
    }

    // ── Resources ───────────────────────────────────────────────────────────

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<RmcpListResourcesResult, McpError>> + Send + '_ {
        async {
            if self.registry_context.resource_provider.is_none()
                && self.registry_context.skill_provider.is_none()
            {
                return Err(McpError::method_not_found::<ListResourcesRequestMethod>());
            }
            let mut resources = self
                .registry_context
                .resource_provider
                .as_ref()
                .map(|provider| provider.list_resources(&self.state.catalog))
                .unwrap_or_default();
            if let Some(provider) = self.registry_context.skill_provider.as_ref() {
                resources.extend(provider.list_skill_resources(&self.state.catalog));
            }
            resources.sort_by(|a, b| a.uri.cmp(&b.uri));
            resources.dedup_by(|a, b| a.uri == b.uri);
            let rmcp_resources: Vec<_> = resources
                .iter()
                .map(rmcp_adapter::resource_to_rmcp)
                .collect();

            debug!(count = rmcp_resources.len(), "rmcp: listed resources");

            Ok(RmcpListResourcesResult::with_all_items(rmcp_resources))
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResponse, McpError>> + Send + '_ {
        async move {
            let is_skill_uri = request.uri.starts_with("skill://");
            if is_skill_uri {
                let Some(provider) = self.registry_context.skill_provider.as_ref() else {
                    return Err(McpError::method_not_found::<ReadResourceRequestMethod>());
                };
                return provider
                    .read_skill_resource(&request.uri, &self.state.catalog)
                    .map(|result| rmcp_adapter::read_result_to_rmcp(&result).into())
                    .map_err(|error| provider_error_to_mcp(&error));
            }
            if self.registry_context.resource_provider.is_none() {
                return Err(McpError::method_not_found::<ReadResourceRequestMethod>());
            }
            let Some(provider) = self.registry_context.resource_provider.as_ref() else {
                return Err(McpError::method_not_found::<ReadResourceRequestMethod>());
            };

            match provider.read_resource(&request.uri, &self.state.catalog) {
                Ok(result) => Ok(rmcp_adapter::read_result_to_rmcp(&result).into()),
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
                let mut result = RmcpListPromptsResult::with_all_items(vec![]);
                result.meta = prompt_diagnostics_meta(Some(serde_json::json!({
                    "enabled": false,
                    "prompt_count": 0,
                    "notes": ["No prompt provider is configured for this server."]
                })));
                return Ok(result);
            };

            let prompts = provider.list_prompts(&self.state.catalog);
            let rmcp_prompts: Vec<_> = prompts.iter().map(rmcp_adapter::prompt_to_rmcp).collect();
            let meta = if rmcp_prompts.is_empty() {
                prompt_diagnostics_meta(provider.prompt_diagnostics(&self.state.catalog))
            } else {
                None
            };

            debug!(count = rmcp_prompts.len(), "rmcp: listed prompts");

            let mut result = RmcpListPromptsResult::with_all_items(rmcp_prompts);
            result.meta = meta;
            Ok(result)
        }
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<GetPromptResponse, McpError>> + Send + '_ {
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
                Ok(result) => Ok(rmcp_adapter::get_prompt_result_to_rmcp(&result).into()),
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

impl DccMcpHandler {
    async fn handle_skills_request(
        &self,
        request: CustomRequest,
    ) -> Result<CustomResult, McpError> {
        let Some(provider) = self.registry_context.skill_provider.as_ref() else {
            return Err(McpError::new(
                ErrorCode::METHOD_NOT_FOUND,
                "Skills extension is not enabled",
                None,
            ));
        };
        let method = request.method;
        let params = request.params.unwrap_or_else(|| serde_json::json!({}));
        let mut result = match method.as_str() {
            "skills/list" => {
                let mut result = provider.list_skills(&self.state.catalog);
                let offset = self.decode_request_cursor(&params)?;
                if offset > result.skills.len() {
                    return Err(McpError::invalid_params(
                        "cursor is past the end of the skill listing",
                        None,
                    ));
                }
                let end = (offset + 32).min(result.skills.len());
                result.next_cursor =
                    (end < result.skills.len()).then(|| dcc_mcp_jsonrpc::encode_cursor(end));
                result.skills = result.skills[offset..end].to_vec();
                serde_json::to_value(result)
            }
            "skills/get" => {
                let params: dcc_mcp_jsonrpc::GetSkillParams = serde_json::from_value(params)
                    .map_err(|_| McpError::invalid_params("Invalid skills/get params", None))?;
                serde_json::to_value(
                    provider
                        .get_skill(&params.uri, &self.state.catalog)
                        .map_err(|error| provider_error_to_mcp(&error))?,
                )
            }
            "resources/directory/read" => {
                let params: dcc_mcp_jsonrpc::ReadResourceDirectoryParams =
                    serde_json::from_value(params).map_err(|_| {
                        McpError::invalid_params("Invalid resources/directory/read params", None)
                    })?;
                let offset = self.decode_request_cursor_from_option(params.cursor.as_deref())?;
                let mut result = provider
                    .read_skill_directory(&params.uri, &self.state.catalog)
                    .map_err(|error| provider_error_to_mcp(&error))?;
                if offset > result.resources.len() {
                    return Err(McpError::invalid_params(
                        "cursor is past the end of the directory listing",
                        None,
                    ));
                }
                let end = (offset + 128).min(result.resources.len());
                result.next_cursor =
                    (end < result.resources.len()).then(|| dcc_mcp_jsonrpc::encode_cursor(end));
                result.resources = result.resources[offset..end].to_vec();
                serde_json::to_value(result)
            }
            _ => unreachable!("skill request was checked before dispatch"),
        }
        .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        if matches!(method.as_str(), "skills/list" | "skills/get")
            && let Some(result) = result.as_object_mut()
        {
            result.insert(
                "resultType".to_string(),
                Value::String("complete".to_string()),
            );
            result.insert("ttlMs".to_string(), Value::from(0));
            result.insert(
                "cacheScope".to_string(),
                Value::String("private".to_string()),
            );
        }
        Ok(CustomResult(result))
    }

    fn server_discover_result(&self) -> Value {
        let skills_enabled = self.registry_context.skill_provider.is_some();
        let mut capabilities = serde_json::json!({
            "tools": {"listChanged": true}
        });
        if self.state.features.enable_resources || skills_enabled {
            capabilities["resources"] = serde_json::json!({
                "subscribe": self.state.features.enable_resources,
                "listChanged": self.state.features.enable_resources
            });
        }
        if self.state.features.enable_prompts {
            capabilities["prompts"] = serde_json::json!({"listChanged": false});
        }
        if skills_enabled {
            capabilities["extensions"] = serde_json::json!({
                "io.modelcontextprotocol/skills": {"directoryRead": true}
            });
        }
        serde_json::json!({
            "supportedVersions": dcc_mcp_jsonrpc::SUPPORTED_PROTOCOL_VERSIONS,
            "capabilities": capabilities,
            "instructions": "Use skills/list and skills/get for workflow instructions, then resources/read for supporting files.",
            "resultType": "complete",
            "ttlMs": 0,
            "cacheScope": "private"
        })
    }

    fn decode_request_cursor(&self, params: &Value) -> Result<usize, McpError> {
        let Some(cursor) = params.get("cursor") else {
            return Ok(0);
        };
        let Some(cursor) = cursor.as_str() else {
            return Err(McpError::invalid_params("cursor must be a string", None));
        };
        self.decode_request_cursor_from_option(Some(cursor))
    }

    fn decode_request_cursor_from_option(&self, cursor: Option<&str>) -> Result<usize, McpError> {
        let Some(cursor) = cursor else {
            return Ok(0);
        };
        dcc_mcp_jsonrpc::decode_cursor(cursor)
            .ok_or_else(|| McpError::invalid_params("cursor is not valid", None))
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
        ProviderError::NotEnabled(msg) => McpError::new(
            ErrorCode(RESOURCE_NOT_ENABLED_ERROR as i32),
            msg.clone(),
            None,
        ),
        ProviderError::MissingArg(msg) => {
            McpError::invalid_params(format!("missing required argument: {msg}"), None)
        }
        ProviderError::Internal(msg) => McpError::internal_error(msg.clone(), None),
    }
}

fn prompt_diagnostics_meta(diagnostics: Option<Value>) -> Option<MetaObject> {
    let Some(Value::Object(diagnostics)) = diagnostics else {
        return None;
    };
    let mut meta = MetaObject::new();
    meta.insert(
        "dcc.prompt_diagnostics".to_string(),
        Value::Object(diagnostics),
    );
    Some(meta)
}
