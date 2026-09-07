//! Capability declarations must match the selected lifecycle's handlers.

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
    mut params: Value,
    modern: bool,
) -> Value {
    let mut request = client
        .post(url)
        .header("Accept", "application/json, text/event-stream");
    if modern {
        request = request
            .header("MCP-Protocol-Version", "2026-07-28")
            .header("Mcp-Method", method);
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": {
                "name": "capability-contract", "version": "1"
            }
        });
    }
    let envelope = JsonRpcRequestBuilder::new("capability-check", method)
        .with_params(params)
        .to_value();
    let response = request.json(&envelope).send().await.expect("MCP response");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("JSON-RPC body");
    assert_eq!(body["id"], "capability-check");
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_capabilities_match_provider_support_without_changing_legacy() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("HTTP client");
    for enable_resources in [false, true] {
        for enable_prompts in [false, true] {
            let mut config = McpHttpConfig::default();
            config.server.port = 0;
            config.gateway.gateway_port = 0;
            config.features.enable_resources = enable_resources;
            config.features.enable_prompts = enable_prompts;
            let handle = McpHttpServer::new(Arc::new(ToolRegistry::new()), config)
                .start()
                .await
                .expect("start server");
            let url = format!("http://127.0.0.1:{}/mcp", handle.port);

            #[cfg(feature = "mcp-2026-07-28")]
            {
                let discovery = request(&client, &url, "server/discover", json!({}), true).await;
                let mut expected = json!({"tools": {"listChanged": false}});
                if enable_resources {
                    expected["resources"] = json!({"subscribe": false, "listChanged": false});
                }
                if enable_prompts {
                    expected["prompts"] = json!({"listChanged": false});
                }
                assert_eq!(
                    discovery["result"]["capabilities"], expected,
                    "resources={enable_resources}, prompts={enable_prompts}"
                );
                let tools = request(&client, &url, "tools/list", json!({}), true).await;
                assert!(
                    !tools["result"]["tools"]
                        .as_array()
                        .expect("tools")
                        .is_empty()
                );
            }

            let legacy = request(
                &client,
                &url,
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "capability-contract", "version": "1"}
                }),
                false,
            )
            .await;
            let capabilities = &legacy["result"]["capabilities"];
            assert_eq!(capabilities.get("resources").is_some(), enable_resources);
            assert_eq!(capabilities.get("prompts").is_some(), enable_prompts);
            assert_eq!(capabilities["tools"]["listChanged"], true);
            handle.shutdown().await;
        }
    }
}
