//! Real HTTP ingress fixtures and the documented configurable body budget.

use dcc_mcp_actions::ToolRegistry;
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use dcc_mcp_jsonrpc::JsonRpcRequestBuilder;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

fn legacy_body(bytes: usize) -> String {
    let mut body = JsonRpcRequestBuilder::new("legacy-limit", "initialize")
        .with_params(json!({"protocolVersion":"2025-06-18", "capabilities":{},
            "clientInfo":{"name":"bounded-test","version":"1"}, "padding":""}))
        .to_value();
    let overhead = body.to_string().len();
    body["params"]["padding"] = json!("x".repeat(bytes.checked_sub(overhead).unwrap()));
    let encoded = body.to_string();
    assert_eq!(encoded.len(), bytes);
    encoded
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_request_body_limit_covers_mcp_and_honors_larger_values() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    for limit in [1024, 16 * 1024 * 1024 + 1024] {
        let mut config = McpHttpConfig::default();
        config.server.port = 0;
        config.gateway.gateway_port = 0;
        config.queue.max_request_body_bytes = limit;
        let handle = McpHttpServer::new(Arc::new(ToolRegistry::new()), config)
            .start()
            .await
            .unwrap();
        let url = format!("http://127.0.0.1:{}/mcp", handle.port);
        let sizes = if limit == 1024 {
            vec![limit - 1, limit, limit + 1]
        } else {
            vec![16 * 1024 * 1024 + 1]
        };
        for bytes in sizes {
            let response = client
                .post(&url)
                .header("Accept", "application/json, text/event-stream")
                .header("Content-Type", "application/json")
                .header("MCP-Protocol-Version", "2025-06-18")
                .body(legacy_body(bytes))
                .send()
                .await
                .unwrap();
            if bytes > limit {
                assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
            } else {
                assert_eq!(response.status(), reqwest::StatusCode::OK);
                let body: Value = response.json().await.unwrap();
                assert_eq!(body["id"], "legacy-limit");
                assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
                assert!(body["result"].get("resultType").is_none());
            }
        }
        handle.shutdown().await;
    }
}

#[cfg(feature = "mcp-2026-07-28")]
mod modern {
    use super::*;
    use dcc_mcp_jsonrpc::{
        CLIENT_CAPABILITIES_META_KEY as CAPS, PROTOCOL_VERSION_META_KEY as VERSION,
        StatelessRequestMeta,
    };

    fn request(method: &str, params: Value) -> Value {
        let meta =
            StatelessRequestMeta::parse(Some(&json!({VERSION:"2026-07-28",CAPS:{}}))).unwrap();
        JsonRpcRequestBuilder::new("positive", method)
            .with_params(params)
            .with_stateless_metadata(&meta)
            .unwrap()
            .to_value()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn final_request_boundaries_match_pinned_negative_cases_and_sdk() {
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
        let cases: Vec<Value> = serde_json::from_str(include_str!(
            "../../dcc-mcp-jsonrpc/tests/fixtures/modern_request_cases.json"
        ))
        .unwrap();
        for case in cases {
            // Routing-only fixtures are also asserted by the pure classifier
            // and the official SDK's public isLegacyRequest predicate.
            if case["route"] == "legacy" {
                continue;
            }
            let mut req = client
                .post(&url)
                .header("Accept", "application/json, text/event-stream")
                .json(&case["body"]);
            for (name, value) in case["headers"].as_object().unwrap() {
                req = req.header(name.as_str(), value.as_str().unwrap());
            }
            let response = req.send().await.unwrap();
            assert_eq!(
                response.status().as_u16(),
                case["status"].as_u64().unwrap_or(400) as u16,
                "{}",
                case["name"]
            );
            assert!(response.headers().get("Mcp-Session-Id").is_none());
            let body: Value = response.json().await.unwrap();
            assert_eq!(
                body["error"]["code"], case["code"],
                "{}: {body}",
                case["name"]
            );
            if case["code"] == -32600 {
                assert!(body["id"].is_null());
            } else {
                assert_eq!(body["id"], case["body"]["id"]);
            }
            if case["code"] == -32022 {
                assert_eq!(
                    body["error"]["data"],
                    json!({"supported":["2026-07-28"],"requested":"2099-01-01"})
                );
            }
        }

        for (method, params, name) in [
            ("server/discover", json!({}), None),
            ("tools/list", json!({}), None),
            (
                "tools/call",
                json!({"name":"search_tools","arguments":{"query":"scene"}}),
                Some("search_tools"),
            ),
        ] {
            let mut req = client
                .post(&url)
                .header("Accept", "application/json, text/event-stream")
                .header("MCP-Protocol-Version", "2026-07-28")
                .header("Mcp-Method", method)
                .json(&request(method, params));
            if let Some(name) = name {
                req = req.header("Mcp-Name", name);
            }
            let response = req.send().await.unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let body: Value = response.json().await.unwrap();
            assert!(body.get("error").is_none(), "{body}");
            assert_eq!(body["result"]["resultType"], "complete");
        }

        // Business invalid-params remains in-band, not a blanket HTTP 400.
        let response = client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2026-07-28")
            .header("Mcp-Method", "tools/call")
            .json(&request("tools/call", json!({})))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["code"],
            -32602
        );

        // Duplicate standard headers must not hide a disagreeing second value.
        let response = client
            .post(&url)
            .header("MCP-Protocol-Version", "2026-07-28")
            .header("Mcp-Method", "tools/list")
            .header("Mcp-Method", "tools/call")
            .json(&request("tools/list", json!({})))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["code"],
            -32020
        );

        if std::env::var_os("DCC_MCP_SDK_SMOKE").is_some() {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/interop/mcp-2026");
            for script in ["client.mjs", "request-oracle.mjs"] {
                let output = std::process::Command::new("node")
                    .arg(root.join(script))
                    .arg(&url)
                    .output()
                    .expect("pinned SDK check");
                assert!(
                    output.status.success(),
                    "{script}: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                println!("{}", String::from_utf8_lossy(&output.stdout));
            }
        }
        handle.shutdown().await;
    }
}
