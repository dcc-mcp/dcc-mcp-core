//! Regression coverage for issue #314 — configurable gateway backend timeout.
//!
//! Before this fix the gateway hard-coded a 10-second per-backend timeout
//! (`BACKEND_TIMEOUT` in `gateway/aggregator.rs`), which short-circuited any
//! legitimately long-running DCC tool (scene import, USD composition,
//! simulation bake) with a transport-level timeout error. The fix promoted
//! the value to [`McpHttpConfig::backend_timeout_ms`], threaded through
//! [`GatewayConfig`] → [`GatewayState::backend_timeout`] → every fan-out
//! helper in the aggregator.
//!
//! These tests assert three things:
//!
//! 1. The config plumbing actually carries the value end-to-end.
//! 2. A gateway configured with a *long* backend timeout tolerates a backend
//!    response that would have tripped the old hard-coded 10-second ceiling.
//! 3. A gateway configured with a *short* backend timeout still fails fast,
//!    proving the value is honoured (not silently ignored).
//!
//! The slow-backend scenarios use a tiny axum mock that sleeps before
//! replying, with timeouts scaled to milliseconds so the suite finishes in
//! well under a second on CI.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{Json, Router, routing::post};
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};

use dcc_mcp_http::config::McpHttpConfig;
use dcc_mcp_http::gateway::aggregator::{aggregate_tools_list, compute_tools_fingerprint};
use dcc_mcp_http::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;

// ── Helpers ────────────────────────────────────────────────────────────────

/// Build a `GatewayState` with the given backend timeout, using a fresh
/// empty `FileRegistry`. Returns the state plus the registry handle so the
/// caller can register mock backends against it.
async fn make_state(
    backend_timeout: Duration,
) -> (GatewayState, Arc<RwLock<FileRegistry>>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(16);

    let state = GatewayState {
        registry: registry.clone(),
        stale_timeout: Duration::from_secs(30),
        backend_timeout,
        async_dispatch_timeout: Duration::from_secs(60),
        wait_terminal_timeout: Duration::from_secs(600),
        server_name: "test".into(),
        server_version: "0.0.0".into(),
        own_host: "127.0.0.1".into(),
        own_port: 0,
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
        subscriber: dcc_mcp_http::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
    };
    (state, registry, dir)
}

/// Spawn a minimal MCP-shaped backend on `127.0.0.1:<random>` that sleeps
/// for `delay` before responding to `tools/list` with a single tool. Returns
/// the bound port so the caller can register a `ServiceEntry` pointing at it.
async fn spawn_slow_backend(delay: Duration) -> u16 {
    async fn handler(
        axum::extract::State(delay): axum::extract::State<Duration>,
        Json(req): Json<Value>,
    ) -> Json<Value> {
        tokio::time::sleep(delay).await;
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [ {
                    "name": "slow_tool",
                    "description": "slow",
                    "inputSchema": { "type": "object" }
                } ]
            }
        }))
    }

    let app = Router::new().route("/mcp", post(handler)).with_state(delay);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    // Give the listener a beat to start accepting before the test dials it.
    tokio::time::sleep(Duration::from_millis(25)).await;
    port
}

async fn register_backend(registry: &Arc<RwLock<FileRegistry>>, port: u16) {
    let reg = registry.read().await;
    let entry = ServiceEntry::new("maya", "127.0.0.1", port);
    reg.register(entry).unwrap();
}

// ── Config plumbing ────────────────────────────────────────────────────────

#[test]
fn mcp_http_config_default_backend_timeout_is_two_minutes() {
    let cfg = McpHttpConfig::new(8765);
    // Default raised from 10 s to 120 s: DCC scene operations (mesh import,
    // simulation bake, complex keyframe setup) routinely take tens of seconds.
    // A 10-second ceiling caused spurious gateway cancellations logged as
    // "tool call cancelled cooperatively" on the DCC backend at exactly 10 s.
    assert_eq!(
        cfg.backend_timeout_ms, 120_000,
        "default backend_timeout_ms should be 120_000 (2 minutes)"
    );
}

#[test]
fn mcp_http_config_with_backend_timeout_ms_is_fluent() {
    let cfg = McpHttpConfig::new(8765).with_backend_timeout_ms(120_000);
    assert_eq!(cfg.backend_timeout_ms, 120_000);
}

// ── Runtime behaviour ──────────────────────────────────────────────────────

/// Issue #314 acceptance: a gateway configured with a long backend timeout
/// must tolerate a backend that takes longer than the legacy 10-second
/// ceiling to respond. We scale the scenario down by 1000× for test speed
/// (ms instead of seconds) while preserving the "timeout > backend delay"
/// invariant the user observes in production.
#[tokio::test]
async fn aggregate_tools_list_respects_long_backend_timeout() {
    // Backend takes ~250ms — would trip any timeout ≤ 200ms, passes at 1s.
    let port = spawn_slow_backend(Duration::from_millis(250)).await;
    let (state, registry, _tmp) = make_state(Duration::from_secs(1)).await;
    register_backend(&registry, port).await;

    let started = Instant::now();
    let result = aggregate_tools_list(&state, None).await;
    let elapsed = started.elapsed();

    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools array");
    // Tier 1 (meta) + Tier 2 (skill mgmt) + 1 backend tool = non-empty, and
    // at least one tool must come from the backend (encoded name contains `.`).
    let has_backend_tool = tools.iter().any(|t| {
        t.get("name")
            .and_then(Value::as_str)
            .map(|n| n.contains("slow_tool"))
            .unwrap_or(false)
    });
    assert!(
        has_backend_tool,
        "backend tool should be present when backend_timeout > backend delay; got tools={tools:#?}"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "aggregation should return as soon as the backend replies, not at the timeout (elapsed={elapsed:?})"
    );
}

/// Complementary case: a gateway with a *shorter* backend timeout than the
/// backend's response time must drop the backend's contribution (fetch_tools
/// swallows the error and returns an empty vec). This proves the timeout
/// value is actually honoured rather than ignored.
#[tokio::test]
async fn aggregate_tools_list_drops_backend_when_timeout_is_exceeded() {
    let port = spawn_slow_backend(Duration::from_millis(400)).await;
    let (state, registry, _tmp) = make_state(Duration::from_millis(50)).await;
    register_backend(&registry, port).await;

    let result = aggregate_tools_list(&state, None).await;
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools array");
    assert!(
        !tools.iter().any(|t| {
            t.get("name")
                .and_then(Value::as_str)
                .map(|n| n.contains("slow_tool"))
                .unwrap_or(false)
        }),
        "backend tool must not appear when backend_timeout < backend delay; got tools={tools:#?}"
    );
}

/// `compute_tools_fingerprint` is the other consumer of the backend timeout
/// (it drives `tools/list_changed` SSE notifications). Regression-guard the
/// parameter plumbing so a future refactor cannot silently drop it.
#[tokio::test]
async fn compute_tools_fingerprint_honours_backend_timeout() {
    let port = spawn_slow_backend(Duration::from_millis(250)).await;
    let (_state, registry, _tmp) = make_state(Duration::from_secs(1)).await;
    register_backend(&registry, port).await;

    let client = reqwest::Client::new();

    let short = compute_tools_fingerprint(
        &registry,
        Duration::from_secs(30),
        &client,
        Duration::from_millis(25),
    )
    .await;
    assert!(
        short.is_empty(),
        "short timeout should yield empty fingerprint (backend dropped); got {short:?}"
    );

    let long = compute_tools_fingerprint(
        &registry,
        Duration::from_secs(30),
        &client,
        Duration::from_secs(1),
    )
    .await;
    assert!(
        long.contains("slow_tool"),
        "long timeout should let the backend's tool into the fingerprint; got {long:?}"
    );
}
