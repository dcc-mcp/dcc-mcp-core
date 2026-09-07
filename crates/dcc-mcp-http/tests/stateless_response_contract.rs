//! Modern response projection over HTTP, independent of request validation work.

#![cfg(feature = "mcp-2026-07-28")]

use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_actions::ToolRegistry;
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_jsonrpc::{JsonRpcRequestBuilder, SERVER_INFO_META_KEY};
use serde_json::{Value, json};

async fn modern_request(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    mut params: Value,
) -> Value {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "response-contract", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let mut request = client
        .post(url)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", method);
    if let Some(name) = params.get("name").and_then(Value::as_str) {
        request = request.header("Mcp-Name", name);
    }
    let response = request
        .json(
            &JsonRpcRequestBuilder::new(method, method)
                .with_params(params)
                .to_value(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(response.headers().get("Mcp-Session-Id").is_none());
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["id"], method);
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discover_list_call_and_errors_use_the_modern_response_boundary() {
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = 0;
    let handle = McpHttpServer::new(Arc::new(ToolRegistry::new()), config)
        .start()
        .await
        .unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", handle.port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    for (method, params, cacheable) in [
        ("server/discover", json!({}), true),
        ("tools/list", json!({}), true),
        (
            "tools/call",
            json!({"name": "search_tools", "arguments": {"query": "scene"}}),
            false,
        ),
        ("ping", json!({}), false),
    ] {
        let body = modern_request(&client, &url, method, params).await;
        assert!(body.get("error").is_none(), "{body}");
        let result = &body["result"];
        assert_eq!(result["resultType"], "complete");
        assert!(result["_meta"][SERVER_INFO_META_KEY]["name"].is_string());
        assert!(result["_meta"][SERVER_INFO_META_KEY]["version"].is_string());
        if cacheable {
            assert_eq!(result["ttlMs"], 0);
            assert_eq!(result["cacheScope"], "private");
        } else {
            assert!(result.get("ttlMs").is_none());
            assert!(result.get("cacheScope").is_none());
        }
        if method == "server/discover" {
            assert_eq!(result["supportedVersions"][0], "2026-07-28");
            assert!(result.get("serverInfo").is_none());
            assert!(result.get("protocolVersion").is_none());
        } else if method == "tools/call" {
            assert_ne!(result["isError"], true, "{result}");
            assert!(result["content"].is_array());
        }
    }
    let error = modern_request(&client, &url, "tools/call", json!({"arguments": {}})).await;
    assert_eq!(error["error"]["code"], -32602);
    assert!(error.get("result").is_none());
    assert!(error.get("resultType").is_none());

    // Optional real SDK check against this exact build, never an installed wheel.
    // Install tests/interop/mcp-2026 dependencies, then set DCC_MCP_SDK_SMOKE=1.
    if std::env::var_os("DCC_MCP_SDK_SMOKE").is_some() {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/interop/mcp-2026/client.mjs");
        let output = std::process::Command::new("node")
            .arg(script)
            .arg(&url)
            .output()
            .expect("run pinned official SDK");
        assert!(
            output.status.success(),
            "SDK stdout: {}\nSDK stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }

    handle.shutdown().await;
}
