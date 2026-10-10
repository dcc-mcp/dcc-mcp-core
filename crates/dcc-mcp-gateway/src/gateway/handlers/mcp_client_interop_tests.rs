// Opt-in interoperability fixture: real Core HTTP handlers, no DCC host.
use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires task-local pinned Node MCP clients; see docs/guide/mcp-cli-clients.md"]
async fn external_mcp_clients_consume_core_protocol() {
    use axum::{
        Router,
        extract::{Path, Query},
        routing::{get, post},
    };
    let backend = Router::new()
        .route("/health", get(|| async { Json(json!({"ok":true})) }))
        .route("/v1/tools", get(|| async { Json(json!({"tools":[], "total":0})) }))
        .route("/v1/prompts", get(|| async { Json(json!({"prompts":[{"name":"tree_recipe", "description":"Fixture recipe", "arguments":[{"name":"seed", "required":true},{"name":"label", "required":true}]}], "total":1})) }))
        .route("/v1/prompts/{name}", get(|Path(name): Path<String>, Query(query): Query<HashMap<String,String>>| async move {
            if name != "tree_recipe" {
                return (StatusCode::NOT_FOUND, Json(json!({"error":"unknown fixture prompt"})));
            }
            let Some(raw) = query.get("args") else {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":"prompt arguments required"})));
            };
            let arguments: Value = serde_json::from_str(raw).unwrap();
            (StatusCode::OK, Json(json!({"description":"Fixture only", "messages":[{"role":"user", "content":{"type":"text", "text":serde_json::to_string(&arguments).unwrap()}}]})))
        }));
    let backend_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_port = backend_listener.local_addr().unwrap().port();
    let backend_task =
        tokio::spawn(async move { axum::serve(backend_listener, backend).await.unwrap() });
    let registry_dir = tempfile::tempdir().unwrap();
    let mut state = test_gateway_state();
    state.registry = Arc::new(FileRegistry::new(registry_dir.path()).unwrap());
    state
        .registry
        .register(dcc_mcp_transport::discovery::types::ServiceEntry::new(
            "blender",
            "127.0.0.1",
            backend_port,
        ))
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    state.own_port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}/mcp", state.own_port);
    let app = Router::new()
        .route(
            "/mcp",
            post(handle_gateway_mcp).get(super::super::super::handle_gateway_get),
        )
        .route("/old/mcp", post(old_gateway_fixture))
        .with_state(state);
    let gateway_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client_dir =
        std::env::var("DCC_MCP_INTEROP_CLIENT_DIR").expect("pinned task-local clients directory");
    let output_dir =
        std::env::var("DCC_MCP_INTEROP_OUTPUT_DIR").expect("task-local evidence directory");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/test_mcp_client_interop.mjs");
    let outcome = tokio::task::spawn_blocking(move || {
        std::process::Command::new("node")
            .arg(script)
            .arg(url)
            .arg(client_dir)
            .arg(output_dir)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    gateway_task.abort();
    backend_task.abort();
    println!("{}", String::from_utf8_lossy(&outcome.stdout));
    assert!(
        outcome.status.success(),
        "{}",
        String::from_utf8_lossy(&outcome.stderr)
    );
}

// An older readable guide must not be mistaken for the versioned policy.
async fn old_gateway_fixture(Json(request): Json<Value>) -> Response {
    let Some(id) = request.get("id") else {
        return StatusCode::ACCEPTED.into_response();
    };
    let result = match request["method"].as_str().unwrap_or_default() {
        "initialize" => json!({"protocolVersion":"2025-03-26", "capabilities":{"resources":{},"tools":{}}, "serverInfo":{"name":"old-gateway-fixture","version":"0"}, "instructions":"Read gateway://docs/agent-workflows"}),
        "tools/list" => json!({"tools":[]}),
        "resources/read" => json!({"contents":[{"uri":"gateway://docs/agent-workflows", "mimeType":"application/json", "text":"{\"format\":\"markdown\",\"document\":\"# Old guide\\nUse tools\"}"}]}),
        "ping" => json!({}),
        method => return Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("Method not found: {method}")}})).into_response(),
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
}
