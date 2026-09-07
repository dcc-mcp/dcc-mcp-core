//! Parameter mirrors are checked at the actual HTTP side-effect boundary.

#![cfg(feature = "mcp-2026-07-28")]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use dcc_mcp_actions::{ToolDispatcher, ToolMeta, ToolRegistry};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_jsonrpc::{JsonRpcRequestBuilder, encode_mcp_header_value};
use serde_json::{Value, json};

fn annotated_schema() -> Value {
    json!({"type":"object", "properties": {
        "routing":{"type":"object", "properties": {
            "tenant.name":{"type":"string", "x-mcp-header":"Tenant", "maxLength":100}
        }},
        "count":{"type":"integer", "x-mcp-header":"Count"},
        "active":{"type":"boolean", "x-mcp-header":"Active"},
        "optional":{"anyOf":[{"type":"string"},{"type":"null"}]}
    }})
}

fn request(method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientCapabilities":{}
    });
    JsonRpcRequestBuilder::new("parameter-probe", method)
        .with_params(params)
        .to_value()
}

fn post(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: Value,
) -> reqwest::RequestBuilder {
    let mut builder = client
        .post(url)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", method);
    if let Some(name) = params.get("name").and_then(Value::as_str) {
        builder = builder.header("Mcp-Name", encode_mcp_header_value(name));
    }
    builder.json(&request(method, params))
}

async fn assert_error(response: reqwest::Response, status: u16, code: i64) {
    assert_eq!(response.status().as_u16(), status);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["id"], "parameter-probe");
    assert_eq!(body["error"]["code"], code, "{body}");
    assert!(!body.to_string().contains("secret"), "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parameter_headers_use_source_schemas_and_gate_actual_http_execution() {
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    let calls = Arc::new(AtomicUsize::new(0));
    let invalid = json!({"type":"object", "properties": {
        "value":{"anyOf":[{"type":"string", "x-mcp-header":"Invalid"},{"type":"null"}]}
    }});
    for (name, schema, skill) in [
        ("annotated_echo", annotated_schema(), None),
        (
            "fixture_tools__alias_echo",
            annotated_schema(),
            Some("fixture-tools"),
        ),
        ("invalid_definition", invalid.clone(), None),
        ("search_tools", invalid, None),
        (
            "ordinary",
            json!({"type":"object", "properties": {
                "optional":{"anyOf":[{"type":"string"},{"type":"null"}]}
            }}),
            None,
        ),
    ] {
        registry.register_action(ToolMeta {
            name: name.into(),
            input_schema: schema,
            skill_name: skill.map(str::to_string),
            ..Default::default()
        });
        let calls = calls.clone();
        dispatcher.register_handler(name, move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"accepted":true}))
        });
    }
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = 0;
    config.features.lazy_actions = true;
    let handle = McpHttpServer::new(registry, config)
        .with_dispatcher(dispatcher)
        .start()
        .await
        .unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", handle.port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let list: Value = post(&client, &url, "tools/list", json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tools = list["result"]["tools"].as_array().unwrap();
    let tool = |name: &str| tools.iter().find(|tool| tool["name"] == name).unwrap();
    assert_eq!(tool("annotated_echo")["inputSchema"], annotated_schema());
    assert!(
        tools
            .iter()
            .all(|tool| tool["name"] != "invalid_definition")
    );
    assert_eq!(
        tools
            .iter()
            .filter(|tool| tool["name"] == "search_tools")
            .count(),
        1
    );
    assert!(
        tool("ordinary")["inputSchema"]["properties"]["optional"]
            .get("anyOf")
            .is_none()
    );
    let alias = tools
        .iter()
        .find(|tool| tool["name"].as_str().unwrap().contains("alias_echo"))
        .unwrap()["name"]
        .as_str()
        .unwrap();

    let arguments = json!({"routing":{"tenant.name":"secret-tenant"}, "count":42, "active":true});
    for headers in [
        vec![],
        vec![("Mcp-Param-Tenant", "secret-wrong")],
        vec![("Mcp-Param-Tenant", "=?base64?/w==?=")],
    ] {
        let mut req = post(
            &client,
            &url,
            "tools/call",
            json!({"name":"annotated_echo", "arguments":arguments}),
        );
        for (name, value) in headers {
            req = req.header(name, value);
        }
        assert_error(req.send().await.unwrap(), 400, -32020).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    for count in ["42.0000000000000001", "+42"] {
        assert_error(
            post(
                &client,
                &url,
                "tools/call",
                json!({"name":"annotated_echo", "arguments":arguments}),
            )
            .header("Mcp-Param-Tenant", "secret-tenant")
            .header("Mcp-Param-Count", count)
            .header("Mcp-Param-Active", "true")
            .send()
            .await
            .unwrap(),
            400,
            -32020,
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    assert_error(
        post(
            &client,
            &url,
            "tools/call",
            json!({"name":"annotated_echo", "arguments":arguments}),
        )
        .header("Mcp-Param-Tenant", "secret-tenant")
        .header("Mcp-Param-Tenant", "secret-wrong")
        .header("Mcp-Param-Count", "42")
        .header("Mcp-Param-Active", "true")
        .send()
        .await
        .unwrap(),
        400,
        -32020,
    )
    .await;
    for name in [alias, "fixture_tools__alias_echo", "alias_echo"] {
        assert_error(
            post(
                &client,
                &url,
                "tools/call",
                json!({"name":name, "arguments":arguments}),
            )
            .send()
            .await
            .unwrap(),
            400,
            -32020,
        )
        .await;
    }
    for arguments in [json!({}), json!({"routing":{"tenant.name":null}})] {
        assert_error(
            post(
                &client,
                &url,
                "tools/call",
                json!({"name":"annotated_echo", "arguments":arguments}),
            )
            .header("Mcp-Param-Tenant", "=?base64?ZE==?=")
            .send()
            .await
            .unwrap(),
            400,
            -32020,
        )
        .await;
    }
    assert_error(
        post(
            &client,
            &url,
            "tools/call",
            json!({"name":"annotated_echo", "arguments":{}}),
        )
        .header(
            "Mcp-Param-Tenant",
            reqwest::header::HeaderValue::from_bytes(b"\xff").unwrap(),
        )
        .send()
        .await
        .unwrap(),
        400,
        -32020,
    )
    .await;
    for arguments in [
        json!({"count":9007199254740992_u64}),
        json!({"active":"true"}),
        json!("{}"),
    ] {
        assert_error(
            post(
                &client,
                &url,
                "tools/call",
                json!({"name":"annotated_echo", "arguments":arguments}),
            )
            .send()
            .await
            .unwrap(),
            200,
            -32602,
        )
        .await;
    }
    assert_error(
        post(
            &client,
            &url,
            "tools/call",
            json!({"name":"invalid_definition", "arguments":{}}),
        )
        .send()
        .await
        .unwrap(),
        200,
        -32603,
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    for name in [
        "annotated_echo",
        alias,
        "fixture_tools__alias_echo",
        "alias_echo",
    ] {
        let response = post(
            &client,
            &url,
            "tools/call",
            json!({"name":name, "arguments":arguments}),
        )
        .header("mcp-param-tenant", "secret-tenant")
        .header("Mcp-Param-Count", "0042.0")
        .header("Mcp-Param-Active", "true")
        .header("Mcp-Param-Unrecognized", "=?base64?ZE==?=")
        .send()
        .await
        .unwrap();
        let body: Value = response.json().await.unwrap();
        assert_ne!(body["result"]["isError"], true, "{body}");
        assert_eq!(
            body["result"]["structuredContent"]["accepted"], true,
            "{body}"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    let response: Value = post(
        &client,
        &url,
        "tools/call",
        json!({"name":"search_tools", "arguments":{"query":"scene"}}),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        4,
        "core winner must not call colliding registry action"
    );
    // call_action mirrors its own declared arguments, not an invented inner-tool contract.
    let response: Value = post(
        &client,
        &url,
        "tools/call",
        json!({"name":"call_action", "arguments":{"id":"annotated_echo", "args":arguments}}),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert_eq!(calls.load(Ordering::SeqCst), 5);

    if std::env::var_os("DCC_MCP_SDK_SMOKE").is_some() {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/interop/mcp-2026/param-client.mjs");
        let output = std::process::Command::new("node")
            .arg(script)
            .arg(&url)
            .output()
            .expect("pinned SDK parameter client");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 9);
    }
    // The same registry still exposes its previous compatible legacy schema;
    // legacy clients do not opt in merely by sending an Mcp-Param header.
    let initialize = client
        .post(&url)
        .header("Accept", "application/json, text/event-stream")
        .json(
            &JsonRpcRequestBuilder::new("legacy", "initialize")
                .with_params(json!({
                    "protocolVersion":"2025-06-18", "capabilities":{},
                    "clientInfo":{"name":"legacy-parameter-contract", "version":"1"}
                }))
                .to_value(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(initialize.status().as_u16(), 200);
    let initialized: Value = initialize.json().await.unwrap();
    assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18");
    let legacy_post = |method: &str, params: Value| {
        client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-06-18")
            .json(
                &JsonRpcRequestBuilder::new("legacy", method)
                    .with_params(params)
                    .to_value(),
            )
    };
    let list: Value = legacy_post("tools/list", json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tools = list["result"]["tools"].as_array().unwrap();
    assert!(
        tools
            .iter()
            .any(|tool| tool["name"] == "invalid_definition")
    );
    let schema = &tools
        .iter()
        .find(|tool| tool["name"] == "annotated_echo")
        .unwrap()["inputSchema"];
    assert!(schema["properties"]["optional"].get("anyOf").is_none());
    let before = calls.load(Ordering::SeqCst);
    let response: Value = legacy_post(
        "tools/call",
        json!({"name":"annotated_echo", "arguments":arguments}),
    )
    .header("Mcp-Param-Tenant", "secret-wrong")
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert_eq!(calls.load(Ordering::SeqCst), before + 1);
    handle.shutdown().await;
}
