//! The request header selects a lifecycle; initialize cannot upgrade it.

use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_actions::ToolRegistry;
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_jsonrpc::JsonRpcRequestBuilder;
use serde_json::{Value, json};

async fn request(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: Value,
    protocol_header: Option<&str>,
) -> Value {
    let envelope = JsonRpcRequestBuilder::new("lifecycle-check", method)
        .with_params(params)
        .to_value();
    let mut request = client
        .post(url)
        .header("Accept", "application/json, text/event-stream")
        .json(&envelope);
    if let Some(version) = protocol_header {
        request = request.header("MCP-Protocol-Version", version);
    }
    let response = request.send().await.expect("MCP response");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("JSON-RPC body");
    assert_eq!(body["id"], "lifecycle-check");
    body
}

fn initialize_params(version: Option<&str>) -> Value {
    let mut params = json!({
        "capabilities": {},
        "clientInfo": {"name": "protocol-contract", "version": "1"}
    });
    if let Some(version) = version {
        params["protocolVersion"] = json!(version);
    }
    params
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_header_selects_lifecycle_independently_of_initialize_body() {
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = 0;
    let handle = McpHttpServer::new(Arc::new(ToolRegistry::new()), config)
        .start()
        .await
        .expect("start server");
    let url = format!("http://127.0.0.1:{}/mcp", handle.port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    for (requested, expected) in [
        (Some("2025-03-26"), "2025-03-26"),
        (Some("2025-06-18"), "2025-06-18"),
        (Some("2026-07-28"), "2025-06-18"),
        (Some("2099-01-01"), "2025-06-18"),
        (None, "2025-06-18"),
    ] {
        let body = request(
            &client,
            &url,
            "initialize",
            initialize_params(requested),
            None,
        )
        .await;
        assert_eq!(body["result"]["protocolVersion"], expected, "{requested:?}");
    }

    #[cfg(feature = "mcp-2026-07-28")]
    {
        let discovery = request(
            &client,
            &url,
            "server/discover",
            json!({}),
            Some("2026-07-28"),
        )
        .await;
        assert_eq!(discovery["result"]["protocolVersion"], "2026-07-28");
        let initialize = request(
            &client,
            &url,
            "initialize",
            initialize_params(Some("2026-07-28")),
            Some("2026-07-28"),
        )
        .await;
        assert_eq!(initialize["error"]["code"], -32601);
        assert!(initialize.get("result").is_none());
    }

    handle.shutdown().await;
}
