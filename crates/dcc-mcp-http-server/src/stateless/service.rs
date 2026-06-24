//! Stateless MCP service for protocol version 2026-07-28 (ADR-010 Phase 1).
//!
//! [`StatelessMcpService`] handles JSON-RPC requests for the 2026-07-28
//! stateless protocol path:
//!
//! - No session is created or read.
//! - `Mcp-Session-Id` header is completely ignored.
//! - Every request is self-contained; client identity lives in `_meta`.
//! - `server/discover` replaces `initialize`.
//! - Tool calls route to the existing [`dispatch_rmcp_tool_call`] — the
//!   Tool Registry, Dispatcher, and Catalog are **not** modified.
//!
//! # Feature gate
//!
//! This module is compiled only when the `mcp-2026-07-28` feature is enabled.

use std::sync::Arc;

use serde_json::{Value, json};
use tracing::debug;

use dcc_mcp_jsonrpc::{
    Discover2026PromptsCapability, Discover2026ResourcesCapability, Discover2026ToolsCapability,
    DiscoverCapabilities, DiscoverServerInfo, JsonRpcRequest, JsonRpcResponse,
    MCP_PROTOCOL_VERSION_2026_07_28, ServerDiscoverResult, TasksCapability, error_codes,
};

use crate::mcp_tool_list_builder::{assemble_full_tool_list, slice_tools_page};
use crate::rmcp_registry_context::RegistryContext;
use crate::rmcp_tool_call_dispatch::dispatch_rmcp_tool_call;
use crate::server_state::ServerState;

use super::meta::RequestMeta;

/// Stateless MCP service (2026-07-28 protocol path).
///
/// Clone-cheap: all heavy state is behind `Arc`.
#[derive(Clone)]
pub struct StatelessMcpService {
    state: ServerState,
    registry_context: Arc<RegistryContext>,
}

impl StatelessMcpService {
    /// Create a new service backed by the given server state.
    #[must_use]
    pub fn new(state: ServerState, registry_context: Arc<RegistryContext>) -> Self {
        Self {
            state,
            registry_context,
        }
    }

    /// Handle one JSON-RPC request and return a response value.
    ///
    /// Notifications (requests with no `id`) should be handled by the caller
    /// and not forwarded here; they return `null`.
    pub async fn handle_request(&self, req: &JsonRpcRequest) -> Option<Value> {
        let id = req.id.clone()?;
        let meta = req.params.as_ref().and_then(|p| p.get("_meta")).cloned();
        let _request_meta = RequestMeta::from_value(meta);

        let response = match req.method.as_str() {
            dcc_mcp_jsonrpc::SERVER_DISCOVER_METHOD => self.handle_discover(id),
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "tools/list" => self.handle_tools_list(id, req).await,
            "tools/call" => self.handle_tools_call(id, req).await,
            "resources/list" => self.handle_resources_list(id).await,
            "prompts/list" => self.handle_prompts_list(id).await,
            other => {
                debug!(method = other, "stateless: method not found");
                let error = JsonRpcResponse::method_not_found(Some(id), other);
                serde_json::to_value(error).unwrap_or_else(|_| json!(null))
            }
        };
        Some(response)
    }

    /// `server/discover` — returns server capabilities without creating a session.
    ///
    /// Equivalent to `initialize` in the 2026-07-28 stateless model (SEP-2575).
    fn handle_discover(&self, id: Value) -> Value {
        let caps = self.build_stateless_capabilities();
        let result = ServerDiscoverResult {
            protocol_version: MCP_PROTOCOL_VERSION_2026_07_28.to_string(),
            server_info: DiscoverServerInfo {
                name: self.state.server_name.clone(),
                version: self.state.server_version.clone(),
            },
            capabilities: caps,
            instructions: Some(
                "Direct DCC workflow: search_tools(query) → load_skill → tools/call. \
                 tools/list is paginated; follow nextCursor if you list it."
                    .to_string(),
            ),
        };
        let result_value =
            serde_json::to_value(result).unwrap_or_else(|_| json!({"error": "serialize_failed"}));
        json!({"jsonrpc": "2.0", "id": id, "result": result_value})
    }

    fn build_stateless_capabilities(&self) -> DiscoverCapabilities {
        DiscoverCapabilities {
            tools: Some(Discover2026ToolsCapability { list_changed: true }),
            resources: if self.state.enable_resources {
                Some(Discover2026ResourcesCapability {
                    subscribe: true,
                    list_changed: true,
                })
            } else {
                None
            },
            prompts: if self.state.enable_prompts {
                Some(Discover2026PromptsCapability { list_changed: true })
            } else {
                None
            },
            // Tasks are a first-class capability in 2026-07-28.
            tasks: Some(TasksCapability {}),
            experimental: None,
        }
    }

    /// `tools/list` — returns the paginated tool list.
    ///
    /// No session context: session_id is always `None` in stateless mode.
    async fn handle_tools_list(&self, id: Value, req: &JsonRpcRequest) -> Value {
        let full = assemble_full_tool_list(&self.state, true, None);
        let cursor = req
            .params
            .as_ref()
            .and_then(|p| p.get("cursor"))
            .and_then(Value::as_str);
        let (page, next_cursor) = slice_tools_page(full, cursor);
        let tools: Vec<Value> = page
            .iter()
            .map(|t| serde_json::to_value(t).unwrap_or(Value::Null))
            .collect();
        let mut result = json!({"tools": tools});
        if let Some(c) = next_cursor {
            result["nextCursor"] = Value::String(c);
        }
        debug!(count = tools.len(), "stateless: tools/list");
        json!({"jsonrpc": "2.0", "id": id, "result": result})
    }

    /// `tools/call` — routes to the existing dispatch pipeline (zero registry changes).
    async fn handle_tools_call(&self, id: Value, req: &JsonRpcRequest) -> Value {
        let params = match req.params.as_ref() {
            Some(p) => p,
            None => {
                return json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": error_codes::INVALID_PARAMS, "message": "Missing params"}
                });
            }
        };

        let tool_name = match params.get("name").and_then(Value::as_str) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => {
                return json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": error_codes::INVALID_PARAMS, "message": "Missing tool name"}
                });
            }
        };

        let arguments = params.get("arguments").cloned();

        // Extract _meta for async dispatch / progress routing.
        let call_meta: Option<dcc_mcp_jsonrpc::CallToolMeta> = params
            .get("_meta")
            .and_then(|m| serde_json::from_value(m.clone()).ok());

        debug!(tool = %tool_name, "stateless: tools/call");

        // Stateless path: no session_id.
        let dispatch_result = dispatch_rmcp_tool_call(
            &self.state,
            &self.registry_context,
            None,
            &tool_name,
            arguments,
            call_meta.as_ref(),
        )
        .await;

        match dispatch_result {
            Ok(result) => {
                let result_value = serde_json::to_value(&result)
                    .unwrap_or_else(|_| json!({"isError": true, "content": []}));
                json!({"jsonrpc": "2.0", "id": id, "result": result_value})
            }
            Err(msg) => {
                json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": error_codes::INVALID_PARAMS, "message": msg}
                })
            }
        }
    }

    /// `resources/list` — returns an empty list when resources are disabled.
    async fn handle_resources_list(&self, id: Value) -> Value {
        if !self.state.enable_resources {
            return json!({
                "jsonrpc": "2.0", "id": id,
                "error": {
                    "code": error_codes::METHOD_NOT_FOUND,
                    "message": "Resources not enabled"
                }
            });
        }
        // Stateless path: no resource provider wiring yet (Phase 1).
        json!({"jsonrpc": "2.0", "id": id, "result": {"resources": []}})
    }

    /// `prompts/list` — returns `METHOD_NOT_FOUND` when prompts are disabled
    /// (matching `resources/list` behaviour).
    async fn handle_prompts_list(&self, id: Value) -> Value {
        if !self.state.enable_prompts {
            return json!({
                "jsonrpc": "2.0", "id": id,
                "error": {
                    "code": error_codes::METHOD_NOT_FOUND,
                    "message": "Prompts not enabled"
                }
            });
        }
        json!({"jsonrpc": "2.0", "id": id, "result": {"prompts": []}})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
    use dcc_mcp_skill_rest::StaticReadiness;
    use dcc_mcp_skills::SkillCatalog;
    use serde_json::json;

    fn make_service() -> StatelessMcpService {
        let registry = Arc::new(ToolRegistry::new());
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        let state = ServerState::builder(registry, dispatcher, catalog).build();
        let registry_context = Arc::new(RegistryContext {
            resource_provider: None,
            prompt_provider: None,
            readiness: Arc::new(StaticReadiness::fully_ready()),
            on_skill_catalog_mutated: Arc::new(|| {}),
        });
        StatelessMcpService::new(state, registry_context)
    }

    fn make_request(method: &str, id: Value, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.to_string(),
            params,
        }
    }

    #[tokio::test]
    async fn server_discover_returns_capabilities() {
        let svc = make_service();
        let req = make_request("server/discover", json!(1), None);
        let resp = svc.handle_request(&req).await.expect("has id");

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        let result = &resp["result"];
        assert_eq!(result["protocolVersion"], MCP_PROTOCOL_VERSION_2026_07_28);
        assert_eq!(result["serverInfo"]["name"], "dcc-mcp-http");
        assert!(result["capabilities"]["tools"].is_object());
        assert!(result["capabilities"]["tasks"].is_object());
        assert!(result["instructions"].is_string());
    }

    #[tokio::test]
    async fn server_discover_has_no_session_artifacts() {
        let svc = make_service();
        let req = make_request("server/discover", json!("disc"), None);
        let resp = svc.handle_request(&req).await.expect("has id");

        // Must not include session-scoped fields.
        let result = &resp["result"];
        assert!(result.get("sessionId").is_none());
        assert!(result["capabilities"].get("elicitation").is_none());
        assert!(result["capabilities"].get("logging").is_none());
    }

    #[tokio::test]
    async fn tools_list_returns_paginated_result() {
        let svc = make_service();
        let req = make_request("tools/list", json!(2), None);
        let resp = svc.handle_request(&req).await.expect("has id");

        let tools = resp["result"]["tools"].as_array().expect("tools array");
        // Core tools are always present (search_tools, load_skill, etc.)
        assert!(!tools.is_empty());
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let svc = make_service();
        let req = make_request("initialize", json!(3), None);
        let resp = svc.handle_request(&req).await.expect("has id");

        assert_eq!(resp["error"]["code"], error_codes::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn ping_returns_empty_result() {
        let svc = make_service();
        let req = make_request("ping", json!(4), None);
        let resp = svc.handle_request(&req).await.expect("has id");

        assert_eq!(resp["result"], json!({}));
    }

    #[tokio::test]
    async fn notification_returns_none() {
        let svc = make_service();
        // Notifications have no id.
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };
        let resp = svc.handle_request(&req).await;
        assert!(resp.is_none());
    }

    #[tokio::test]
    async fn tools_call_missing_name_returns_invalid_params() {
        let svc = make_service();
        let req = make_request("tools/call", json!(5), Some(json!({"arguments": {}})));
        let resp = svc.handle_request(&req).await.expect("has id");
        assert_eq!(resp["error"]["code"], error_codes::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn tools_call_unknown_tool_returns_error_result() {
        let svc = make_service();
        let req = make_request(
            "tools/call",
            json!(6),
            Some(json!({"name": "non_existent_tool_xyz", "arguments": {}})),
        );
        let resp = svc.handle_request(&req).await.expect("has id");
        // dispatch_rmcp_tool_call returns Ok(CallToolResult { is_error: true })
        // for unknown tools rather than Err.
        let result = &resp["result"];
        assert_eq!(result["isError"], true);
    }
}
