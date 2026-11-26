//! Shared MCP Streamable HTTP client for DCCMCP products.
//!
//! The client owns MCP wire concerns so the CLI, desktop integration, and
//! other agent-facing entry points use the same handshake and session logic.

use std::time::Duration;

use dcc_mcp_jsonrpc::{
    CallToolResult, GetPromptResult, InitializeResult, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, ListPromptsResult, ListResourcesResult, ListToolsResult, MCP_PROTOCOL_VERSION,
    MCP_SESSION_HEADER, McpPrompt, McpResource, McpTool, ReadResourceResult,
    SUPPORTED_PROTOCOL_VERSIONS,
};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};
use thiserror::Error;

pub const STREAMABLE_HTTP_ACCEPT: &str = "application/json, text/event-stream";
pub const DEFAULT_CLIENT_NAME: &str = "dccmcp";

#[derive(Debug, Error)]
pub enum McpClientError {
    #[error("invalid MCP endpoint: {0}")]
    InvalidEndpoint(String),
    #[error("MCP HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("MCP server returned HTTP {status}: {body}")]
    HttpStatus {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("MCP response could not be decoded: {0}")]
    Json(#[from] serde_json::Error),
    #[error("MCP protocol error {code}: {message}")]
    JsonRpc {
        code: i64,
        message: String,
        data: Option<Value>,
    },
    #[error("MCP protocol error: {0}")]
    Protocol(String),
}

#[derive(Clone)]
pub struct McpHttpClient {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    timeout: Duration,
    client_name: String,
    client_version: String,
    protocol_version: String,
    session_id: Option<String>,
    initialized: bool,
    next_request_id: u64,
}

impl McpHttpClient {
    pub fn new(endpoint: impl AsRef<str>) -> Result<Self, McpClientError> {
        Self::with_timeout(endpoint, Duration::from_secs(30))
    }

    pub fn with_timeout(
        endpoint: impl AsRef<str>,
        timeout: Duration,
    ) -> Result<Self, McpClientError> {
        let endpoint = reqwest::Url::parse(endpoint.as_ref())
            .map_err(|error| McpClientError::InvalidEndpoint(error.to_string()))?;
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        Ok(Self {
            client,
            endpoint,
            timeout,
            client_name: DEFAULT_CLIENT_NAME.to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            session_id: None,
            initialized: false,
            next_request_id: 1,
        })
    }

    #[must_use]
    pub fn with_client_info(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.client_name = name.into();
        self.client_version = version.into();
        self
    }

    #[must_use]
    pub fn endpoint(&self) -> &reqwest::Url {
        &self.endpoint
    }

    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    #[must_use]
    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }

    pub async fn initialize(&mut self) -> Result<InitializeResult, McpClientError> {
        let protocol_version = self.protocol_version.clone();
        let client_name = self.client_name.clone();
        let client_version = self.client_version.clone();
        let result = self
            .send_request(
                "initialize",
                Some(json!({
                    "protocolVersion": protocol_version,
                    "capabilities": {},
                    "clientInfo": {
                        "name": client_name,
                        "version": client_version,
                    }
                })),
            )
            .await?;
        let initialized: InitializeResult = serde_json::from_value(result)?;
        if !SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .any(|version| *version == initialized.protocol_version)
        {
            return Err(McpClientError::Protocol(format!(
                "server selected unsupported protocol version {}",
                initialized.protocol_version
            )));
        }
        self.protocol_version = initialized.protocol_version.clone();
        self.initialized = true;
        self.send_notification("notifications/initialized", None)
            .await?;
        Ok(initialized)
    }

    pub async fn request(
        &mut self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<Value, McpClientError> {
        self.ensure_initialized().await?;
        self.send_request(&method.into(), params).await
    }

    pub async fn tools_list(&mut self) -> Result<ListToolsResult, McpClientError> {
        let result = self.request("tools/list", Some(json!({}))).await?;
        Ok(serde_json::from_value(result)?)
    }

    pub async fn tools_list_all(&mut self) -> Result<Vec<McpTool>, McpClientError> {
        let mut cursor = None;
        let mut tools = Vec::new();
        loop {
            let mut params = serde_json::Map::new();
            if let Some(cursor) = cursor.take() {
                params.insert("cursor".to_string(), Value::String(cursor));
            }
            let page: ListToolsResult = serde_json::from_value(
                self.request("tools/list", Some(Value::Object(params)))
                    .await?,
            )?;
            tools.extend(page.tools);
            cursor = page.next_cursor;
            if cursor.is_none() {
                return Ok(tools);
            }
        }
    }

    pub async fn tools_call(
        &mut self,
        name: impl Into<String>,
        arguments: Value,
        meta: Option<Value>,
    ) -> Result<CallToolResult, McpClientError> {
        let mut params = json!({
            "name": name.into(),
            "arguments": arguments,
        });
        if let Some(meta) = meta {
            params["_meta"] = meta;
        }
        let result = self.request("tools/call", Some(params)).await?;
        Ok(serde_json::from_value(result)?)
    }

    pub async fn resources_list(&mut self) -> Result<ListResourcesResult, McpClientError> {
        let result = self.request("resources/list", Some(json!({}))).await?;
        Ok(serde_json::from_value(result)?)
    }

    pub async fn resources_list_all(&mut self) -> Result<Vec<McpResource>, McpClientError> {
        let mut cursor = None;
        let mut resources = Vec::new();
        loop {
            let mut params = serde_json::Map::new();
            if let Some(cursor) = cursor.take() {
                params.insert("cursor".to_string(), Value::String(cursor));
            }
            let page: ListResourcesResult = serde_json::from_value(
                self.request("resources/list", Some(Value::Object(params)))
                    .await?,
            )?;
            resources.extend(page.resources);
            cursor = page.next_cursor;
            if cursor.is_none() {
                return Ok(resources);
            }
        }
    }

    pub async fn resources_read(
        &mut self,
        uri: impl Into<String>,
    ) -> Result<ReadResourceResult, McpClientError> {
        let result = self
            .request("resources/read", Some(json!({ "uri": uri.into() })))
            .await?;
        Ok(serde_json::from_value(result)?)
    }

    pub async fn prompts_list(&mut self) -> Result<ListPromptsResult, McpClientError> {
        let result = self.request("prompts/list", Some(json!({}))).await?;
        Ok(serde_json::from_value(result)?)
    }

    pub async fn prompts_list_all(&mut self) -> Result<Vec<McpPrompt>, McpClientError> {
        let mut cursor = None;
        let mut prompts = Vec::new();
        loop {
            let mut params = serde_json::Map::new();
            if let Some(cursor) = cursor.take() {
                params.insert("cursor".to_string(), Value::String(cursor));
            }
            let page: ListPromptsResult = serde_json::from_value(
                self.request("prompts/list", Some(Value::Object(params)))
                    .await?,
            )?;
            prompts.extend(page.prompts);
            cursor = page.next_cursor;
            if cursor.is_none() {
                return Ok(prompts);
            }
        }
    }

    pub async fn prompts_get(
        &mut self,
        name: impl Into<String>,
        arguments: serde_json::Map<String, Value>,
    ) -> Result<GetPromptResult, McpClientError> {
        let result = self
            .request(
                "prompts/get",
                Some(json!({ "name": name.into(), "arguments": arguments })),
            )
            .await?;
        Ok(serde_json::from_value(result)?)
    }

    async fn ensure_initialized(&mut self) -> Result<(), McpClientError> {
        if !self.initialized {
            self.initialize().await?;
        }
        Ok(())
    }

    async fn send_request(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, McpClientError> {
        let id = Value::from(self.next_request_id);
        self.next_request_id += 1;
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id.clone()),
            method: method.to_string(),
            params,
        };
        let response = self.send_body(serde_json::to_value(request)?).await?;
        let response = parse_response(&response, &id)?;
        self.update_session(&response.headers);
        match response.message.error {
            Some(error) => Err(McpClientError::JsonRpc {
                code: error.code,
                message: error.message,
                data: error.data,
            }),
            None => response.message.result.ok_or_else(|| {
                McpClientError::Protocol(format!("MCP {method} response had no result"))
            }),
        }
    }

    async fn send_notification(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), McpClientError> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };
        let response = self.send_body(serde_json::to_value(notification)?).await?;
        self.update_session(&response.headers);
        Ok(())
    }

    async fn send_body(&self, body: Value) -> Result<RawResponse, McpClientError> {
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, STREAMABLE_HTTP_ACCEPT)
            .header("Mcp-Protocol-Version", self.protocol_version.as_str())
            .json(&body);
        if let Some(session_id) = &self.session_id {
            request = request.header(MCP_SESSION_HEADER, session_id);
        }
        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(McpClientError::HttpStatus { status, body });
        }
        Ok(RawResponse { headers, body })
    }

    fn update_session(&mut self, headers: &reqwest::header::HeaderMap) {
        if let Some(session_id) = headers
            .get(MCP_SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
        {
            self.session_id = Some(session_id.to_string());
        }
        if let Some(protocol_version) = headers
            .get("Mcp-Protocol-Version")
            .and_then(|value| value.to_str().ok())
        {
            if SUPPORTED_PROTOCOL_VERSIONS.contains(&protocol_version) {
                self.protocol_version = protocol_version.to_string();
            }
        }
    }
}

struct RawResponse {
    headers: reqwest::header::HeaderMap,
    body: String,
}

struct ParsedResponse {
    headers: reqwest::header::HeaderMap,
    message: JsonRpcResponse,
}

fn parse_response(
    raw: &RawResponse,
    expected_id: &Value,
) -> Result<ParsedResponse, McpClientError> {
    if raw.body.trim().is_empty() {
        return Err(McpClientError::Protocol(
            "MCP response body was empty".to_string(),
        ));
    }

    if let Ok(message) = serde_json::from_str::<JsonRpcResponse>(&raw.body) {
        return Ok(ParsedResponse {
            headers: raw.headers.clone(),
            message,
        });
    }

    let mut event_data = Vec::new();
    let mut current = Vec::new();
    for line in raw.body.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            current.push(data.trim_start().to_string());
        } else if line.trim().is_empty() && !current.is_empty() {
            event_data.push(current.join("\n"));
            current.clear();
        }
    }
    if !current.is_empty() {
        event_data.push(current.join("\n"));
    }

    for data in event_data {
        if data == "[DONE]" {
            continue;
        }
        let value: Value = serde_json::from_str(&data)?;
        let message: JsonRpcResponse = serde_json::from_value(value)?;
        if message.id.as_ref() == Some(expected_id)
            || message.result.is_some()
            || message.error.is_some()
        {
            return Ok(ParsedResponse {
                headers: raw.headers.clone(),
                message,
            });
        }
    }

    Err(McpClientError::Protocol(
        "MCP response was neither JSON nor a JSON-RPC SSE event".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::Response;
    use axum::routing::post;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

    #[derive(Clone, Default)]
    struct FixtureState {
        requests: Arc<Mutex<Vec<Value>>>,
    }

    struct Fixture {
        url: String,
        state: FixtureState,
        shutdown: Option<oneshot::Sender<()>>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(shutdown) = self.shutdown.take() {
                let _ = shutdown.send(());
            }
        }
    }

    async fn mcp_handler(
        State(state): State<FixtureState>,
        headers: HeaderMap,
        body: String,
    ) -> Response<String> {
        let request: Value = serde_json::from_str(&body).unwrap();
        state.requests.lock().unwrap().push(request.clone());
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request["method"].as_str().unwrap();
        let mut response = Response::new(match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "fixture", "version": "1"}
                }
            })
            .to_string(),
            "notifications/initialized" => String::new(),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"tools": [{"name": "create_sphere", "description": "Create a sphere", "inputSchema": {"type": "object"}}]}
            })
            .to_string(),
            "tools/call" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"content": [{"type": "text", "text": "ok"}], "isError": false}
            })
            .to_string(),
            "resources/list" => {
                let first_page = request.pointer("/params/cursor").is_none();
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": if first_page {
                        json!({
                            "resources": [{"uri": "scene://active", "name": "Active Scene", "mimeType": "application/json"}],
                            "nextCursor": "resource-page-2"
                        })
                    } else {
                        json!({
                            "resources": [{"uri": "scene://selection", "name": "Selection", "mimeType": "application/json"}]
                        })
                    }
                })
                .to_string()
            }
            "resources/read" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"contents": [{"uri": "scene://active", "mimeType": "application/json", "text": "{\"frame\":1001}"}]}
            })
            .to_string(),
            "prompts/list" => {
                let first_page = request.pointer("/params/cursor").is_none();
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": if first_page {
                        json!({
                            "prompts": [{"name": "review_scene", "description": "Review the active scene"}],
                            "nextCursor": "prompt-page-2"
                        })
                    } else {
                        json!({
                            "prompts": [{"name": "render_review"}]
                        })
                    }
                })
                .to_string()
            }
            "prompts/get" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"description": "Rendered scene review", "messages": [{"role": "user", "content": {"type": "text", "text": "Review frame 1001"}}]}
            })
            .to_string(),
            _ => panic!("unexpected method {method}"),
        });
        if method == "notifications/initialized" {
            *response.status_mut() = StatusCode::ACCEPTED;
        } else {
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
            response
                .headers_mut()
                .insert(MCP_SESSION_HEADER, "fixture-session".parse().unwrap());
        }
        if headers.get(MCP_SESSION_HEADER).is_some() && method == "tools/list" {
            assert_eq!(headers[MCP_SESSION_HEADER], "fixture-session");
        }
        response
    }

    async fn fixture() -> Fixture {
        let state = FixtureState::default();
        let app = Router::new()
            .route("/mcp", post(mcp_handler))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        Fixture {
            url: format!("http://{addr}/mcp"),
            state,
            shutdown: Some(shutdown_tx),
        }
    }

    #[tokio::test]
    async fn initializes_once_and_reuses_the_session_for_tools() {
        let fixture = fixture().await;
        let mut client = McpHttpClient::new(&fixture.url).unwrap();
        let page = client.tools_list().await.unwrap();
        assert_eq!(page.tools[0].name, "create_sphere");
        let result = client
            .tools_call("create_sphere", json!({"radius": 2}), None)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(client.session_id(), Some("fixture-session"));
        let requests = fixture.state.requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[0]["method"], "initialize");
        assert_eq!(requests[1]["method"], "notifications/initialized");
        assert_eq!(requests[2]["method"], "tools/list");
        assert_eq!(requests[3]["method"], "tools/call");
    }

    #[tokio::test]
    async fn lists_and_reads_resources_and_prompts_with_pagination() {
        let fixture = fixture().await;
        let mut client = McpHttpClient::new(&fixture.url).unwrap();

        let resources = client.resources_list_all().await.unwrap();
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[1].uri, "scene://selection");
        let resource = client.resources_read("scene://active").await.unwrap();
        assert_eq!(
            resource.contents[0].text.as_deref(),
            Some("{\"frame\":1001}")
        );

        let prompts = client.prompts_list_all().await.unwrap();
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0].name, "review_scene");
        let mut arguments = serde_json::Map::new();
        arguments.insert("frame".to_string(), json!("1001"));
        let prompt = client.prompts_get("review_scene", arguments).await.unwrap();
        assert_eq!(prompt.messages[0].role, "user");
    }

    #[test]
    fn parses_json_rpc_error_details() {
        let raw = RawResponse {
            headers: HeaderMap::new(),
            body: json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": {"code": -32601, "message": "missing", "data": {"tool": "x"}}
            })
            .to_string(),
        };
        let response = parse_response(&raw, &json!(1)).unwrap();
        let error = response.message.error.unwrap();
        assert_eq!(error.code, -32601);
        assert_eq!(error.message, "missing");
        assert_eq!(error.data, Some(json!({"tool": "x"})));
    }

    #[test]
    fn parses_streamable_http_sse_response() {
        let raw = RawResponse {
            headers: HeaderMap::new(),
            body:
                "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n"
                    .to_string(),
        };
        let response = parse_response(&raw, &json!(7)).unwrap();
        assert_eq!(response.message.result, Some(json!({"ok": true})));
    }
}
