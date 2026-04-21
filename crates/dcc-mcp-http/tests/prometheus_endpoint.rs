//! Integration tests for the Prometheus `/metrics` endpoint (issue #331).
//!
//! These exercise the full HTTP surface: starting a real server, issuing
//! MCP tool calls over HTTP, and scraping `/metrics`. The tests only run
//! when the `prometheus` Cargo feature is enabled on dcc-mcp-http.

#![cfg(feature = "prometheus")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use dcc_mcp_actions::{ActionDispatcher, ActionMeta, ActionRegistry};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};

/// Wait until a TCP connect to the handle's bind address succeeds, or
/// the deadline elapses. Returns `true` on success.
async fn wait_reachable(addr: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

/// Helper: build a registry with one tool + handler, returning a fully
/// wired server ready to `.start()`.
fn make_server(config: McpHttpConfig) -> McpHttpServer {
    let registry = Arc::new(ActionRegistry::new());
    registry.register_action(ActionMeta {
        name: "ping".into(),
        description: "test ping tool".into(),
        category: "test".into(),
        version: "1.0.0".into(),
        ..Default::default()
    });
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    dispatcher.register_handler("ping", |_args| Ok(serde_json::json!({"pong": true})));
    McpHttpServer::new(registry, config).with_dispatcher(dispatcher)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn metrics_endpoint_exposes_prometheus_payload() {
    let cfg = McpHttpConfig::new(0).with_name("prom-basic");
    let mut cfg = cfg;
    cfg.enable_prometheus = true;

    let server = make_server(cfg);
    let handle = server.start().await.expect("server must start");
    let addr = handle.bind_addr.clone();
    assert!(wait_reachable(&addr).await, "server unreachable");

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/metrics"))
        .send()
        .await
        .expect("metrics request must succeed");
    assert_eq!(resp.status(), 200, "GET /metrics must return 200");
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ctype.starts_with("text/plain"),
        "content-type must be text/plain (got `{ctype}`)"
    );
    assert!(ctype.contains("version=0.0.4"));
    let body = resp.text().await.unwrap();

    // The build_info series is always emitted, regardless of traffic.
    assert!(body.contains("dcc_mcp_build_info"));
    assert!(body.contains("dcc_mcp_active_sessions"));
    assert!(body.contains("dcc_mcp_registered_tools"));

    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tool_calls_increment_counter() {
    let mut cfg = McpHttpConfig::new(0).with_name("prom-counter");
    cfg.enable_prometheus = true;

    let server = make_server(cfg);
    let handle = server.start().await.expect("server must start");
    let addr = handle.bind_addr.clone();
    assert!(wait_reachable(&addr).await);

    let client = reqwest::Client::new();

    // Issue a handful of tools/call requests to warm the counter.
    for i in 0..3 {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": i,
            "method": "tools/call",
            "params": { "name": "ping", "arguments": {} }
        });
        let resp = client
            .post(format!("http://{addr}/mcp"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .json(&body)
            .send()
            .await
            .expect("tools/call must succeed");
        assert!(
            resp.status().is_success(),
            "tools/call returned {}",
            resp.status()
        );
    }

    let metrics = client
        .get(format!("http://{addr}/metrics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // At least one success row for the ping tool must be present with
    // count >= 3. We assert >=3 (not exactly 3) so the test survives
    // any incidental tool calls emitted by the handler internals.
    assert!(
        metrics.contains(r#"dcc_mcp_tool_calls_total{status="success",tool="ping"}"#),
        "metrics missing ping success counter:\n{metrics}"
    );
    let count_line = metrics
        .lines()
        .find(|l| l.contains(r#"dcc_mcp_tool_calls_total{status="success",tool="ping"}"#))
        .expect("counter line present");
    let value: u64 = count_line
        .rsplit(' ')
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(value >= 3, "expected >=3 ping calls, got {value}");

    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn basic_auth_rejects_without_header() {
    let mut cfg = McpHttpConfig::new(0).with_name("prom-auth");
    cfg.enable_prometheus = true;
    cfg.prometheus_basic_auth = Some(("admin".to_string(), "s3cret".to_string()));

    let server = make_server(cfg);
    let handle = server.start().await.expect("server must start");
    let addr = handle.bind_addr.clone();
    assert!(wait_reachable(&addr).await);

    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://{addr}/metrics"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "missing auth header must yield 401");
    let www_auth = resp
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(www_auth.contains("Basic"));

    // Wrong credentials → 401.
    let bad = base64::engine::general_purpose::STANDARD.encode("admin:nope");
    let resp = client
        .get(format!("http://{addr}/metrics"))
        .header(reqwest::header::AUTHORIZATION, format!("Basic {bad}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "wrong password must yield 401");

    // Correct credentials → 200.
    let good = base64::engine::general_purpose::STANDARD.encode("admin:s3cret");
    let resp = client
        .get(format!("http://{addr}/metrics"))
        .header(reqwest::header::AUTHORIZATION, format!("Basic {good}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "valid auth must yield 200");
    let body = resp.text().await.unwrap();
    assert!(body.contains("dcc_mcp_build_info"));

    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn metrics_endpoint_absent_when_flag_is_off() {
    let cfg = McpHttpConfig::new(0).with_name("prom-off");
    // enable_prometheus is false by default.
    let server = make_server(cfg);
    let handle = server.start().await.expect("server must start");
    let addr = handle.bind_addr.clone();
    assert!(wait_reachable(&addr).await);

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/metrics"))
        .send()
        .await
        .unwrap();
    // Router does not mount /metrics, so Axum returns 404.
    assert_eq!(
        resp.status(),
        404,
        "metrics endpoint must not exist when enable_prometheus=false"
    );

    handle.shutdown().await;
}
