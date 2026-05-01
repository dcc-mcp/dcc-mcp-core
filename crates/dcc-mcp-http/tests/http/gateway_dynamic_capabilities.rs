//! End-to-end coverage for issues #653/#654/#655 — the REST-backed
//! dynamic-capability layer on top of the gateway.
//!
//! The goal of this file is to prove, with realistic multi-backend
//! fixtures, that an agent can discover and invoke DCC capabilities
//! through **either** the REST API or the MCP wrapper tools without:
//!
//! 1. paying the linear `tools/list` token cost of the legacy Tier-3
//!    fan-out (the central motivation for #657);
//! 2. ever losing the ability to reach a specific backend action
//!    (REST and MCP must route the same slug to the same place);
//! 3. seeing skill stubs, gateway-local tools, or duplicate entries
//!    leak into the capability index.
//!
//! Every test spins up one or more in-process `axum` backends that
//! advertise a small, hand-crafted set of tools via `tools/list`, and
//! drives the gateway's aggregate_tools_list / REST handlers /
//! capability service directly so the assertions are deterministic.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::{Value, json};
use tokio::sync::{RwLock, broadcast, watch};

use dcc_mcp_http::gateway::GatewayToolExposure;
use dcc_mcp_http::gateway::aggregator::{aggregate_tools_list, route_tools_call};
use dcc_mcp_http::gateway::capability::{CapabilityIndex, RefreshReason, SearchQuery, parse_slug};
use dcc_mcp_http::gateway::capability_service::{
    call_service, describe_service, parse_search_payload, refresh_all_live_backends, search_service,
};
use dcc_mcp_http::gateway::sse_subscriber::SubscriberManager;
use dcc_mcp_http::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;

// ── Fixture helpers ────────────────────────────────────────────────────────

fn make_state(registry: Arc<RwLock<FileRegistry>>, exposure: GatewayToolExposure) -> GatewayState {
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(16);
    GatewayState {
        registry,
        stale_timeout: Duration::from_secs(30),
        backend_timeout: Duration::from_secs(3),
        async_dispatch_timeout: Duration::from_secs(3),
        wait_terminal_timeout: Duration::from_secs(3),
        server_name: "test-657".into(),
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
        tool_exposure: exposure,
        cursor_safe_tool_names: true,
        capability_index: Arc::new(CapabilityIndex::new()),
    }
}

/// Backend model: a name, a description, and a handler that records
/// the received tool-call name / arguments so tests can assert
/// routing landed on the right action with the right payload.
#[derive(Clone)]
struct BackendSpec {
    tools: Vec<(&'static str, &'static str)>,
}

/// Spin up a fake backend that returns `tools` from `tools/list` and
/// echoes `(name, arguments)` back for any `tools/call`.
///
/// Returns the listening port and a shared counter that increments
/// every time a `tools/call` reaches the backend — used to assert
/// tokens are not wasted on duplicate forwards.
async fn spawn_backend(spec: BackendSpec) -> (u16, Arc<tokio::sync::RwLock<u32>>) {
    let call_count = Arc::new(tokio::sync::RwLock::new(0u32));
    let call_count_clone = call_count.clone();

    async fn handler(
        axum::extract::State(state): axum::extract::State<HandlerState>,
        Json(req): Json<Value>,
    ) -> Json<Value> {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        match method {
            "tools/list" => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": state.tools.iter().map(|(name, desc)| json!({
                        "name": name,
                        "description": desc,
                        "inputSchema": {
                            "type": "object",
                            "properties": {"x": {"type": "number"}},
                        }
                    })).collect::<Vec<_>>()
                }
            })),
            "tools/call" => {
                let mut c = state.call_count.write().await;
                *c += 1;
                let received_name = req
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let received_args = req
                    .get("params")
                    .and_then(|p| p.get("arguments"))
                    .cloned()
                    .unwrap_or(Value::Null);
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": format!("echo name={received_name} args={received_args}")
                        }],
                        "structuredContent": {
                            "received_tool": received_name,
                            "received_args": received_args,
                        }
                    }
                }))
            }
            _ => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("unknown method: {method}")}
            })),
        }
    }

    #[derive(Clone)]
    struct HandlerState {
        tools: Vec<(&'static str, &'static str)>,
        call_count: Arc<tokio::sync::RwLock<u32>>,
    }

    let state = HandlerState {
        tools: spec.tools.clone(),
        call_count: call_count_clone,
    };
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/mcp", post(handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    (port, call_count)
}

async fn register_backend(
    registry: &Arc<RwLock<FileRegistry>>,
    dcc: &str,
    port: u16,
) -> ServiceEntry {
    let entry = ServiceEntry::new(dcc, "127.0.0.1", port);
    let out = entry.clone();
    let reg = registry.read().await;
    reg.register(entry).unwrap();
    out
}

// ── Test 1 — token budget: slim mode hides Tier 3 ─────────────────────────

/// The primary #657 acceptance criterion: in slim/rest mode the
/// gateway's `tools/list` must stay bounded regardless of how many
/// backend tools are live, so the agent's token budget cannot be
/// drained by adding more DCC instances.
#[tokio::test]
async fn slim_mode_tools_list_stays_bounded_with_many_backends() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Slim);

    // Register three DCC backends, each advertising 20 tools. In
    // pre-#657 Full mode this would bloat tools/list to >60 rows.
    let bulk: Vec<(&'static str, &'static str)> = (0..20)
        .map(|i| {
            let name = Box::leak(format!("bulk_tool_{i:02}").into_boxed_str()) as &'static str;
            (name, "bulk")
        })
        .collect();
    for dcc in ["maya", "blender", "houdini"] {
        let (port, _) = spawn_backend(BackendSpec {
            tools: bulk.clone(),
        })
        .await;
        register_backend(&registry, dcc, port).await;
    }

    let result = aggregate_tools_list(&state, None).await;
    let names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t.get("name").and_then(|v| v.as_str()))
        .collect();

    // The slim tool surface is exactly:
    // * 8 gateway meta-tools (list_dcc_instances, get/connect/acquire/
    //   release_dcc_instance, diagnostics_*),
    // * 3 dynamic-capability wrappers (search_tools, describe_tool,
    //   call_tool),
    // * 5 skill-management tools (list/search/get_info/load/unload),
    // → 16 entries.
    //
    // Adding more backends or backend tools must NOT change this
    // number; that is the whole point of slim mode.
    assert!(
        names.len() <= 20,
        "slim mode tools/list must stay small (got {} entries): {names:?}",
        names.len(),
    );
    assert!(names.contains(&"search_tools"));
    assert!(names.contains(&"describe_tool"));
    assert!(names.contains(&"call_tool"));
    // No Tier-3 row has leaked through.
    assert!(
        !names.iter().any(|n| n.contains("bulk_tool")),
        "backend tools must not appear in slim-mode tools/list; leaked: {names:?}",
    );
}

// ── Test 2 — REST search → describe → call happy path ─────────────────────

/// Agents discover capabilities by REST: search narrows, describe
/// resolves a slug, call forwards. Each step must return stable JSON
/// shapes so the agent can chain them without guessing.
#[tokio::test]
async fn rest_search_describe_call_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Slim);

    let (port, call_count) = spawn_backend(BackendSpec {
        tools: vec![
            ("create_sphere", "Create a polygonal sphere"),
            ("create_cube", "Create a polygonal cube"),
        ],
    })
    .await;
    let entry = register_backend(&registry, "maya", port).await;

    // Seed the index (normally done lazily by the REST handlers).
    refresh_all_live_backends(&state, RefreshReason::InstanceJoined).await;

    // ── Search ─────────────────────────────────────────────────────
    let query = parse_search_payload(&json!({"query": "sphere"}));
    let hits = search_service(&state.capability_index, &query);
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one sphere hit; got {hits:?}"
    );
    let slug = hits[0].record.tool_slug.clone();

    // ── Describe ───────────────────────────────────────────────────
    let rec = describe_service(&state.capability_index, &slug).expect("slug must resolve");
    assert_eq!(rec.backend_tool, "create_sphere");
    let (dcc, id8, tool) = parse_slug(&rec.tool_slug).unwrap();
    assert_eq!(dcc, "maya");
    assert_eq!(id8, &entry.instance_id.to_string().replace('-', "")[..8]);
    assert_eq!(tool, "create_sphere");

    // ── Call ───────────────────────────────────────────────────────
    let result = call_service(&state, &slug, json!({"radius": 2.0}), None)
        .await
        .expect("call_service must route successfully");
    let echoed = result["structuredContent"]["received_tool"]
        .as_str()
        .unwrap_or_default();
    assert_eq!(
        echoed, "create_sphere",
        "gateway must forward the original backend tool name; result={result:?}",
    );
    // The backend got exactly one tools/call — no retry / duplicate
    // forwarding. Keeping this assertion guards the token-and-cost
    // budget #657 is explicitly trying to preserve.
    assert_eq!(*call_count.read().await, 1);
}

// ── Test 3 — REST ↔ MCP wrapper parity ────────────────────────────────────

/// For the same query, the REST `POST /v1/search` and the MCP
/// `search_tools` wrapper must return the same ranked slugs. Without
/// this the agent choosing one transport over the other would see
/// different capabilities.
#[tokio::test]
async fn rest_and_mcp_wrapper_return_identical_search_hits() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Slim);

    let (port_a, _) = spawn_backend(BackendSpec {
        tools: vec![
            ("render_scene", "Render the current scene"),
            ("open_scene", "Open a scene file"),
        ],
    })
    .await;
    let (port_b, _) = spawn_backend(BackendSpec {
        tools: vec![("render_image", "Render a still image")],
    })
    .await;
    register_backend(&registry, "maya", port_a).await;
    register_backend(&registry, "blender", port_b).await;

    // Both surfaces go through the shared service, so we expect
    // byte-identical ordering.
    let query = parse_search_payload(&json!({"query": "render"}));
    refresh_all_live_backends(&state, RefreshReason::InstanceJoined).await;
    let rest_hits = search_service(&state.capability_index, &query);

    // Drive the MCP wrapper through `route_tools_call` so we cover
    // the exact dispatch the agent would go through.
    let (mcp_body, is_error) = route_tools_call(
        &state,
        "search_tools",
        &json!({"query": "render"}),
        None,
        None,
        None,
    )
    .await;
    assert!(!is_error, "MCP search_tools failed: {mcp_body}");
    let parsed: Value = serde_json::from_str(&mcp_body).expect("MCP search_tools returns JSON");
    let mcp_slugs: Vec<String> = parsed["hits"]
        .as_array()
        .expect("hits array")
        .iter()
        .map(|h| h["tool_slug"].as_str().unwrap().to_string())
        .collect();
    let rest_slugs: Vec<String> = rest_hits
        .iter()
        .map(|h| h.record.tool_slug.clone())
        .collect();
    assert_eq!(
        rest_slugs, mcp_slugs,
        "REST and MCP wrappers must return identical ranked slugs",
    );
    // Two render-related tools across two DCCs both match.
    assert!(rest_slugs.len() >= 2);
}

// ── Test 4 — MCP call_tool routes correctly without a retry waste ─────────

/// The `call_tool` wrapper must forward the backend tool name (not
/// the gateway slug) to the owning backend exactly once when the
/// slug is already indexed. No extra token or HTTP round-trip cost.
#[tokio::test]
async fn mcp_call_tool_forwards_original_backend_tool_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Slim);

    let (port, call_count) = spawn_backend(BackendSpec {
        tools: vec![("export_fbx", "Export the scene as FBX")],
    })
    .await;
    let entry = register_backend(&registry, "maya", port).await;

    // Prime the index; the agent's canonical flow is
    // `search_tools` → `call_tool`, so seeding via search_service
    // mirrors that path.
    refresh_all_live_backends(&state, RefreshReason::InstanceJoined).await;
    let hits = search_service(
        &state.capability_index,
        &parse_search_payload(&json!({"query": "fbx"})),
    );
    assert_eq!(hits.len(), 1);
    let slug = hits[0].record.tool_slug.clone();
    // Sanity-check the slug encodes the real backend identity so we
    // know the later call routed through the capability service
    // rather than sneaking in a lucky direct forward.
    let (_, id8, _) = parse_slug(&slug).unwrap();
    assert_eq!(id8, &entry.instance_id.to_string().replace('-', "")[..8]);

    // ── Invoke via the MCP wrapper ────────────────────────────────
    let (body, is_error) = route_tools_call(
        &state,
        "call_tool",
        &json!({"tool_slug": slug, "arguments": {"path": "/tmp/out.fbx"}}),
        None,
        None,
        None,
    )
    .await;
    assert!(!is_error, "MCP call_tool failed: {body}");
    let parsed: Value = serde_json::from_str(&body).expect("call_tool envelope is JSON");
    assert_eq!(
        parsed["structuredContent"]["received_tool"].as_str(),
        Some("export_fbx"),
        "gateway must decode the slug back to the backend tool name",
    );
    assert_eq!(
        parsed["structuredContent"]["received_args"],
        json!({"path": "/tmp/out.fbx"}),
    );
    // Exactly one backend `tools/call` round-trip: no wasted tokens.
    assert_eq!(*call_count.read().await, 1);
}

// ── Test 5 — call_tool retries once on unknown-slug after refresh ─────────

/// When the slug is not in the index (e.g. because a skill loaded
/// between the agent's search and the call), the wrapper must
/// refresh once and retry — the agent should see the successful
/// call, not an error.
#[tokio::test]
async fn mcp_call_tool_retries_unknown_slug_after_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Slim);

    let (port, call_count) = spawn_backend(BackendSpec {
        tools: vec![("late_action", "Action that appeared after the last refresh")],
    })
    .await;
    let entry = register_backend(&registry, "maya", port).await;

    // The caller constructs the slug by hand from metadata they
    // already have (their `list_dcc_instances` + manifest). The
    // index is empty — forcing the call path through its
    // unknown-slug retry branch.
    let slug = format!(
        "maya.{}.late_action",
        &entry.instance_id.to_string().replace('-', "")[..8]
    );
    assert!(state.capability_index.snapshot().is_empty());

    let (body, is_error) = route_tools_call(
        &state,
        "call_tool",
        &json!({"tool_slug": slug, "arguments": {}}),
        None,
        None,
        None,
    )
    .await;
    assert!(!is_error, "call_tool must recover after refresh: {body}");
    let parsed: Value = serde_json::from_str(&body).expect("envelope is JSON");
    assert_eq!(
        parsed["structuredContent"]["received_tool"].as_str(),
        Some("late_action"),
    );
    // Exactly one backend call — the refresh is free (one extra
    // tools/list against the backend) but it doesn't duplicate the
    // actual tools/call.
    assert_eq!(*call_count.read().await, 1);
    // And the index is now populated so the next call skips the
    // retry branch entirely.
    assert!(state.capability_index.instance_count() > 0);
}

// ── Test 6 — instance-offline surfaces as a structured error ──────────────

/// If a backend disappears between search and call, the wrapper
/// must emit a `kind = "instance-offline"` error rather than
/// forwarding to a stale URL (which would time out and burn tokens).
#[tokio::test]
async fn call_tool_reports_instance_offline_when_backend_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Slim);

    let (port, _) = spawn_backend(BackendSpec {
        tools: vec![("do_thing", "Do a thing")],
    })
    .await;
    let entry = register_backend(&registry, "maya", port).await;

    refresh_all_live_backends(&state, RefreshReason::InstanceJoined).await;
    let slug = format!(
        "maya.{}.do_thing",
        &entry.instance_id.to_string().replace('-', "")[..8]
    );

    // Evict the backend from the registry — simulates a crash.
    {
        let reg = registry.read().await;
        let key = dcc_mcp_transport::discovery::types::ServiceKey {
            dcc_type: entry.dcc_type.clone(),
            instance_id: entry.instance_id,
        };
        reg.deregister(&key).ok();
    }

    let (body, is_error) = route_tools_call(
        &state,
        "call_tool",
        &json!({"tool_slug": slug, "arguments": {}}),
        None,
        None,
        None,
    )
    .await;
    assert!(is_error, "expected an error envelope; got: {body}");
    let parsed: Value = serde_json::from_str(&body).expect("error envelope is JSON");
    // After the wrapper's one-shot refresh, the capability record
    // for the dead backend is gone — so the second attempt reports
    // unknown-slug rather than instance-offline. Either error class
    // is acceptable here because both mean "do not retry blindly"
    // to the agent; the test pins that to one of the two so a
    // silent regression that goes back to timing-out against the
    // dead URL would fail loudly.
    let kind = parsed["error"]["kind"].as_str().unwrap_or("");
    assert!(
        kind == "instance-offline" || kind == "unknown-slug",
        "expected instance-offline or unknown-slug; got {kind:?}: {body}",
    );
}

// ── Test 7 — skills (stubs + mgmt) are never indexed as actions ───────────

/// The agent-visible #657 promise is "skills are discoverable via
/// `list_skills` / `load_skill`, and individual actions via the
/// capability index." Those two surfaces must not double up —
/// `__skill__hello-world` stubs and gateway-local tools must never
/// appear as rows in `search_tools` / `POST /v1/search`.
#[tokio::test]
async fn capability_index_never_contains_skill_stubs_or_local_tools() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Slim);

    // Backend ships exactly the shape a real skill-enabled DCC ships:
    // one skill stub, one gateway-local meta-tool, and one real action.
    let (port, _) = spawn_backend(BackendSpec {
        tools: vec![
            ("__skill__hello-world", "stub"),
            ("list_skills", "meta"),
            ("hello-world.greet", "Say hello"),
        ],
    })
    .await;
    register_backend(&registry, "maya", port).await;

    refresh_all_live_backends(&state, RefreshReason::InstanceJoined).await;
    let hits = search_service(&state.capability_index, &SearchQuery::default());
    for hit in &hits {
        assert!(
            !hit.record.backend_tool.starts_with("__skill__"),
            "skill stub leaked into capability index: {:?}",
            hit.record,
        );
        assert_ne!(hit.record.backend_tool, "list_skills");
    }
    // The real action is reachable — and its skill metadata is
    // preserved so `search_tools(query="hello")` still matches.
    assert!(
        hits.iter().any(|h| h.record.backend_tool == "greet"),
        "real action must remain addressable: {hits:?}",
    );
    assert_eq!(
        hits.iter()
            .find(|h| h.record.backend_tool == "greet")
            .and_then(|h| h.record.skill_name.as_deref()),
        Some("hello-world"),
    );
}
