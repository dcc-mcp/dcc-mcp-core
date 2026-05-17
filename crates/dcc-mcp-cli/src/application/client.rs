use serde_json::{Value, json};
use thiserror::Error;

use crate::domain::rest::{CallRequest, DescribeRequest, Endpoint, SearchRequest};
use crate::infra::http::{HttpError, HttpGateway};

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
        self.gateway
            .get_json(&self.endpoint.path("/v1/instances"))
            .await
            .map_err(Into::into)
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
}
