use serde_json::{Value, json};
use thiserror::Error;
use tokio::time::{Instant, sleep};

use crate::domain::rest::{
    CallRequest, DescribeRequest, DirectCallRequest, Endpoint, LoadSkillRequest, SearchRequest,
    StopInstanceRequest, WaitReadyRequest,
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
            let payload = self
                .gateway
                .get_json(&self.endpoint.path("/v1/instances"))
                .await
                .map_err(ClientError::from)?;
            match select_instance(
                &payload,
                request.dcc_type.as_deref(),
                request.instance_id.as_deref(),
            ) {
                Ok(Some(instance)) => {
                    let (readiness, source, direct_error) =
                        self.best_readiness_report(&instance).await;
                    let missing = missing_required_fields(readiness.as_ref(), &required);
                    let ready = missing.is_empty();
                    last = json!({
                        "ready": ready,
                        "required": required.clone(),
                        "attempts": attempts,
                        "elapsed_ms": started.elapsed().as_millis() as u64,
                        "instance": instance,
                        "readiness": readiness.unwrap_or(Value::Null),
                        "readiness_source": source,
                        "direct_readyz_error": direct_error,
                        "missing": missing,
                    });
                    if ready {
                        return Ok(last);
                    }
                }
                Ok(None) => {
                    last = json!({
                        "ready": false,
                        "required": required.clone(),
                        "attempts": attempts,
                        "elapsed_ms": started.elapsed().as_millis() as u64,
                        "instance": null,
                        "readiness": null,
                        "missing": required.clone(),
                        "error": {
                            "kind": "instance-not-found-yet",
                            "dcc_type": request.dcc_type,
                            "instance_id": request.instance_id,
                        }
                    });
                }
                Err(error) => {
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

    async fn best_readiness_report(&self, instance: &Value) -> (Option<Value>, Value, Value) {
        let cached = readiness_from_instance(instance);
        if let Some(mcp_url) = instance.get("mcp_url").and_then(Value::as_str) {
            let readyz_url = Endpoint::from_mcp_url(mcp_url).path("/v1/readyz");
            match self.gateway.get_json(&readyz_url).await {
                Ok(value) => {
                    if let Some(report) = normalize_readiness_report(&value) {
                        return (Some(report), json!("direct"), Value::Null);
                    }
                    return (
                        cached,
                        json!("cached"),
                        json!(format!(
                            "direct readyz response did not contain readiness: {readyz_url}"
                        )),
                    );
                }
                Err(err) => {
                    if cached.is_some() {
                        return (cached, json!("cached"), json!(err.to_string()));
                    }
                    return (None, Value::Null, json!(err.to_string()));
                }
            }
        }
        (cached, json!("cached"), Value::Null)
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

fn select_instance(
    payload: &Value,
    dcc_type: Option<&str>,
    instance_hint: Option<&str>,
) -> Result<Option<Value>, Value> {
    let hint = instance_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    if let Some(hint) = hint.as_deref()
        && hint.len() < 4
    {
        return Err(json!({
            "kind": "instance-id-prefix-too-short",
            "instance_id": hint,
            "min_len": 4,
        }));
    }

    let matches: Vec<Value> = payload
        .get("instances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|instance| {
            dcc_type.is_none_or(|expected| {
                instance
                    .get("dcc_type")
                    .and_then(Value::as_str)
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
            })
        })
        .filter(|instance| {
            hint.as_deref().is_none_or(|expected| {
                instance
                    .get("instance_short")
                    .and_then(Value::as_str)
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
                    || instance
                        .get("instance_id")
                        .and_then(Value::as_str)
                        .is_some_and(|actual| instance_id_matches(actual, expected))
            })
        })
        .cloned()
        .collect();

    match matches.as_slice() {
        [] => Ok(None),
        [instance] => Ok(Some(instance.clone())),
        _ => Err(json!({
            "kind": "ambiguous-instance",
            "candidates": matches.iter().map(instance_summary).collect::<Vec<_>>(),
        })),
    }
}

fn instance_id_matches(actual: &str, expected: &str) -> bool {
    let actual = actual.to_ascii_lowercase();
    let actual_simple = actual.replace('-', "");
    actual == expected || actual_simple.starts_with(expected)
}

fn instance_summary(instance: &Value) -> Value {
    json!({
        "dcc_type": instance.get("dcc_type").cloned().unwrap_or(Value::Null),
        "instance_id": instance.get("instance_id").cloned().unwrap_or(Value::Null),
        "instance_short": instance.get("instance_short").cloned().unwrap_or(Value::Null),
        "status": instance.get("status").cloned().unwrap_or(Value::Null),
        "mcp_url": instance.get("mcp_url").cloned().unwrap_or(Value::Null),
    })
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
