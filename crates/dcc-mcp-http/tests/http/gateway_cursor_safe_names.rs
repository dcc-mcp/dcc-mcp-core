//! Regression coverage for issue #656 — Cursor-safe gateway tool names.
//!
//! The gateway must
//!
//! 1. Emit every Tier 3 backend tool under the Cursor-safe
//!    `i_<id8>__<escaped>` form by default so clients that only accept
//!    `^[A-Za-z0-9_]+$` (notably Cursor) keep seeing the full backend
//!    surface. Historically the gateway published `<id8>.<tool>`; those
//!    names silently disappeared from Cursor's tool picker and were the
//!    motivation for this issue.
//! 2. Keep routing `tools/call` for **every** historically-published
//!    wire form during the compatibility window: the new Cursor-safe
//!    form, the SEP-986 dotted form, and — on a best-effort basis — the
//!    pre-#258 double-underscore form. An agent that has not upgraded
//!    yet must not see "Unknown tool" errors.
//! 3. Support backend tool names that themselves contain dots or
//!    hyphens (e.g. skill-prefixed actions like
//!    `maya-animation.set_keyframe`). The encoding is reversible so the
//!    gateway can still address the original tool on the backend.
//! 4. Suppress the single-instance bare-name alias when the backend
//!    name is not already Cursor-safe. Leaking `maya-animation.foo` as
//!    an alias would undo the whole point of this mode.
//! 5. Revert to the pre-#656 SEP-986 dotted form when the operator
//!    explicitly disables [`GatewayState::cursor_safe_tool_names`].
//!
//! The tests exercise a real in-process axum backend registered through
//! the same `FileRegistry` the gateway aggregator consults, so the
//! fan-out code path is covered end-to-end rather than mocked out.

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
use dcc_mcp_http::gateway::sse_subscriber::SubscriberManager;
use dcc_mcp_http::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;

// ── Fixture helpers ────────────────────────────────────────────────────────

/// Build a `GatewayState` with the Cursor-safe flag pinned explicitly.
///
/// The test intentionally flips the flag both ways so the assertion
/// about default-on behaviour is enforced at the call site rather than
/// relying on `GatewayState::Default::default()` (which this crate does
/// not provide — the struct is built field-by-field in production).
fn make_state(
    registry: Arc<RwLock<FileRegistry>>,
    tool_exposure: GatewayToolExposure,
    cursor_safe_tool_names: bool,
) -> GatewayState {
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(16);
    GatewayState {
        registry,
        stale_timeout: Duration::from_secs(30),
        // Keep every timeout small so a broken fixture cannot stall
        // CI; the tests never wait on a real backend.
        backend_timeout: Duration::from_secs(2),
        async_dispatch_timeout: Duration::from_secs(2),
        wait_terminal_timeout: Duration::from_secs(2),
        server_name: "test-656".into(),
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
        cursor_safe_tool_names,
    }
}

/// Spawn a minimal MCP-compatible backend that advertises the given
/// tool name and echoes the received tool name back on `tools/call`.
///
/// The echo is critical for the routing-compatibility tests: the
/// backend never sees the gateway's wire form, so the only way to
/// verify decode is to assert the backend received the **original**
/// tool name regardless of which encoded form the caller sent.
async fn spawn_echo_backend(tool_name: &'static str) -> u16 {
    async fn handler(
        axum::extract::State(tool_name): axum::extract::State<&'static str>,
        Json(req): Json<Value>,
    ) -> Json<Value> {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        match method {
            "tools/list" => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": tool_name,
                        "description": format!("echo tool advertised as {tool_name}"),
                        "inputSchema": {"type": "object", "properties": {}}
                    }]
                }
            })),
            "tools/call" => {
                let received = req
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_string();
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": format!("echo:{received}")
                        }]
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

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/mcp", post(handler))
        .with_state(tool_name);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    // Give the OS a moment to put the socket in the listening state.
    tokio::time::sleep(Duration::from_millis(30)).await;
    port
}

async fn register_maya_backend(registry: &Arc<RwLock<FileRegistry>>, port: u16) -> ServiceEntry {
    let entry = ServiceEntry::new("maya", "127.0.0.1", port);
    let out = entry.clone();
    let reg = registry.read().await;
    reg.register(entry).unwrap();
    out
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

fn is_cursor_safe_name(s: &str) -> bool {
    // Mirror the client-side regex that Cursor (and several other MCP
    // clients) apply to tool names: `^[A-Za-z0-9_]+$`. Anything outside
    // this alphabet is silently filtered out of the agent's view.
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Byte-identical local copy of the cursor-safe encoder. Using the
/// internal helper directly would couple the integration test to the
/// module-private API; a local mirror also doubles as an executable
/// spec of the wire form we promise external clients.
fn cursor_safe_wire(instance_id: uuid::Uuid, tool: &str) -> String {
    let short = instance_id.to_string().replace('-', "")[..8].to_string();
    let escaped: String = tool
        .bytes()
        .map(|b| match b {
            b'_' => "_U_".to_string(),
            b'.' => "_D_".to_string(),
            b'-' => "_H_".to_string(),
            other if other.is_ascii_alphanumeric() => (other as char).to_string(),
            other => panic!("byte {other:#04x} not in SEP-986 alphabet for {tool:?}"),
        })
        .collect();
    format!("i_{short}__{escaped}")
}

fn legacy_dot_wire(instance_id: uuid::Uuid, tool: &str) -> String {
    let short = instance_id.to_string().replace('-', "")[..8].to_string();
    format!("{short}.{tool}")
}

// ── Default on: every emitted name is Cursor-safe ──────────────────────────

/// The primary acceptance criterion for #656: when the default config
/// is loaded, **no** gateway-published tool name contains a character
/// outside `[A-Za-z0-9_]`. This test exercises the end-to-end aggregator
/// including the single-instance bare-alias branch (#583), which was
/// the path most likely to leak a stray `.` or `-` into the wire form.
#[tokio::test]
async fn default_gateway_emits_only_cursor_safe_names() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Full, true);

    // Advertise a dotted + hyphenated skill-prefixed name — the worst
    // case for cursor-safe emission because the bare alias path would
    // historically leak `.` and `-` straight through (#583).
    let port = spawn_echo_backend("maya-animation.set_keyframe").await;
    register_maya_backend(&registry, port).await;

    let result = aggregate_tools_list(&state, None).await;
    let names = tool_names(&result);
    for name in &names {
        assert!(
            is_cursor_safe_name(name),
            "gateway leaked non-cursor-safe name {name:?} — full list: {names:?}",
        );
    }
}

/// Issue #656 spec: skill-prefixed backend names (which carry both `.`
/// and `-`) must round-trip through the Cursor-safe wire form and
/// remain addressable. Agents using the `i_<id8>__<escaped>` form
/// must reach the same backend tool they would have hit via the old
/// dotted form.
#[tokio::test]
async fn cursor_safe_wire_form_routes_tool_call_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Full, true);

    let port = spawn_echo_backend("maya-animation.set_keyframe").await;
    let entry = register_maya_backend(&registry, port).await;

    let wire = cursor_safe_wire(entry.instance_id, "maya-animation.set_keyframe");
    let (body, is_error) = route_tools_call(&state, &wire, &json!({}), None, None, None).await;

    assert!(
        !is_error,
        "cursor-safe routing must succeed; got error body {body:?}",
    );
    // The backend echoes the tool name it received. If the gateway
    // forwarded the encoded wire form rather than the decoded
    // `maya-animation.set_keyframe`, the backend would not recognise
    // it and the assertion below would catch the regression.
    assert!(
        body.contains("echo:maya-animation.set_keyframe"),
        "gateway must decode the cursor-safe wire form back to the \
         original backend tool name before forwarding; got body {body:?}",
    );
}

/// Compatibility window: agents (or older gateways) that kept the
/// pre-#656 SEP-986 dotted name in memory must still route correctly.
/// The decoder accepts both forms simultaneously so rollout does not
/// require coordinated upgrades of every client.
#[tokio::test]
async fn legacy_dot_wire_form_still_routes_during_compat_window() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Full, true);

    let port = spawn_echo_backend("execute_python").await;
    let entry = register_maya_backend(&registry, port).await;

    let wire = legacy_dot_wire(entry.instance_id, "execute_python");
    let (body, is_error) = route_tools_call(&state, &wire, &json!({}), None, None, None).await;

    assert!(
        !is_error,
        "legacy dotted wire form must still route during the compat window; got {body:?}",
    );
    assert!(
        body.contains("echo:execute_python"),
        "gateway must still decode the pre-#656 dotted form; got {body:?}",
    );
}

// ── Bare-alias path (#583) in cursor-safe mode ─────────────────────────────

/// Single-instance mode used to publish a bare alias (#583) so agents
/// can call `create_sphere` instead of the prefixed form. With
/// cursor-safe on, the alias is safe iff the bare name already matches
/// `[A-Za-z0-9_]+` — otherwise we would be leaking a `.`/`-` name right
/// next to the encoded one. This test pins both branches.
#[tokio::test]
async fn single_instance_bare_alias_is_suppressed_for_unsafe_names() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Full, true);

    // Skill-prefixed backend name — the bare alias would historically
    // surface as `maya-animation.set_keyframe`, which fails the
    // cursor-safe regex and would be filtered by the client.
    let port = spawn_echo_backend("maya-animation.set_keyframe").await;
    register_maya_backend(&registry, port).await;

    let names = tool_names(&aggregate_tools_list(&state, None).await);
    assert!(
        !names.iter().any(|n| n == "maya-animation.set_keyframe"),
        "unsafe bare alias must not be emitted in cursor-safe mode; got {names:?}",
    );
}

/// Conversely, a bare name that already fits the cursor-safe alphabet
/// must still surface as an alias so single-backend ergonomics (#583)
/// are preserved for the common case of plain-identifier tools.
#[tokio::test]
async fn single_instance_bare_alias_is_kept_for_safe_names() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Full, true);

    let port = spawn_echo_backend("create_sphere").await;
    register_maya_backend(&registry, port).await;

    let names = tool_names(&aggregate_tools_list(&state, None).await);
    assert!(
        names.iter().any(|n| n == "create_sphere"),
        "cursor-safe bare alias must still be emitted for plain names; got {names:?}",
    );
}

// ── Opt-out: operators can still pin the SEP-986 dotted form ───────────────

/// The legacy wire form is still useful for diagnostic parity with
/// a single-instance server that publishes SEP-986 dotted names
/// directly. Flipping `cursor_safe_tool_names` to `false` must restore
/// the pre-#656 behaviour verbatim so deployments can opt out without
/// downgrading the binary.
#[tokio::test]
async fn disabling_cursor_safe_restores_sep986_dotted_form() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Full, false);

    let port = spawn_echo_backend("create_sphere").await;
    let entry = register_maya_backend(&registry, port).await;

    let names = tool_names(&aggregate_tools_list(&state, None).await);
    let expected_legacy = legacy_dot_wire(entry.instance_id, "create_sphere");
    assert!(
        names.iter().any(|n| n == &expected_legacy),
        "opt-out must emit the pre-#656 dotted form {expected_legacy:?}; got {names:?}",
    );
    assert!(
        !names.iter().any(|n| n.starts_with("i_")),
        "opt-out must not emit the cursor-safe form; got {names:?}",
    );
}

// ── Error path: unknown tool hint mentions the new wrapper tools ───────────

/// The #657 roadmap moves discovery/invocation to `search_tools`,
/// `describe_tool`, `call_tool`. The "Unknown tool" error text is the
/// first thing an agent sees when it tries to call a tool that isn't
/// emitted (e.g. because cursor-safe mode suppressed an alias). Point
/// agents at the wrapper tools so they recover without human help.
#[tokio::test]
async fn unknown_tool_hint_points_at_search_describe_call() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let state = make_state(registry.clone(), GatewayToolExposure::Full, true);

    // Two backends → the "one backend, accept any bare name" shortcut
    // (#583) is off, so the decoder falls straight into the
    // Unknown-tool branch.
    let port_a = spawn_echo_backend("probe_a").await;
    let port_b = spawn_echo_backend("probe_b").await;
    register_maya_backend(&registry, port_a).await;
    register_maya_backend(&registry, port_b).await;

    let (body, is_error) = route_tools_call(
        &state,
        "definitely_not_a_tool",
        &json!({}),
        None,
        None,
        None,
    )
    .await;
    assert!(is_error);
    for keyword in ["search_tools", "describe_tool", "call_tool"] {
        assert!(
            body.contains(keyword),
            "Unknown-tool hint must mention {keyword:?} so agents know the new \
             dynamic-capability entry points; full body: {body:?}",
        );
    }
}
