use serde_json::{Value, json};
use thiserror::Error;
use tokio::time::{Instant, sleep};

use crate::application::instance_selection::select_one_instance;
use crate::domain::rest::{
    CallRequest, DescribeRequest, DirectCallRequest, Endpoint, LoadSkillRequest, SearchRequest,
    StatsRequest, StopInstanceRequest, WaitReadyRequest,
};
use crate::infra::http::{HttpError, HttpGateway};

const MCP_STREAMABLE_HTTP_ACCEPT: &str = "application/json, text/event-stream";
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

#[derive(Debug, Error)]
pub enum ClientError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("MCP protocol error: {0}")]
    Protocol(String),
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

    pub async fn stats(&self, request: StatsRequest) -> Result<Value, ClientError> {
        let mut url = reqwest::Url::parse(&self.endpoint.path("/v1/debug/stats"))
            .map_err(|error| ClientError::Protocol(format!("invalid stats endpoint: {error}")))?;
        url.query_pairs_mut().extend_pairs(request.query_pairs());
        self.gateway
            .get_json(url.as_str())
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
            "tool_slug": &request.tool_slug,
            "arguments": &request.arguments,
            "meta": &request.meta,
        });
        match self
            .gateway
            .post_json(&self.endpoint.path("/v1/call"), &body)
            .await
        {
            Ok(value) => Ok(value),
            Err(error) if is_unknown_rest_tool(&error) => self.call_mcp_tool(request).await,
            Err(error) => Err(error.into()),
        }
    }

    async fn call_mcp_tool(&self, request: CallRequest) -> Result<Value, ClientError> {
        let tool_slug = request.tool_slug;
        let mut params = json!({
            "name": &tool_slug,
            "arguments": request.arguments,
        });
        if let Some(meta) = request.meta {
            params["_meta"] = meta;
        }
        let response = self
            .gateway
            .post_json_with_headers(
                &self.endpoint.mcp_url(),
                &json!({
                    "jsonrpc": "2.0",
                    "id": "dcc-mcp-cli-call",
                    "method": "tools/call",
                    "params": params,
                }),
                &[
                    ("Mcp-Protocol-Version", MCP_PROTOCOL_VERSION),
                    ("Accept", MCP_STREAMABLE_HTTP_ACCEPT),
                ],
            )
            .await?;
        if let Some(error) = response.get("error") {
            return Err(ClientError::Protocol(error.to_string()));
        }
        let result = response
            .get("result")
            .cloned()
            .ok_or_else(|| ClientError::Protocol("missing tools/call result".to_string()))?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            let message = result
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .unwrap_or("tools/call failed");
            return Err(ClientError::Protocol(message.to_string()));
        }
        let output = result.get("structuredContent").cloned().unwrap_or(result);
        Ok(json!({"slug": tool_slug, "output": output}))
    }

    pub async fn call_batch(&self, body: Value) -> Result<Value, ClientError> {
        self.gateway
            .post_json(&self.endpoint.path("/v1/call_batch"), &body)
            .await
            .map_err(Into::into)
    }

    pub async fn direct_call(&self, request: DirectCallRequest) -> Result<Value, ClientError> {
        let body = json!({
            "backend_tool": request.backend_tool,
            "arguments": request.arguments,
            "meta": request.meta,
        });
        let path = format!(
            "/v1/dcc/{}/instances/{}/call",
            request.dcc_type, request.instance_id
        );
        self.gateway
            .post_json(&self.endpoint.path(&path), &body)
            .await
            .map_err(Into::into)
    }

    pub async fn stop_instance(&self, request: StopInstanceRequest) -> Result<Value, ClientError> {
        let body = json!({
            "expected_owner": request.expected_owner,
            "expected_session": request.expected_session,
        });
        let path = format!(
            "/v1/dcc/{}/instances/{}/stop",
            request.dcc_type, request.instance_id
        );
        self.gateway
            .post_json(&self.endpoint.path(&path), &body)
            .await
            .map_err(Into::into)
    }

    pub async fn wait_ready(&self, request: WaitReadyRequest) -> Result<Value, ClientError> {
        let required = normalize_required_fields(request.required);
        if let Some(invalid) = required.iter().find(|field| !is_readiness_field(field)) {
            return Ok(json!({
                "ready": false,
                "required": required.clone(),
                "error": {
                    "kind": "unknown-readiness-field",
                    "field": invalid,
                    "known_fields": READINESS_FIELDS,
                }
            }));
        }

        let started = Instant::now();
        let timeout = request.timeout;
        let interval = request.interval;
        let mut attempts = 0_u64;
        let mut last = json!({
            "ready": false,
            "required": required.clone(),
            "instance": null,
            "readiness": null,
            "missing": required.clone(),
        });

        loop {
            attempts += 1;
            let payload = self.gateway_readyz_or_inventory().await?;
            match readiness_candidate(
                &payload.body,
                request.dcc_type.as_deref(),
                request.instance_id.as_deref(),
            ) {
                ReadinessCandidate::Instance(instance) => {
                    let readiness = readiness_from_instance(&instance);
                    let missing = missing_required_fields(readiness.as_ref(), &required);
                    let ready = missing.is_empty();
                    last = json!({
                        "ready": ready,
                        "required": required.clone(),
                        "attempts": attempts,
                        "elapsed_ms": started.elapsed().as_millis() as u64,
                        "instance": instance,
                        "readiness": readiness.unwrap_or(Value::Null),
                        "readiness_source": payload.source,
                        "gateway_readyz_error": payload.readyz_error,
                        "direct_readyz_error": Value::Null,
                        "missing": missing,
                    });
                    if ready {
                        return Ok(last);
                    }
                }
                ReadinessCandidate::Endpoint(readiness) => {
                    let missing = missing_required_fields(Some(&readiness), &required);
                    let ready = missing.is_empty();
                    last = json!({
                        "ready": ready,
                        "required": required.clone(),
                        "attempts": attempts,
                        "elapsed_ms": started.elapsed().as_millis() as u64,
                        "instance": null,
                        "readiness": readiness,
                        "readiness_source": payload.source,
                        "gateway_readyz_error": payload.readyz_error,
                        "direct_readyz_error": Value::Null,
                        "missing": missing,
                    });
                    if ready {
                        return Ok(last);
                    }
                }
                ReadinessCandidate::None => {
                    last = json!({
                        "ready": false,
                        "required": required.clone(),
                        "attempts": attempts,
                        "elapsed_ms": started.elapsed().as_millis() as u64,
                        "instance": null,
                        "readiness": null,
                        "readiness_source": payload.source,
                        "gateway_readyz_error": payload.readyz_error,
                        "direct_readyz_error": Value::Null,
                        "missing": required.clone(),
                        "error": {
                            "kind": "instance-not-found-yet",
                            "dcc_type": request.dcc_type,
                            "instance_id": request.instance_id,
                        }
                    });
                }
                ReadinessCandidate::Error(error) => {
                    return Ok(json!({
                        "ready": false,
                        "required": required.clone(),
                        "attempts": attempts,
                        "elapsed_ms": started.elapsed().as_millis() as u64,
                        "error": error,
                    }));
                }
            }

            if started.elapsed() >= timeout {
                return Ok(last);
            }
            sleep(interval).await;
        }
    }

    async fn gateway_readyz_or_inventory(&self) -> Result<ReadinessPayload, ClientError> {
        match self
            .gateway
            .get_json(&self.endpoint.path("/v1/readyz"))
            .await
        {
            Ok(body) => Ok(ReadinessPayload {
                body,
                source: json!("gateway_readyz"),
                readyz_error: Value::Null,
            }),
            Err(err) => {
                let readyz_error = json!(err.to_string());
                let body = self
                    .gateway
                    .get_json(&self.endpoint.path("/v1/instances"))
                    .await
                    .map_err(ClientError::from)?;
                Ok(ReadinessPayload {
                    body,
                    source: json!("gateway_inventory"),
                    readyz_error,
                })
            }
        }
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

fn is_unknown_rest_tool(error: &HttpError) -> bool {
    let HttpError::Status { status, body } = error else {
        return false;
    };
    if *status != reqwest::StatusCode::BAD_REQUEST {
        return false;
    }
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|message| message.starts_with("invalid tool slug "))
}

const READINESS_FIELDS: &[&str] = &[
    "process",
    "dcc",
    "skill_catalog",
    "dispatcher",
    "host_execution_bridge",
    "main_thread_executor",
];

const DEFAULT_REQUIRED_READINESS_FIELDS: &[&str] =
    &["process", "dcc", "skill_catalog", "dispatcher"];

struct ReadinessPayload {
    body: Value,
    source: Value,
    readyz_error: Value,
}

enum ReadinessCandidate {
    Instance(Value),
    Endpoint(Value),
    None,
    Error(Value),
}

fn normalize_required_fields(fields: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = fields
        .into_iter()
        .map(|field| field.trim().to_ascii_lowercase().replace('-', "_"))
        .filter(|field| !field.is_empty())
        .collect();
    if normalized.is_empty() {
        normalized = DEFAULT_REQUIRED_READINESS_FIELDS
            .iter()
            .map(|field| (*field).to_string())
            .collect();
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

fn is_readiness_field(field: &str) -> bool {
    READINESS_FIELDS.contains(&field)
}

fn readiness_candidate(
    payload: &Value,
    dcc_type: Option<&str>,
    instance_hint: Option<&str>,
) -> ReadinessCandidate {
    if payload.get("instances").and_then(Value::as_array).is_some() {
        return match select_one_instance(payload, dcc_type, instance_hint) {
            Ok(Some(instance)) => ReadinessCandidate::Instance(instance),
            Ok(None) => ReadinessCandidate::None,
            Err(error) => ReadinessCandidate::Error(error.to_json()),
        };
    }

    if dcc_type.is_none()
        && instance_hint.is_none()
        && let Some(readiness) = normalize_readiness_report(payload)
    {
        return ReadinessCandidate::Endpoint(readiness);
    }

    ReadinessCandidate::None
}

fn readiness_from_instance(instance: &Value) -> Option<Value> {
    instance
        .get("diagnostics")
        .and_then(|diagnostics| diagnostics.get("readiness"))
        .and_then(normalize_readiness_report)
        .or_else(|| {
            instance
                .get("readiness")
                .and_then(normalize_readiness_report)
        })
}

fn normalize_readiness_report(value: &Value) -> Option<Value> {
    if let Some(readiness) = value.get("readiness")
        && readiness.is_object()
    {
        return Some(readiness.clone());
    }
    if value.is_object()
        && READINESS_FIELDS
            .iter()
            .any(|field| value.get(*field).and_then(Value::as_bool).is_some())
    {
        return Some(value.clone());
    }
    None
}

fn missing_required_fields(readiness: Option<&Value>, required: &[String]) -> Vec<String> {
    required
        .iter()
        .filter(|field| {
            readiness
                .and_then(|report| report.get(field.as_str()))
                .and_then(Value::as_bool)
                != Some(true)
        })
        .cloned()
        .collect()
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::Json;
    use axum::Router;
    use axum::extract::Query;
    use axum::routing::get;
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn stats_requests_the_stable_debug_endpoint_with_all_filters() {
        async fn echo(Query(query): Query<HashMap<String, String>>) -> Json<Value> {
            Json(json!({"query": query}))
        }

        let app = Router::new().route("/v1/debug/stats", get(echo));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = DccMcpClient::new(Endpoint::new(format!("http://{addr}")));

        let response = client
            .stats(StatsRequest {
                range: "7d".into(),
                dcc_type: Some("houdini".into()),
                skill: Some("houdini-render".into()),
                tool: Some("render_rop".into()),
                status: Some("failure".into()),
                instance_id: Some("instance-a".into()),
                session_id: Some("solar-session".into()),
            })
            .await
            .unwrap();

        assert_eq!(response["query"]["range"], "7d");
        assert_eq!(response["query"]["dcc_type"], "houdini");
        assert_eq!(response["query"]["skill"], "houdini-render");
        assert_eq!(response["query"]["tool"], "render_rop");
        assert_eq!(response["query"]["status"], "failure");
        assert_eq!(response["query"]["instance_id"], "instance-a");
        assert_eq!(response["query"]["session_id"], "solar-session");
        server.abort();
    }
}
