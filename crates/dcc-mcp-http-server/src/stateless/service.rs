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
    DiscoverResult, JsonRpcRequest, JsonRpcResponse, SUPPORTED_MODERN_PROTOCOL_VERSIONS,
    ServerInfo, StatelessServerCapabilities, ToolsCapability, complete_modern_result, error_codes,
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

/// Transport-neutral origin of a stateless result. Handler errors remain
/// in-band; only ingress and method-registry failures change HTTP status.
#[derive(Debug)]
pub enum StatelessDispatchOutcome {
    Notification,
    Response(Value),
    InvalidEnvelope(Value),
    MethodNotFound(Value),
}

impl StatelessDispatchOutcome {
    pub fn into_response(self) -> Option<Value> {
        match self {
            Self::Notification => None,
            Self::Response(value) | Self::InvalidEnvelope(value) | Self::MethodNotFound(value) => {
                Some(value)
            }
        }
    }
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
        self.handle_request_with_outcome(req).await.into_response()
    }

    /// Preserve error origin for HTTP adapters without inspecting handler
    /// error codes or duplicating the method registry.
    pub async fn handle_request_with_outcome(
        &self,
        req: &JsonRpcRequest,
    ) -> StatelessDispatchOutcome {
        let Some(id) = req.id.clone() else {
            return StatelessDispatchOutcome::Notification;
        };
        let meta = req.params.as_ref().and_then(|p| p.get("_meta"));
        let metadata = match RequestMeta::parse(meta) {
            Ok(meta) => meta,
            Err(issue) => {
                return StatelessDispatchOutcome::InvalidEnvelope(
                    serde_json::to_value(JsonRpcResponse::error_with_data(
                        Some(id),
                        error_codes::INVALID_PARAMS,
                        format!("Invalid request envelope: {}: {}", issue.key, issue.problem),
                        Some(json!({"envelope": issue})),
                    ))
                    .expect("JSON-RPC envelope error"),
                );
            }
        };
        if !dcc_mcp_jsonrpc::SUPPORTED_MODERN_PROTOCOL_VERSIONS
            .contains(&metadata.protocol_version.as_str())
        {
            return StatelessDispatchOutcome::InvalidEnvelope(
                serde_json::to_value(JsonRpcResponse::unsupported_protocol_version(
                    Some(id),
                    &metadata.protocol_version,
                    dcc_mcp_jsonrpc::SUPPORTED_MODERN_PROTOCOL_VERSIONS,
                ))
                .expect("JSON-RPC version error"),
            );
        }
        let mut business_request = req.clone();
        if let Some(params) = business_request.params.as_mut() {
            dcc_mcp_jsonrpc::strip_request_envelope(params);
        }
        let req = &business_request;

        let mut response = match req.method.as_str() {
            "server/discover" => self.handle_discover(id),
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "tools/list" => self.handle_tools_list(id, req).await,
            "tools/call" => self.handle_tools_call(id, req).await,
            "resources/list" => self.handle_resources_list(id).await,
            "prompts/list" => self.handle_prompts_list(id).await,
            other => {
                debug!(method = other, "stateless: method not found");
                let error = JsonRpcResponse::method_not_found(Some(id), other);
                return StatelessDispatchOutcome::MethodNotFound(
                    serde_json::to_value(error).unwrap_or(Value::Null),
                );
            }
        };
        if let Some(result) = response.get_mut("result").and_then(Value::as_object_mut) {
            let server_info = ServerInfo {
                name: self.state.server_name.clone(),
                version: self.state.server_version.clone(),
            };
            if let Err(message) = complete_modern_result(&req.method, result, &server_info) {
                return StatelessDispatchOutcome::Response(
                    serde_json::to_value(JsonRpcResponse::internal_error(req.id.clone(), message))
                        .unwrap_or(Value::Null),
                );
            }
        }
        StatelessDispatchOutcome::Response(response)
    }

    /// `server/discover` — returns server capabilities without creating a session.
    ///
    /// Equivalent to `initialize` in the 2026-07-28 stateless model (SEP-2575).
    fn handle_discover(&self, id: Value) -> Value {
        let caps = self.build_stateless_capabilities();
        let result = DiscoverResult {
            supported_versions: SUPPORTED_MODERN_PROTOCOL_VERSIONS
                .iter()
                .map(|v| (*v).to_string())
                .collect(),
            capabilities: caps,
            instructions: Some(
                "Direct DCC workflow: search_tools(query) → load_skill → tools/call. \
                 tools/list is paginated; follow nextCursor if you list it."
                    .to_string(),
            ),
            ..Default::default()
        };
        let result_value =
            serde_json::to_value(result).unwrap_or_else(|_| json!({"error": "serialize_failed"}));
        json!({"jsonrpc": "2.0", "id": id, "result": result_value})
    }

    fn build_stateless_capabilities(&self) -> StatelessServerCapabilities {
        StatelessServerCapabilities {
            // This path has no subscription stream. Resources and prompts
            // remain unadvertised until their read/get handlers are wired.
            // The final 2026 revision does not include the task wire surface.
            tools: Some(ToolsCapability {
                list_changed: false,
            }),
            ..Default::default()
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

        // Legacy callers may coerce JSON-string arguments in the shared
        // dispatcher. The modern wire contract must validate the raw object
        // before dispatch (and before schema-driven parameter-header checks).
        if params
            .get("arguments")
            .is_some_and(|arguments| !arguments.is_object())
        {
            return serde_json::to_value(JsonRpcResponse::invalid_params(
                Some(id),
                "arguments must be an object when present",
            ))
            .unwrap_or(Value::Null);
        }
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
        if !self.state.features.enable_resources {
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

    /// `prompts/list` — returns an empty list when prompts are disabled.
    async fn handle_prompts_list(&self, id: Value) -> Value {
        if !self.state.features.enable_prompts {
            return json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"prompts": []}
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
    use dcc_mcp_jsonrpc::{MCP_PROTOCOL_VERSION_2026_07_28, SERVER_INFO_META_KEY};
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
        let mut params = params.unwrap_or_else(|| json!({}));
        params["_meta"] = json!({
            dcc_mcp_jsonrpc::PROTOCOL_VERSION_META_KEY: "2026-07-28",
            dcc_mcp_jsonrpc::CLIENT_CAPABILITIES_META_KEY: {}
        });
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.to_string(),
            params: Some(params),
        }
    }

    #[tokio::test]
    async fn direct_stateless_dispatch_requires_valid_request_metadata() {
        let svc = make_service();
        let mut request = make_request("tools/list", json!("metadata"), None);
        request.params = None;
        let response = svc.handle_request(&request).await.unwrap();
        assert_eq!(response["error"]["code"], error_codes::INVALID_PARAMS);
        request.params = Some(json!({"_meta": {
            dcc_mcp_jsonrpc::PROTOCOL_VERSION_META_KEY: "2099-01-01",
            dcc_mcp_jsonrpc::CLIENT_CAPABILITIES_META_KEY: {}
        }}));
        let response = svc.handle_request(&request).await.unwrap();
        assert_eq!(
            response["error"]["code"],
            error_codes::UNSUPPORTED_PROTOCOL_VERSION
        );
        assert_eq!(response["error"]["data"]["requested"], "2099-01-01");
    }

    #[tokio::test]
    async fn server_discover_returns_capabilities() {
        let svc = make_service();
        let req = make_request("server/discover", json!(1), None);
        let resp = svc.handle_request(&req).await.expect("has id");

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        let result = &resp["result"];
        assert_eq!(
            result["supportedVersions"],
            json!([MCP_PROTOCOL_VERSION_2026_07_28])
        );
        assert_eq!(
            result["_meta"][SERVER_INFO_META_KEY]["name"],
            "dcc-mcp-http"
        );
        assert!(result.get("protocolVersion").is_none());
        assert!(result.get("serverInfo").is_none());
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["ttlMs"], 0);
        assert_eq!(result["cacheScope"], "private");
        assert!(result["capabilities"]["tools"].is_object());
        assert!(result["capabilities"].get("tasks").is_none());
        assert!(result["instructions"].is_string());
    }

    #[tokio::test]
    async fn discovery_advertises_only_implemented_stateless_capabilities() {
        for enable_resources in [false, true] {
            for enable_prompts in [false, true] {
                let mut svc = make_service();
                svc.state.features.enable_resources = enable_resources;
                svc.state.features.enable_prompts = enable_prompts;
                let req = make_request("server/discover", json!("capabilities"), None);
                let resp = svc.handle_request(&req).await.expect("has id");

                assert_eq!(
                    resp["result"]["capabilities"],
                    json!({"tools": {"listChanged": false}}),
                    "resources={enable_resources}, prompts={enable_prompts}"
                );
            }
        }
    }

    #[tokio::test]
    async fn unadvertised_stateless_methods_remain_unsupported() {
        let svc = make_service();
        for method in [
            "resources/read",
            "prompts/get",
            "subscriptions/listen",
            "tasks/get",
            "tasks/cancel",
        ] {
            let req = make_request(method, json!(method), Some(json!({})));
            let resp = svc.handle_request(&req).await.expect("has id");
            assert_eq!(
                resp["error"]["code"],
                error_codes::METHOD_NOT_FOUND,
                "{method}"
            );
            assert!(resp.get("result").is_none(), "{method}");
        }
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
        let outcome = svc.handle_request_with_outcome(&req).await;
        assert!(matches!(
            outcome,
            StatelessDispatchOutcome::MethodNotFound(_)
        ));
        let resp = outcome.into_response().expect("has id");
        assert_eq!(resp["error"]["code"], error_codes::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn ping_returns_empty_result() {
        let svc = make_service();
        let req = make_request("ping", json!(4), None);
        let resp = svc.handle_request(&req).await.expect("has id");

        assert_eq!(resp["result"]["resultType"], "complete");
        assert!(resp["result"]["_meta"][SERVER_INFO_META_KEY].is_object());
        assert!(resp["result"].get("ttlMs").is_none());
        assert!(resp["result"].get("cacheScope").is_none());
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
        let outcome = svc.handle_request_with_outcome(&req).await;
        assert!(matches!(outcome, StatelessDispatchOutcome::Response(_)));
        let resp = outcome.into_response().expect("has id");
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
        assert_eq!(result["resultType"], "complete");
        assert!(result["_meta"][SERVER_INFO_META_KEY].is_object());
        assert!(result.get("ttlMs").is_none());
    }

    #[tokio::test]
    async fn modern_argument_shape_rejects_before_handler_without_changing_legacy_coercion() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let svc = make_service();
        let calls = Arc::new(AtomicUsize::new(0));
        svc.state
            .registry
            .register_action(dcc_mcp_actions::ToolMeta {
                name: "bounded_probe".into(),
                input_schema: json!({"type":"object","properties":{}}),
                ..Default::default()
            });
        let observed = calls.clone();
        svc.state
            .dispatcher
            .register_handler("bounded_probe", move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"ok":true}))
            });
        for arguments in [json!(null), json!("{}"), json!([]), json!(false)] {
            let request = make_request(
                "tools/call",
                json!("invalid"),
                Some(json!({"name":"bounded_probe","arguments":arguments})),
            );
            let response = svc.handle_request(&request).await.unwrap();
            assert_eq!(response["error"]["code"], error_codes::INVALID_PARAMS);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        for params in [
            json!({"name":"bounded_probe"}),
            json!({"name":"bounded_probe","arguments":{}}),
        ] {
            let request = make_request("tools/call", json!("valid"), Some(params));
            let response = svc.handle_request(&request).await.unwrap();
            assert_ne!(response["result"]["isError"], true, "{response}");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let legacy = dispatch_rmcp_tool_call(
            &svc.state,
            &svc.registry_context,
            None,
            "bounded_probe",
            Some(json!("{}")),
            None,
        )
        .await
        .unwrap();
        assert!(!legacy.is_error);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
