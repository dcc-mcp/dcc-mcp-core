use serde_json::{Value, json};
use thiserror::Error;

use crate::domain::rest::{
    CallRequest, DescribeRequest, Endpoint, LoadSkillRequest, SearchRequest,
};
use crate::infra::http::{HttpError, HttpGateway};

const MCP_STREAMABLE_HTTP_ACCEPT: &str = "application/json, text/event-stream";
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

#[derive(Debug, Error)]
pub enum ClientError {
    #[error(transparent)]
    Http(#[from] HttpError),
}

pub struct DccMcpClient {
    endpoint: Endpoint,
    gateway: HttpGateway,
}

impl DccMcpClient {
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            gateway: HttpGateway::default(),
        }
    }

    #[must_use]
    pub fn with_gateway(endpoint: Endpoint, gateway: HttpGateway) -> Self {
        Self { endpoint, gateway }
    }

    pub async fn health(&self) -> Result<Value, ClientError> {
        self.gateway
            .get_json(&self.endpoint.path("/v1/healthz"))
            .await
            .map_err(Into::into)
    }

    pub async fn list_instances(&self) -> Result<Value, ClientError> {
        let mut payload = self
            .gateway
            .get_json(&self.endpoint.path("/v1/instances"))
            .await
            .map_err(ClientError::from)?;

        let gateway = match self
            .gateway
            .get_json(&self.endpoint.path("/admin/api/health"))
            .await
        {
            Ok(health) => health.get("gateway").cloned().unwrap_or_else(|| {
                json!({
                    "current": null,
                    "candidates": [],
                    "source": "/admin/api/health"
                })
            }),
            Err(err) => json!({
                "current": null,
                "candidates": [],
                "error": err.to_string(),
                "source": "/admin/api/health"
            }),
        };

        if let Some(obj) = payload.as_object_mut() {
            obj.insert("gateway".to_string(), gateway);
        }
        Ok(payload)
    }

    pub async fn search(&self, request: SearchRequest) -> Result<Value, ClientError> {
        let body = serde_json::to_value(request).unwrap_or_else(|_| json!({}));
        self.gateway
            .post_json(&self.endpoint.path("/v1/search"), &body)
            .await
            .map_err(Into::into)
    }

    pub async fn describe(&self, request: DescribeRequest) -> Result<Value, ClientError> {
        let body = json!({ "tool_slug": request.tool_slug });
        self.gateway
            .post_json(&self.endpoint.path("/v1/describe"), &body)
            .await
            .map_err(Into::into)
    }

    pub async fn load_skill(&self, request: LoadSkillRequest) -> Result<Value, ClientError> {
        self.gateway
            .post_json(&self.endpoint.path("/v1/load_skill"), &request.body)
            .await
            .map_err(Into::into)
    }

    pub async fn call(&self, request: CallRequest) -> Result<Value, ClientError> {
        let body = json!({
            "tool_slug": request.tool_slug,
            "arguments": request.arguments,
            "meta": request.meta,
        });
        self.gateway
            .post_json(&self.endpoint.path("/v1/call"), &body)
            .await
            .map_err(Into::into)
    }

    pub async fn smoke(&self, mcp_url: Option<String>, query: String, limit: usize) -> Value {
        let mcp_url = mcp_url.unwrap_or_else(|| self.endpoint.mcp_url());
        let mut checks = Vec::new();

        let health = self.gateway.get_json(&self.endpoint.path("/health")).await;
        checks.push(check_json("health", &self.endpoint.path("/health"), health));

        let initialize = self
            .gateway
            .post_json_with_headers(
                &mcp_url,
                &json!({
                    "jsonrpc": "2.0",
                    "id": "smoke-initialize",
                    "method": "initialize",
                    "params": {
                        "protocolVersion": MCP_PROTOCOL_VERSION,
                        "capabilities": {},
                        "clientInfo": {
                            "name": "dcc-mcp-cli",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }
                }),
                &[
                    ("Mcp-Protocol-Version", MCP_PROTOCOL_VERSION),
                    ("Accept", MCP_STREAMABLE_HTTP_ACCEPT),
                ],
            )
            .await;
        checks.push(check_json("mcp_initialize", &mcp_url, initialize));

        let tools_list = self
            .gateway
            .post_json_with_headers(
                &mcp_url,
                &json!({
                    "jsonrpc": "2.0",
                    "id": "smoke-tools-list",
                    "method": "tools/list",
                    "params": {}
                }),
                &[
                    ("Mcp-Protocol-Version", MCP_PROTOCOL_VERSION),
                    ("Accept", MCP_STREAMABLE_HTTP_ACCEPT),
                ],
            )
            .await;
        checks.push(check_json("mcp_tools_list", &mcp_url, tools_list));

        let search_body = json!({
            "query": query,
            "limit": limit,
        });
        let search = self
            .gateway
            .post_json(&self.endpoint.path("/v1/search"), &search_body)
            .await;
        checks.push(check_json(
            "rest_search",
            &self.endpoint.path("/v1/search"),
            search,
        ));

        let ok = checks
            .iter()
            .all(|check| check.get("ok").and_then(Value::as_bool).unwrap_or(false));

        json!({
            "ok": ok,
            "base_url": self.endpoint.base_url.clone(),
            "mcp_url": mcp_url,
            "checks": checks,
        })
    }
}

fn check_json(name: &str, url: &str, result: Result<Value, HttpError>) -> Value {
    match result {
        Ok(value) => json!({
            "name": name,
            "url": url,
            "ok": value.get("error").is_none(),
            "response": value,
        }),
        Err(error) => json!({
            "name": name,
            "url": url,
            "ok": false,
            "error": error.to_string(),
        }),
    }
}
