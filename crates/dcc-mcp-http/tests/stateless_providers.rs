//! Real HTTP parity through the providers registered by the public server.

#![cfg(feature = "mcp-2026-07-28")]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use dcc_mcp_actions::ToolRegistry;
use dcc_mcp_http::prompts::{PromptArgumentSpec, PromptEntry, PromptSource};
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
        if let Some(name) = params.get("name").or_else(|| params.get("uri")) {
            request = request.header("Mcp-Name", name.as_str().unwrap());
        }
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": {"name": "provider-parity", "version": "1"}
        });
    }
    let envelope = JsonRpcRequestBuilder::new("provider-check", method)
        .with_params(params)
        .to_value();
    let response = request.json(&envelope).send().await.expect("HTTP response");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    if modern {
        assert!(response.headers().get("Mcp-Session-Id").is_none());
    }
    let body: Value = response.json().await.expect("JSON-RPC body");
    assert_eq!(body["id"], "provider-check");
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mounted_stateless_providers_match_legacy_content_and_templates() {
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = 0;
    config.features.enable_resources = true;
    config.features.enable_prompts = true;
    let server = McpHttpServer::new(Arc::new(ToolRegistry::new()), config);
    let scene = json!({"objects": ["Birch"], "dcc": "blender"});
    server.resources().set_scene(scene.clone());
    server.prompts().register_prompt(
        "provider-parity",
        PromptEntry {
            name: "review_asset".into(),
            description: Some("Inspect an asset".into()),
            arguments: vec![PromptArgumentSpec {
                name: "asset".into(),
                description: Some("Exact asset name".into()),
                required: true,
            }],
            template: "Inspect {{asset}} without changing it.".into(),
            source: PromptSource::Explicit,
            skill: "provider-parity".into(),
        },
    );
    let handle = server.start().await.expect("start server");
    let url = format!("http://127.0.0.1:{}/mcp", handle.port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let initialized = request(
        &client,
        &url,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "provider-parity", "version": "1"}
        }),
        false,
    )
    .await;
    assert!(initialized.get("result").is_some());

    for (method, key) in [("resources/list", "resources"), ("prompts/list", "prompts")] {
        let legacy = request(&client, &url, method, json!({}), false).await;
        let modern = request(&client, &url, method, json!({}), true).await;
        let identity = if key == "resources" { "uri" } else { "name" };
        let by_identity = |body: &Value| -> BTreeMap<String, Value> {
            body["result"][key]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| (row[identity].as_str().unwrap().to_owned(), row.clone()))
                .collect()
        };
        assert_eq!(by_identity(&modern), by_identity(&legacy));
        assert!(!modern["result"][key].as_array().unwrap().is_empty());
    }
    for (method, params, key) in [
        (
            "resources/read",
            json!({"uri": "scene://current"}),
            "contents",
        ),
        (
            "prompts/get",
            json!({"name": "review_asset", "arguments": {"asset": "Birch"}}),
            "messages",
        ),
    ] {
        let legacy = request(&client, &url, method, params.clone(), false).await;
        let modern = request(&client, &url, method, params, true).await;
        assert_eq!(modern["result"][key], legacy["result"][key]);
        if key == "contents" {
            let actual: Value =
                serde_json::from_str(modern["result"][key][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(actual, scene);
        } else {
            assert_eq!(
                modern["result"][key][0]["content"]["text"],
                "Inspect Birch without changing it."
            );
        }
    }
    let invalid = request(
        &client,
        &url,
        "resources/list",
        json!({"cursor": "0é0"}),
        true,
    )
    .await;
    assert_eq!(invalid["error"]["code"], -32602);
    handle.shutdown().await;
}
