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
    send(client, url, method, envelope, protocol_header).await
}

/// Send a fully-formed envelope. Modern dispatch requires the request
/// envelope inside `_meta`; a version header alone is never sufficient.
/// `expected` is the transport status the design assigns to the outcome:
/// a modern method-not-found is a 404, business errors stay 200.
async fn send_expecting(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    envelope: Value,
    protocol_header: Option<&str>,
    expected: reqwest::StatusCode,
) -> Value {
    let mut request = client
        .post(url)
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Method", method)
        .json(&envelope);
    if let Some(version) = protocol_header {
        request = request.header("MCP-Protocol-Version", version);
    }
    let response = request.send().await.expect("MCP response");
    let status = response.status();
    let raw = response.text().await.expect("MCP body");
    assert_eq!(
        status, expected,
        "{method} (MCP-Protocol-Version: {protocol_header:?}) -> {status}: {raw}"
    );
    let body: Value = serde_json::from_str(&raw).expect("JSON-RPC body");
    assert_eq!(body["id"], "lifecycle-check");
    body
}

async fn send(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    envelope: Value,
    protocol_header: Option<&str>,
) -> Value {
    send_expecting(
        client,
        url,
        method,
        envelope,
        protocol_header,
        reqwest::StatusCode::OK,
    )
    .await
}

/// A complete final-revision request envelope for `method`.
#[cfg(feature = "mcp-2026-07-28")]
fn modern_envelope(method: &str, mut params: Value) -> Value {
    use dcc_mcp_jsonrpc::{CLIENT_CAPABILITIES_META_KEY, PROTOCOL_VERSION_META_KEY};
    params["_meta"] = json!({
        PROTOCOL_VERSION_META_KEY: "2026-07-28",
        CLIENT_CAPABILITIES_META_KEY: {},
    });
    JsonRpcRequestBuilder::new("lifecycle-check", method)
        .with_params(params)
        .to_value()
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
        use dcc_mcp_jsonrpc::SERVER_INFO_META_KEY;
        let discovery = send(
            &client,
            &url,
            "server/discover",
            modern_envelope("server/discover", json!({})),
            Some("2026-07-28"),
        )
        .await;
        // Discovery carries no body-level protocol identity: the final-revision
        // result reports supportedVersions and stamps server identity under
        // result `_meta` (ADR-034). RC `protocolVersion`/`serverInfo` are gone.
        assert_eq!(
            discovery["result"]["supportedVersions"],
            json!(["2026-07-28"])
        );
        assert!(discovery["result"].get("protocolVersion").is_none());
        assert!(discovery["result"].get("serverInfo").is_none());
        assert!(
            discovery["result"]["_meta"][SERVER_INFO_META_KEY]["name"].is_string(),
            "discovery must carry result _meta server identity: {discovery}"
        );
        // A header alone never upgrades: `initialize` stays a legacy handshake,
        // so the modern registry answers method-not-found (HTTP 404) for its
        // modern form. The request envelope is otherwise well-formed, proving
        // the routing decision, not envelope validation, produced the 404.
        let initialize = send_expecting(
            &client,
            &url,
            "initialize",
            modern_envelope("initialize", initialize_params(Some("2026-07-28"))),
            Some("2026-07-28"),
            reqwest::StatusCode::NOT_FOUND,
        )
        .await;
        assert_eq!(initialize["error"]["code"], -32601);
        assert!(initialize.get("result").is_none());
    }

    handle.shutdown().await;
}
