//! Regression coverage for gateway tool-exposure mode (`Slim` / `Rest`).
//!
//! The gateway must **never** publish Tier 3 backend tools — `Slim` and
//! `Rest` both keep the visible surface bounded to meta-tools +
//! skill-management layer regardless of how many live backends are
//! registered.
//!
//! `Full` and `Both` variants have been removed (issue #674); the
//! correct and unique behaviour is tested here.
//!
//! The mode is also advertised through `diagnostics__tool_metrics` so
//! operators can verify a running gateway without reading process args.
//!
//! The tests use a real in-process axum backend registered through the
//! same `FileRegistry` that the gateway aggregator consults.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};

use dcc_mcp_http::gateway::GatewayToolExposure;
use dcc_mcp_http::gateway::aggregator::aggregate_tools_list;
use dcc_mcp_http::gateway::sse_subscriber::SubscriberManager;
use dcc_mcp_http::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;

// ── Fixture helpers ────────────────────────────────────────────────────────

/// Build a `GatewayState` pinned to a specific [`GatewayToolExposure`].
///
/// The test intentionally pins every `Duration` to a short value so an
/// unreachable backend cannot stall the suite beyond a handful of
/// seconds even if assertions fail and the test harness keeps retrying.
fn make_state(
    registry: Arc<RwLock<FileRegistry>>,
    tool_exposure: GatewayToolExposure,
) -> GatewayState {
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(16);
    GatewayState {
        registry,
        stale_timeout: Duration::from_secs(30),
        backend_timeout: Duration::from_secs(2),
        async_dispatch_timeout: Duration::from_secs(2),
        wait_terminal_timeout: Duration::from_secs(2),
        server_name: "test-652".into(),
        server_version: "0.0.0-test".into(),
        own_host: "127.0.0.1".into(),
        own_port: 0,
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
        subscriber: SubscriberManager::default(),
        allow_unknown_tools: false,
        adapter_version: None,
        adapter_dcc: None,
        tool_exposure,
        cursor_safe_tool_names: true,
        capability_index: std::sync::Arc::new(
            dcc_mcp_http::gateway::capability::CapabilityIndex::new(),
        ),
    }
}

/// Spawn a minimal MCP-compatible backend on an OS-assigned port.
///
/// `tools/list` always returns a single `backend_probe` tool so that
/// fan-out in `Full` mode is observable (the tool name must survive the
/// `{id8}.{tool}` encoding into the gateway output), while the bare
/// alias branch — active only when exactly one backend is live — can be
/// asserted independently.
async fn spawn_backend_advertising(tool_name: &'static str) -> u16 {
    async fn handler(
        axum::extract::State(tool_name): axum::extract::State<&'static str>,
        Json(req): Json<Value>,
    ) -> Json<Value> {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        if method == "tools/list" {
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": tool_name,
                        "description": format!("probe tool exposed by the fake backend for {tool_name}"),
                        "inputSchema": {"type": "object", "properties": {}}
                    }]
                }
            }))
        } else {
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("unknown method: {method}")}
            }))
        }
    }

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/mcp", post(handler))
        .with_state(tool_name);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    // Give the OS a moment to put the socket in the listening state so
    // the very next fan-out request does not race with accept() setup.
    tokio::time::sleep(Duration::from_millis(30)).await;
    port
}

async fn register_maya_backend(registry: &Arc<RwLock<FileRegistry>>, port: u16) {
    let entry = ServiceEntry::new("maya", "127.0.0.1", port);
    let reg = registry.read().await;
    reg.register(entry).unwrap();
}

fn tool_names(result: &Value) -> Vec<String> {
    result
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

// ── Slim / Rest: bounded surface regardless of live backends ────────────────

/// Issue #652 acceptance: a gateway in `Slim` mode must not expose any
/// backend-provided tool via `tools/list`, even when multiple live
/// backends are registered. This is the primary guarantee the REST
/// capability redesign depends on (#657).
#[tokio::test]
async fn slim_mode_hides_backend_tools_even_with_many_backends() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Slim);

    // Simulate the multi-instance blow-up scenario from the issue: two
    // live backends, each advertising a distinct tool. In `Full` mode
    // both would surface (plus potential aliases); `Slim` must drop
    // every one of them.
    let port_a = spawn_backend_advertising("slim_probe_a").await;
    let port_b = spawn_backend_advertising("slim_probe_b").await;
    register_maya_backend(&registry, port_a).await;
    register_maya_backend(&registry, port_b).await;

    let result = aggregate_tools_list(&state, None).await;
    let names = tool_names(&result);

    assert!(
        !names
            .iter()
            .any(|n| n.contains("slim_probe_a") || n.contains("slim_probe_b")),
        "Slim mode must hide backend tools; leaked {names:?}"
    );
    // Sanity: Tier 1 + 2 must still be present — otherwise agents have
    // no way to discover backends at all.
    assert!(
        names.iter().any(|n| n == "list_dcc_instances"),
        "Tier 1 gateway meta-tools must still be listed in Slim mode; got {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "list_skills"),
        "Tier 2 skill-management tools must still be listed in Slim mode; got {names:?}"
    );
}

/// Issue #652: `Rest` behaves identically to `Slim` for `tools/list`
/// today; the distinction is reserved for future REST-specific
/// behaviour (e.g. emitting capability-index resources). Pin the
/// equivalence so we do not diverge accidentally.
#[tokio::test]
async fn rest_mode_hides_backend_tools_like_slim() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Rest);

    let port = spawn_backend_advertising("rest_probe").await;
    register_maya_backend(&registry, port).await;

    let result = aggregate_tools_list(&state, None).await;
    let names = tool_names(&result);

    assert!(
        !names.iter().any(|n| n.contains("rest_probe")),
        "Rest mode must hide backend tools; leaked {names:?}"
    );
}

// ── Bounded surface: no backends → identical list in every mode ─────────────

/// With no live backends, every mode must return the exact same tier
/// 1 + 2 list. This is the operator-visible guarantee that a gateway
/// fresh out of the box (before any DCC registers) looks the same
/// regardless of the exposure token.
#[tokio::test]
async fn zero_backend_list_is_mode_invariant() {
    let modes = [GatewayToolExposure::Slim, GatewayToolExposure::Rest];

    let mut outputs = Vec::new();
    for mode in modes {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
        let state = make_state(registry, mode);
        let result = aggregate_tools_list(&state, None).await;
        let mut names = tool_names(&result);
        names.sort();
        outputs.push((mode, names));
    }

    // Slim and Rest must return identical tool sets.
    let (_, baseline) = &outputs[0];
    for (mode, names) in &outputs[1..] {
        assert_eq!(
            names, baseline,
            "{mode} returned a different empty-registry tool set than Slim; divergence means the mode enum is leaking into the gateway/skill tables"
        );
    }
}

// ── Diagnostics: mode is visible to operators ───────────────────────────────

/// Issue #652 acceptance: the configured mode must be surfaced through
/// `diagnostics__tool_metrics` so operators tailing logs or hitting the
/// gateway via MCP can verify the runtime configuration without reading
/// process args or env vars.
#[tokio::test]
async fn diagnostics_tool_metrics_surfaces_mode() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry, GatewayToolExposure::Slim);

    let text = dcc_mcp_http::gateway::tools::tool_diagnostics_tool_metrics(&state, &Value::Null)
        .await
        .expect("diagnostics call must succeed");
    let parsed: Value = serde_json::from_str(&text).expect("diagnostics returns JSON");

    assert_eq!(
        parsed["metrics"]["tool_exposure"].as_str(),
        Some("slim"),
        "diagnostics must expose the configured mode verbatim: {parsed}"
    );
    assert_eq!(
        parsed["metrics"]["publishes_backend_tools"].as_bool(),
        Some(false),
        "diagnostics must report publishes_backend_tools=false in Slim mode: {parsed}"
    );
}
