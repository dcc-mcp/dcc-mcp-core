//! End-to-end tests for the pluggable method router (#492).

use std::sync::Arc;

use axum_test::TestServer;
use serde_json::json;

use super::{make_app_state, make_router};
use crate::handler::{HandlerFuture, MethodHandler};
use crate::handler::{handle_delete, handle_get, handle_post};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Custom handler that echoes its params back as `{ "echoed": ... }`.
struct EchoHandler;
impl MethodHandler for EchoHandler {
    fn handle<'a>(
        &'a self,
        _state: &'a crate::handler::AppState,
        req: &'a JsonRpcRequest,
        _session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        let id = req.id.clone();
        let params = req.params.clone().unwrap_or(json!(null));
        Box::pin(async move { Ok(JsonRpcResponse::success(id, json!({ "echoed": params }))) })
    }
}

/// `dispatch_request` ends up at the wildcard arm for an unknown method,
/// even after the router refactor. Matches the previous wire behaviour
/// (JSON-RPC error code -32601).
#[tokio::test]
async fn unknown_method_returns_method_not_found() {
    let server = TestServer::new(make_router());
    let resp = server
        .post("/mcp")
        .add_header("accept", "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "totally/unknown/method",
            "params": {}
        }))
        .await;
    let body: serde_json::Value = resp.json();
    assert_eq!(body["error"]["code"], -32601);
}

/// Custom methods registered through `AppState::register_method` are
/// dispatched the same way as built-ins.
#[tokio::test]
async fn custom_method_is_dispatched() {
    use axum::{Router, routing};
    let state = make_app_state();
    state.register_method("custom/echo", Arc::new(EchoHandler));
    let router = Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(state);
    let server = TestServer::new(router);
    let resp = server
        .post("/mcp")
        .add_header("accept", "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "custom/echo",
            "params": {"hello": "world"}
        }))
        .await;
    let body: serde_json::Value = resp.json();
    assert_eq!(body["result"]["echoed"], json!({"hello": "world"}));
}

/// `resources/list` returns method-not-found when the capability is
/// disabled — matches the previous fall-through behaviour where the
/// dispatch `match` arm was guarded by `if state.enable_resources`.
#[tokio::test]
async fn resources_disabled_returns_method_not_found() {
    use axum::{Router, routing};
    let mut state = make_app_state();
    state.enable_resources = false;
    let router = Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(state);
    let server = TestServer::new(router);
    let resp = server
        .post("/mcp")
        .add_header("accept", "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "resources/list",
            "params": {}
        }))
        .await;
    let body: serde_json::Value = resp.json();
    assert_eq!(body["error"]["code"], -32601);
}
