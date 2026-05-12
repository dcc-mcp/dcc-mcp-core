//! End-to-end acceptance coverage for issue #775.
//!
//! This tracker-level test stitches together the seven operability features in
//! one realistic workstation topology: a gateway with middleware enabled, two
//! DCC HTTP servers (Maya + Photoshop), payload limits at the DCC boundary, an
//! OpenAPI-mounted REST backend, and an agent request carrying trace context
//! through the gateway's real HTTP surface.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{Json, Router, extract::Path, routing::get};
use reqwest::StatusCode;
use serde_json::{Value, json};

use dcc_mcp_actions::{ToolDispatcher, ToolMeta, ToolRegistry};
use dcc_mcp_gateway::middleware::{
    AfterCallMiddleware, BeforeCallMiddleware, CallContext, CallResult, MiddlewareChain,
    MiddlewareError, RedactionMiddleware,
};
use dcc_mcp_gateway::openapi::{OpenApiMount, call_operation};
use dcc_mcp_gateway::{GatewayConfig, GatewayRunner};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer, McpServerHandle};
use dcc_mcp_transport::discovery::types::ServiceEntry;

#[derive(Default, Clone)]
struct MiddlewareSpy {
    before: Arc<Mutex<Vec<Value>>>,
    after: Arc<Mutex<Vec<Value>>>,
}

impl BeforeCallMiddleware for MiddlewareSpy {
    fn before_call<'a>(
        &'a self,
        ctx: &'a mut CallContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), MiddlewareError>> + Send + 'a>>
    {
        let before = self.before.clone();
        let snapshot = json!({
            "method": ctx.method,
            "tool": ctx.tool_slug,
            "session": ctx.session_id,
            "args": ctx.args,
        });
        Box::pin(async move {
            before.lock().unwrap().push(snapshot);
            Ok(())
        })
    }
}

impl AfterCallMiddleware for MiddlewareSpy {
    fn after_call<'a>(
        &'a self,
        ctx: &'a CallContext,
        result: &'a mut CallResult,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), MiddlewareError>> + Send + 'a>>
    {
        let after = self.after.clone();
        let snapshot = json!({
            "tool": ctx.tool_slug,
            "is_error": result.is_error,
            "text_len": result.text.len(),
        });
        Box::pin(async move {
            after.lock().unwrap().push(snapshot);
            Ok(())
        })
    }
}

fn pick_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

async fn spawn_dcc_backend(
    dcc: &'static str,
    tool_name: &'static str,
    max_request_body_bytes: usize,
) -> McpServerHandle {
    let registry = Arc::new(ToolRegistry::new());
    registry.register_action(ToolMeta {
        name: tool_name.into(),
        description: format!("{dcc} operability acceptance tool"),
        category: "issue-775".into(),
        dcc: dcc.into(),
        version: "1.0.0".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "asset_id": {"type": "string"},
                "api_key": {"type": "string"}
            }
        }),
        ..Default::default()
    });

    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    dispatcher.register_handler(tool_name, move |params| {
        Ok(json!({
            "dcc": dcc,
            "tool": tool_name,
            "received_args": params,
        }))
    });

    let mut cfg = McpHttpConfig::default()
        .with_port(0)
        .with_name(format!("{dcc}-issue-775"))
        .with_dcc_type(dcc);
    cfg.queue = cfg
        .queue
        .with_max_request_body_bytes(max_request_body_bytes);

    McpHttpServer::new(registry, cfg)
        .with_dispatcher(dispatcher)
        .start()
        .await
        .expect("DCC backend must start")
}

async fn register_backend(
    registry_dir: &std::path::Path,
    dcc: &str,
    port: u16,
) -> dcc_mcp_transport::discovery::file_registry::FileRegistry {
    let reg = dcc_mcp_transport::discovery::file_registry::FileRegistry::new(registry_dir).unwrap();
    reg.register(ServiceEntry::new(dcc, "127.0.0.1", port))
        .unwrap();
    reg
}

async fn start_gateway(
    registry_dir: &std::path::Path,
    spy: MiddlewareSpy,
) -> (dcc_mcp_gateway::GatewayHandle, String) {
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: pick_free_port(),
        heartbeat_secs: 1,
        registry_dir: Some(registry_dir.to_path_buf()),
        middleware_chain: MiddlewareChain::new()
            .with_before(Arc::new(RedactionMiddleware::new(vec!["api_key"])))
            .with_before(Arc::new(spy.clone()))
            .with_after(Arc::new(spy)),
        admin_enabled: false,
        ..GatewayConfig::default()
    };
    let mcp_url = format!("http://127.0.0.1:{}/mcp", cfg.gateway_port);
    let runner = GatewayRunner::new(cfg).expect("GatewayRunner::new");
    let handle = runner
        .start(ServiceEntry::new("maya", "127.0.0.1", 0), None)
        .await
        .expect("gateway must start");
    assert!(handle.is_gateway);
    (handle, mcp_url)
}

async fn start_openapi_backend() -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new().route(
        "/assets/{id}",
        get(|Path(id): Path<String>| async move {
            Json(json!({
                "asset_id": id,
                "source": "openapi-mounted-backend",
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://127.0.0.1:{}", addr.port());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base_url, handle)
}

fn openapi_mount(base_url: &str) -> OpenApiMount {
    OpenApiMount::from_spec_json(json!({
        "openapi": "3.0.0",
        "paths": {
            "/assets/{id}": {
                "get": {
                    "operationId": "getAsset",
                    "summary": "Resolve an asset for DCC export",
                    "parameters": [
                        {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}
                    ]
                }
            }
        }
    }))
    .base_url(base_url)
    .tool_prefix("studio")
}

async fn gateway_tool_call(
    client: &reqwest::Client,
    mcp_url: &str,
    id: &str,
    tool: &str,
    arguments: Value,
) -> Value {
    let resp = client
        .post(mcp_url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("mcp-session-id", "issue-775-session")
        .header(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        )
        .json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": tool, "arguments": arguments}
        }))
        .send()
        .await
        .expect("gateway request must complete");
    assert_eq!(resp.status(), StatusCode::OK);
    resp.json::<Value>().await.expect("JSON-RPC response")
}

fn tool_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("tool result text")
}

async fn find_tool_slug(client: &reqwest::Client, mcp_url: &str, query: &str, dcc: &str) -> String {
    let search = gateway_tool_call(
        client,
        mcp_url,
        &format!("search-{dcc}"),
        "search_tools",
        json!({"query": query, "dcc_type": dcc}),
    )
    .await;
    let payload: Value = serde_json::from_str(tool_text(&search)).expect("search_tools JSON text");
    payload["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|hit| hit["tool_slug"].as_str())
        .unwrap_or_else(|| panic!("missing {dcc} slug in {payload}"))
        .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn issue_775_gateway_operability_acceptance_maya_photoshop() {
    let registry_dir = tempfile::tempdir().unwrap();
    let maya = spawn_dcc_backend("maya", "create_sphere", 16 * 1024).await;
    let photoshop = spawn_dcc_backend("photoshop", "export_layers", 512).await;
    let _maya_row = register_backend(registry_dir.path(), "maya", maya.port).await;
    let _photoshop_row = register_backend(registry_dir.path(), "photoshop", photoshop.port).await;

    let spy = MiddlewareSpy::default();
    let (gateway, mcp_url) = start_gateway(registry_dir.path(), spy.clone()).await;
    tokio::time::sleep(Duration::from_millis(80)).await;

    let client = reqwest::Client::new();

    // #773: resolve external REST data through the OpenAPI-to-MCP mount helper
    // and feed that resolved payload into a DCC gateway call.
    let (openapi_base, _openapi_task) = start_openapi_backend().await;
    let mount = openapi_mount(&openapi_base);
    let tool = mount
        .find_operation("studio__getAsset")
        .expect("OpenAPI operation exposed as MCP tool");
    let resolved_asset = call_operation(&mount, tool, json!({"id": "hero-texture-pack"}), &client)
        .await
        .expect("OpenAPI-mounted backend call succeeds");
    assert_eq!(resolved_asset["source"], "openapi-mounted-backend");

    // Gateway + Maya + Photoshop: discover both DCC tools through the same
    // gateway, proving the cross-DCC capability index is populated.
    let maya_slug = find_tool_slug(&client, &mcp_url, "sphere", "maya").await;
    let photoshop_slug = find_tool_slug(&client, &mcp_url, "layers", "photoshop").await;
    assert!(maya_slug.starts_with("maya."), "maya slug: {maya_slug}");
    assert!(
        photoshop_slug.starts_with("photoshop."),
        "photoshop slug: {photoshop_slug}"
    );

    // #768 + #770: drive an agent-like HTTP request with W3C trace context
    // through the real gateway router. The TraceLayer handles the request span;
    // the middleware spy proves the BeforeCall/AfterCall chain wrapped dispatch.
    let call = gateway_tool_call(
        &client,
        &mcp_url,
        "call-photoshop",
        "call_tool",
        json!({
            "tool_slug": photoshop_slug,
            "arguments": {
                "asset_id": resolved_asset["asset_id"],
                "api_key": "secret-from-agent"
            }
        }),
    )
    .await;
    let backend_envelope: Value = serde_json::from_str(tool_text(&call)).expect("backend envelope");
    let backend_payload = backend_envelope["output"].clone();
    assert_eq!(backend_payload["dcc"], "photoshop");
    assert_eq!(
        backend_payload["received_args"]["asset_id"],
        "hero-texture-pack"
    );
    assert_eq!(backend_payload["received_args"]["api_key"], "[REDACTED]");

    assert!(
        spy.before
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry["tool"] == photoshop_slug
                && entry["session"] == "issue-775-session"
                && entry["args"]["arguments"]["api_key"] == "[REDACTED]"),
        "middleware before-call spy did not observe redacted gateway call",
    );
    assert!(
        spy.after
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry["tool"] == photoshop_slug && entry["is_error"] == false),
        "middleware after-call spy did not observe successful gateway call",
    );

    // The Maya backend stays independently reachable through the same gateway,
    // guarding against Photoshop-specific assumptions in the tracker E2E.
    let maya_call = gateway_tool_call(
        &client,
        &mcp_url,
        "call-maya",
        "call_tool",
        json!({"tool_slug": maya_slug, "arguments": {"asset_id": "maya-shot-010"}}),
    )
    .await;
    let maya_envelope: Value = serde_json::from_str(tool_text(&maya_call)).expect("maya envelope");
    let maya_payload = maya_envelope["output"].clone();
    assert_eq!(maya_payload["dcc"], "maya");

    // #771: the Photoshop DCC server enforces payload limits at the HTTP
    // boundary; oversized agent input is rejected before a skill handler runs.
    let oversized = client
        .post(format!("http://127.0.0.1:{}/mcp", photoshop.port))
        .header("content-type", "application/json")
        .body(
            json!({
                "jsonrpc": "2.0",
                "id": "too-large",
                "method": "tools/call",
                "params": {"name": "export_layers", "arguments": {"blob": "x".repeat(4096)}}
            })
            .to_string(),
        )
        .send()
        .await
        .expect("oversized request completes with HTTP error");
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);

    drop(gateway);
    maya.shutdown().await;
    photoshop.shutdown().await;
}
