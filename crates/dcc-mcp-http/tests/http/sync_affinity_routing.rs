//! Integration tests for sync `tools/call` affinity-aware routing (issue #716).
//!
//! The sync path used to branch on "is a DccExecutor wired?" alone, so every
//! embedded-DCC backend ran `affinity: any` tools through the UI dispatcher
//! where they fought `affinity: main` tools for the same single-slot queue.
//!
//! These tests exercise the full HTTP surface with a `DeferredExecutor` whose
//! pump is **never called**:
//!
//! * an `affinity: any` sync `tools/call` must still complete (it must route
//!   to `spawn_blocking`, bypassing the executor queue)
//! * an `affinity: main` sync `tools/call` must NOT complete while the pump
//!   is starved — demonstrating that `main` tools still go through the
//!   executor and that the routing really does differ between the two
//!   affinities

use std::sync::Arc;
use std::time::{Duration, Instant};

use dcc_mcp_actions::{ActionDispatcher, ActionMeta, ActionRegistry};
use dcc_mcp_http::{DeferredExecutor, McpHttpConfig, McpHttpServer};
use dcc_mcp_models::ThreadAffinity;

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

/// Build a registry with two tools:
/// - `any_tool`: declared `ThreadAffinity::Any`, returns `{"ok": true}`
/// - `main_tool`: declared `ThreadAffinity::Main`, returns `{"ok": true}`
fn make_registry_with_both_affinities() -> (Arc<ActionRegistry>, Arc<ActionDispatcher>) {
    let registry = Arc::new(ActionRegistry::new());
    registry.register_action(ActionMeta {
        name: "any_tool".into(),
        description: "pure compute — no DCC state, any thread is fine".into(),
        category: "test".into(),
        version: "1.0.0".into(),
        thread_affinity: ThreadAffinity::Any,
        ..Default::default()
    });
    registry.register_action(ActionMeta {
        name: "main_tool".into(),
        description: "mutates scene — must run on DCC main thread".into(),
        category: "test".into(),
        version: "1.0.0".into(),
        thread_affinity: ThreadAffinity::Main,
        ..Default::default()
    });
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    dispatcher.register_handler("any_tool", |_args| Ok(serde_json::json!({"ok": true})));
    dispatcher.register_handler("main_tool", |_args| Ok(serde_json::json!({"ok": true})));
    (registry, dispatcher)
}

async fn call_tool(client: &reqwest::Client, addr: &str, tool: &str) -> reqwest::Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": {} }
    });
    client
        .post(format!("http://{addr}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .json(&body)
        .send()
        .await
        .expect("POST /mcp")
}

/// Acceptance test from the issue: with a DeferredExecutor whose pump is
/// never drained, a sync `tools/call` to an `affinity: any` tool must still
/// complete — because the router now bypasses the executor for `Any`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sync_any_affinity_bypasses_blocked_ui_dispatcher() {
    let (registry, dispatcher) = make_registry_with_both_affinities();

    // Build a DeferredExecutor but keep ownership of it so nothing ever
    // calls poll_pending(): the UI queue is effectively stuck.
    let executor = DeferredExecutor::new(16);
    let handle = executor.handle();

    let cfg = McpHttpConfig::new(0).with_name("sync-affinity-any");
    let server = McpHttpServer::new(registry, cfg)
        .with_dispatcher(dispatcher)
        .with_executor(handle);

    let server_handle = server.start().await.expect("server starts");
    let addr = server_handle.bind_addr.clone();
    assert!(wait_reachable(&addr).await);

    let client = reqwest::Client::new();

    // The executor is wired but its pump never runs. If the sync path
    // blindly sends every call through the executor, this request would
    // hang. With #716, `any_tool` must route to spawn_blocking and return
    // quickly.
    let fut = call_tool(&client, &addr, "any_tool");
    let resp = tokio::time::timeout(Duration::from_secs(3), fut)
        .await
        .expect("sync any_tool must not block on starved UI dispatcher")
        .error_for_status()
        .expect("2xx");
    let payload: serde_json::Value = resp.json().await.expect("valid JSON-RPC");
    let is_error = payload
        .pointer("/result/isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(
        !is_error,
        "any-affinity tool must succeed off the UI dispatcher, got {payload:#}"
    );

    // Keep executor alive for the duration of the test; dropping the
    // DeferredExecutor closes the mpsc which would also unblock main calls.
    drop(executor);
    server_handle.shutdown().await;
}

/// Regression guard: `affinity: main` sync calls still route through the
/// executor. Without a pump they never complete within the test window —
/// which is exactly the behaviour that would fail the test above for
/// `any_tool` if #716 had not been fixed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sync_main_affinity_still_routes_through_executor() {
    let (registry, dispatcher) = make_registry_with_both_affinities();

    let executor = DeferredExecutor::new(16);
    let handle = executor.handle();

    let cfg = McpHttpConfig::new(0).with_name("sync-affinity-main");
    let server = McpHttpServer::new(registry, cfg)
        .with_dispatcher(dispatcher)
        .with_executor(handle);

    let server_handle = server.start().await.expect("server starts");
    let addr = server_handle.bind_addr.clone();
    assert!(wait_reachable(&addr).await);

    let client = reqwest::Client::new();

    // With no pump, `main_tool` cannot progress — the request times out.
    // (A generous 500 ms is plenty; we want to observe the *lack* of a
    // response, not measure precise timing.)
    let fut = call_tool(&client, &addr, "main_tool");
    let outcome = tokio::time::timeout(Duration::from_millis(500), fut).await;
    assert!(
        outcome.is_err(),
        "main-affinity sync call must NOT complete while the UI pump is starved; \
         got {outcome:?} — did #716 accidentally route `Main` off the executor?"
    );

    drop(executor);
    server_handle.shutdown().await;
}

/// Sanity: when no DeferredExecutor is wired at all, both affinities go
/// through `spawn_blocking` and succeed — this has always been the case and
/// #716 must not regress it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sync_calls_succeed_without_executor_for_both_affinities() {
    let (registry, dispatcher) = make_registry_with_both_affinities();

    let cfg = McpHttpConfig::new(0).with_name("sync-affinity-no-executor");
    let server = McpHttpServer::new(registry, cfg).with_dispatcher(dispatcher);

    let server_handle = server.start().await.expect("server starts");
    let addr = server_handle.bind_addr.clone();
    assert!(wait_reachable(&addr).await);

    let client = reqwest::Client::new();

    for tool in ["any_tool", "main_tool"] {
        let fut = call_tool(&client, &addr, tool);
        let resp = tokio::time::timeout(Duration::from_secs(3), fut)
            .await
            .unwrap_or_else(|_| panic!("{tool} timed out without an executor"))
            .error_for_status()
            .expect("2xx");
        let payload: serde_json::Value = resp.json().await.unwrap();
        let is_error = payload
            .pointer("/result/isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(
            !is_error,
            "{tool} must succeed with no executor, got {payload:#}"
        );
    }

    server_handle.shutdown().await;
}
