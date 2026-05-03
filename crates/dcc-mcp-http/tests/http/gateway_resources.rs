//! End-to-end tests for gateway resource forwarding (#732).
//!
//! Spin up a real `McpHttpServer` that wins the gateway election, plus one
//! or two real plain-instance backends sharing a common registry directory,
//! then drive the gateway's `/mcp` endpoint over real HTTP to exercise the
//! full wire contract:
//!
//! * `resources/list` returns admin pointers PLUS every backend's resources
//!   with URIs rewritten to `<scheme>://<id8>/<rest>`.
//! * `resources/read` on the prefixed form forwards to the owning backend
//!   and returns the raw payload unchanged — including base64 `blob` bytes
//!   for binary mime-types, byte-for-byte identical to a direct backend read.
//! * `resources/subscribe` + a backend scene update propagates to the
//!   subscribing SSE client as `notifications/resources/updated` within a
//!   short deadline, with `params.uri` rewritten to the client-visible
//!   prefixed URI. Unsubscribe then stops the propagation.
//! * The aggregated `resources/list_changed` watcher emits one SSE frame
//!   when a backend adds or removes a resource.
//! * Fail-soft: one backend with a closed port does not take down
//!   `resources/list` — the healthy backend's resources still appear.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use dcc_mcp_actions::ActionRegistry;
use dcc_mcp_http::{McpHttpConfig, McpHttpServer, McpServerHandle};
use serde_json::{Value, json};

// ── helpers ──────────────────────────────────────────────────────────────

/// Pick a free TCP port on 127.0.0.1 and return it. The port is closed
/// before returning, so there is a tiny race window — acceptable for
/// tests which bind immediately after.
fn pick_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Start a plain-instance backend registered in `registry_dir`. The
/// returned handle owns the serving task; `resources/list`
/// automatically exposes `scene://current` (with a synthetic payload)
/// and `audit://recent` via the default `ResourceRegistry`.
///
/// The `ResourceRegistry` is kept alive via `McpHttpServer`'s internal
/// `Arc` and reachable through `ResourceRegistry::handle_for` only
/// while the server is running — callers that need to drive
/// `set_scene` later must keep a clone of the registry handle from
/// `McpHttpServer::resources()` **before** `start()` consumes self.
async fn spawn_backend_with_scene(
    dcc_type: &str,
    gw_port: u16,
    registry_dir: &std::path::Path,
    initial_scene: Value,
) -> (McpServerHandle, dcc_mcp_http::ResourceRegistry) {
    let action_registry = Arc::new(ActionRegistry::new());
    let cfg = McpHttpConfig::new(0)
        .with_name(format!("{dcc_type}-resources-e2e"))
        .with_gateway(gw_port)
        .with_dcc_type(dcc_type)
        .with_registry_dir(registry_dir);

    let server = McpHttpServer::new(action_registry, cfg);
    // Capture a clone of the registry handle BEFORE start() — the registry
    // is internally an `Arc`, so this clone shares the same subscription
    // map and producers list the serving task holds.
    let resources = server.resources().clone();
    resources.set_scene(initial_scene);
    let handle = server
        .start()
        .await
        .expect("backend McpHttpServer must start");
    (handle, resources)
}

/// POST a JSON-RPC request body to `url` and return the parsed JSON reply.
async fn post_rpc(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: Option<Value>,
    id: i64,
) -> Value {
    let mut body = json!({"jsonrpc": "2.0", "id": id, "method": method});
    if let Some(p) = params {
        body["params"] = p;
    }
    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body.to_string())
        .send()
        .await
        .expect("request must succeed");
    let text = resp.text().await.expect("body readable");
    serde_json::from_str(&text).unwrap_or_else(|_| panic!("invalid JSON response: {text}"))
}

/// Poll `f` every 50 ms up to `budget`, returning the first non-None
/// result. Panics with `diag` on timeout.
async fn wait_until<T, F>(budget: Duration, diag: &str, mut f: F) -> T
where
    F: FnMut() -> Option<T>,
{
    let deadline = Instant::now() + budget;
    loop {
        if let Some(v) = f() {
            return v;
        }
        if Instant::now() > deadline {
            panic!("timeout after {budget:?}: {diag}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Extract the `resources` array from a successful `resources/list`
/// reply — panic with `method` + error payload on failure.
fn resources_from_reply(reply: &Value) -> &Vec<Value> {
    if reply.get("error").is_some() {
        panic!("resources/list returned error: {reply:#}");
    }
    reply
        .get("result")
        .and_then(|r| r.get("resources"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no resources array in reply: {reply:#}"))
}

/// Try to extract the 8-char hex instance id from a prefixed URI.
/// Returns `None` for admin pointers (`dcc://...`) or non-prefixed URIs.
fn id8_from_prefixed(uri: &str) -> Option<(String, String)> {
    let (scheme, rest) = uri.split_once("://")?;
    let (id, remainder) = rest.split_once('/').unwrap_or((rest, ""));
    if id.len() == 8 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some((id.to_string(), format!("{scheme}://{remainder}")))
    } else {
        None
    }
}

/// Sleep just past the gateway's 2-second instance-watcher tick so a new
/// registration is visible through the facade. Slightly generous to
/// account for CI jitter.
async fn wait_for_instance_watcher() {
    tokio::time::sleep(Duration::from_millis(2500)).await;
}

// ── integration tests ────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gateway_resources_list_merges_admin_pointers_and_prefixed_backend_uris() {
    let registry_dir = tempfile::tempdir().unwrap();
    let gw_port = pick_free_port();

    // First backend — wins gateway election.
    let (handle_a, _res_a) = spawn_backend_with_scene(
        "maya",
        gw_port,
        registry_dir.path(),
        json!({"scene_name": "maya_shot_A", "node_count": 7}),
    )
    .await;
    // Give the elected process time to bind the gateway listener.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Second backend — plain instance under the same gateway.
    let (handle_b, _res_b) = spawn_backend_with_scene(
        "blender",
        gw_port,
        registry_dir.path(),
        json!({"scene_name": "blender_shot_B", "node_count": 42}),
    )
    .await;
    wait_for_instance_watcher().await;

    assert!(handle_a.is_gateway, "backend A must win gateway election");
    assert!(!handle_b.is_gateway, "backend B must be a plain instance");

    let gw_url = format!("http://127.0.0.1:{gw_port}/mcp");
    let client = reqwest::Client::new();
    let reply = post_rpc(&client, &gw_url, "resources/list", None, 1).await;
    let resources = resources_from_reply(&reply);
    let uris: Vec<&str> = resources
        .iter()
        .filter_map(|r| r.get("uri").and_then(Value::as_str))
        .collect();

    // Admin pointers — one per live DCC instance.
    assert!(
        uris.iter().any(|u| u.starts_with("dcc://maya/")),
        "missing dcc://maya/ admin pointer; got {uris:#?}",
    );
    assert!(
        uris.iter().any(|u| u.starts_with("dcc://blender/")),
        "missing dcc://blender/ admin pointer; got {uris:#?}",
    );

    // Backend-contributed resources — every live instance's scene + audit.
    let scene_prefixed: Vec<&str> = uris
        .iter()
        .copied()
        .filter(|u| u.starts_with("scene://") && id8_from_prefixed(u).is_some())
        .collect();
    assert_eq!(
        scene_prefixed.len(),
        2,
        "expected one prefixed scene URI per backend, got {scene_prefixed:#?} (all uris: {uris:#?})",
    );
    let audit_prefixed: Vec<&str> = uris
        .iter()
        .copied()
        .filter(|u| u.starts_with("audit://") && id8_from_prefixed(u).is_some())
        .collect();
    assert_eq!(
        audit_prefixed.len(),
        2,
        "expected one prefixed audit URI per backend, got {audit_prefixed:#?}",
    );

    // No unprefixed backend URIs leaked — clients would not be able to
    // route them back to a specific instance.
    assert!(
        !uris.contains(&"scene://current"),
        "gateway leaked unprefixed backend URI; got {uris:#?}",
    );

    handle_b.shutdown().await;
    handle_a.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gateway_resources_read_matches_direct_backend_byte_for_byte() {
    let registry_dir = tempfile::tempdir().unwrap();
    let gw_port = pick_free_port();

    let scene_payload = json!({
        "scene_name": "byte_round_trip",
        "nodes": ["alpha", "beta", "gamma"],
        "frame_range": [1, 240],
        "unicode": "café — こんにちは — 🌟",
    });
    let (handle, _res) =
        spawn_backend_with_scene("maya", gw_port, registry_dir.path(), scene_payload.clone()).await;
    wait_for_instance_watcher().await;
    assert!(handle.is_gateway);

    let gw_url = format!("http://127.0.0.1:{gw_port}/mcp");
    let backend_url = format!("http://{}/mcp", handle.bind_addr);
    let client = reqwest::Client::new();

    // Discover the prefixed scene URI from the gateway's list.
    let list_reply = post_rpc(&client, &gw_url, "resources/list", None, 1).await;
    let resources = resources_from_reply(&list_reply);
    let prefixed_scene = resources
        .iter()
        .filter_map(|r| r.get("uri").and_then(Value::as_str))
        .find(|u| u.starts_with("scene://") && id8_from_prefixed(u).is_some())
        .unwrap_or_else(|| panic!("no prefixed scene URI in {resources:#?}"))
        .to_owned();

    // Read via the gateway (prefixed URI) and directly from the backend
    // (raw URI). The contents array must be identical.
    let gw_reply = post_rpc(
        &client,
        &gw_url,
        "resources/read",
        Some(json!({"uri": prefixed_scene})),
        2,
    )
    .await;
    let backend_reply = post_rpc(
        &client,
        &backend_url,
        "resources/read",
        Some(json!({"uri": "scene://current"})),
        2,
    )
    .await;

    assert!(
        gw_reply.get("error").is_none(),
        "gw read errored: {gw_reply:#}"
    );
    assert!(
        backend_reply.get("error").is_none(),
        "backend read errored: {backend_reply:#}"
    );
    let gw_contents = gw_reply["result"]["contents"]
        .as_array()
        .expect("gateway contents array");
    let backend_contents = backend_reply["result"]["contents"]
        .as_array()
        .expect("backend contents array");
    assert_eq!(gw_contents.len(), backend_contents.len());
    // The gateway rewrites `contents[].uri` from the backend URI back
    // to the prefixed client form so agents can match the response to
    // the URI they originally asked for (#732). Everything else —
    // `mimeType`, `text`, and for binary resources `blob` — must
    // round-trip unchanged byte-for-byte.
    for (gw_item, backend_item) in gw_contents.iter().zip(backend_contents.iter()) {
        assert_eq!(
            gw_item.get("uri").and_then(Value::as_str),
            Some(prefixed_scene.as_str()),
        );
        assert_eq!(
            backend_item.get("uri").and_then(Value::as_str),
            Some("scene://current"),
        );
        for key in ["mimeType", "text", "blob"] {
            assert_eq!(
                gw_item.get(key),
                backend_item.get(key),
                "{key} must survive the proxy unchanged",
            );
        }
    }

    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gateway_resources_read_preserves_blob_bytes_end_to_end() {
    // Install a custom producer on a backend that returns a raw PNG-ish
    // byte payload as a `blob`. The gateway must round-trip the exact
    // base64 string — any UTF-8 lossy conversion anywhere in the path
    // would corrupt the bytes and break this assertion.
    use dcc_mcp_http::{ProducerContent, ResourceError, ResourceProducer, ResourceResult};
    use dcc_mcp_jsonrpc::McpResource;

    struct BlobProducer {
        payload: Vec<u8>,
    }
    impl ResourceProducer for BlobProducer {
        fn scheme(&self) -> &str {
            // Use a unique scheme — the built-in `capture://` producer
            // is registered by default and would take precedence if we
            // also claimed `"capture"`.
            "testblob"
        }
        fn list(&self) -> Vec<McpResource> {
            vec![McpResource {
                uri: "testblob://snapshot".to_string(),
                name: "Binary snapshot".to_string(),
                description: Some("Synthetic PNG for #732 byte round-trip".to_string()),
                mime_type: Some("image/png".to_string()),
            }]
        }
        fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
            if uri != "testblob://snapshot" {
                return Err(ResourceError::NotFound(uri.to_string()));
            }
            Ok(ProducerContent::Blob {
                uri: uri.to_string(),
                mime_type: "image/png".to_string(),
                bytes: self.payload.clone(),
            })
        }
    }

    let registry_dir = tempfile::tempdir().unwrap();
    let gw_port = pick_free_port();

    // Hand-crafted "PNG" — uses the real signature plus a payload that
    // is deliberately not valid UTF-8 (0xFF, 0xFE bytes in the middle).
    // If any layer UTF-8-lossy-decodes this, the round-trip assertion
    // will fail with the pattern `[U+FFFD]` replacement chars visible
    // in the diff.
    let mut raw: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    raw.extend_from_slice(b"IDAT-payload-");
    raw.extend_from_slice(&[0xFF, 0xFE, 0xFD, 0xFC, 0x00, 0x01]);
    raw.extend_from_slice(b"-trailer");
    let expected_b64 = base64::engine::general_purpose::STANDARD.encode(&raw);

    let action_registry = Arc::new(ActionRegistry::new());
    let cfg = McpHttpConfig::new(0)
        .with_name("maya-blob-e2e")
        .with_gateway(gw_port)
        .with_dcc_type("maya")
        .with_registry_dir(registry_dir.path());
    let server = McpHttpServer::new(action_registry, cfg);
    server.resources().add_producer(Arc::new(BlobProducer {
        payload: raw.clone(),
    }));
    let handle = server.start().await.expect("backend must start");
    wait_for_instance_watcher().await;
    assert!(handle.is_gateway);

    let gw_url = format!("http://127.0.0.1:{gw_port}/mcp");
    let client = reqwest::Client::new();

    // Find the prefixed testblob URI.
    let list_reply = post_rpc(&client, &gw_url, "resources/list", None, 1).await;
    let resources = resources_from_reply(&list_reply);
    let prefixed = resources
        .iter()
        .filter_map(|r| r.get("uri").and_then(Value::as_str))
        .find(|u| u.starts_with("testblob://") && id8_from_prefixed(u).is_some())
        .unwrap_or_else(|| panic!("testblob:// prefixed URI missing from {resources:#?}"))
        .to_owned();

    let read_reply = post_rpc(
        &client,
        &gw_url,
        "resources/read",
        Some(json!({"uri": prefixed})),
        2,
    )
    .await;
    assert!(
        read_reply.get("error").is_none(),
        "gateway read errored: {read_reply:#}"
    );
    let content = &read_reply["result"]["contents"][0];
    assert_eq!(
        content["mimeType"].as_str(),
        Some("image/png"),
        "mimeType must survive the proxy"
    );
    let round_tripped_b64 = content["blob"]
        .as_str()
        .expect("blob field must be a base64 string");
    assert_eq!(
        round_tripped_b64, expected_b64,
        "blob base64 must match byte-for-byte through the gateway",
    );

    // Decode and diff the raw bytes to prove there was no UTF-8 lossy
    // conversion anywhere in the round-trip.
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(round_tripped_b64)
        .expect("round-tripped base64 must be valid");
    assert_eq!(
        decoded, raw,
        "decoded bytes must equal the original payload"
    );

    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gateway_resources_list_is_fail_soft_when_one_backend_is_down() {
    let registry_dir = tempfile::tempdir().unwrap();
    let gw_port = pick_free_port();

    let (handle_alive, _res) = spawn_backend_with_scene(
        "maya",
        gw_port,
        registry_dir.path(),
        json!({"scene_name": "alive_one"}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Manually register a dead entry: bind a port, grab it, close the
    // listener, and write a ServiceEntry for that closed port. The
    // gateway's `resources/list` must fan out to both, find the dead
    // port unreachable, emit a WARN, and still return the alive
    // backend's resources.
    let dead_port = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let p = l.local_addr().unwrap().port();
        drop(l);
        p
    };
    {
        let reg =
            dcc_mcp_transport::discovery::file_registry::FileRegistry::new(registry_dir.path())
                .unwrap();
        let entry = dcc_mcp_transport::discovery::types::ServiceEntry::new(
            "blender",
            "127.0.0.1",
            dead_port,
        );
        reg.register(entry).unwrap();
    }
    // Poll until the gateway's live_instances view picks up the dead row.
    wait_for_instance_watcher().await;

    let gw_url = format!("http://127.0.0.1:{gw_port}/mcp");
    let client = reqwest::Client::new();
    let reply = post_rpc(&client, &gw_url, "resources/list", None, 1).await;
    assert!(
        reply.get("error").is_none(),
        "one dead backend must not surface a JSON-RPC error (fail-soft): {reply:#}",
    );
    let resources = resources_from_reply(&reply);
    let uris: Vec<&str> = resources
        .iter()
        .filter_map(|r| r.get("uri").and_then(Value::as_str))
        .collect();

    // Alive backend's prefixed scene URI is present.
    let alive_scene = uris
        .iter()
        .copied()
        .find(|u| u.starts_with("scene://") && id8_from_prefixed(u).is_some());
    assert!(
        alive_scene.is_some(),
        "alive backend's prefixed scene URI missing: {uris:#?}",
    );

    // Admin pointers: expect at least the alive maya row. The blender
    // admin pointer may or may not appear depending on whether the
    // gateway's health-probe has already flipped the row Unreachable in
    // this short window; the hard invariant is that the ALIVE one is
    // present and the call did not 500.
    assert!(
        uris.iter().any(|u| u.starts_with("dcc://maya/")),
        "alive maya admin pointer missing: {uris:#?}",
    );

    handle_alive.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gateway_resources_subscribe_propagates_updated_frame_within_deadline() {
    let registry_dir = tempfile::tempdir().unwrap();
    let gw_port = pick_free_port();

    // Gateway-owning backend (backend A) — resources on A are filtered
    // out of the facade's `live_instances` because it IS the gateway,
    // so subscriptions would never route. Spin up a second, plain
    // backend (B) whose scene we can subscribe to through the facade.
    let (handle_a, _res_a) = spawn_backend_with_scene(
        "maya",
        gw_port,
        registry_dir.path(),
        json!({"scene_name": "gateway_owner_initial"}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(handle_a.is_gateway);

    let (handle_b, resources_registry_b) = spawn_backend_with_scene(
        "blender",
        gw_port,
        registry_dir.path(),
        json!({"scene_name": "plain_backend_initial"}),
    )
    .await;
    wait_for_instance_watcher().await;
    assert!(!handle_b.is_gateway);

    let gw_url = format!("http://127.0.0.1:{gw_port}/mcp");
    let client = reqwest::Client::new();

    // 1. Discover backend B's prefixed scene URI.
    let list_reply = post_rpc(&client, &gw_url, "resources/list", None, 1).await;
    let resources = resources_from_reply(&list_reply);
    let prefixed_scene = resources
        .iter()
        .filter_map(|r| r.get("uri").and_then(Value::as_str))
        .find(|u| u.starts_with("scene://") && id8_from_prefixed(u).is_some())
        .unwrap_or_else(|| panic!("no prefixed scene URI: {resources:#?}"))
        .to_owned();

    // 2. Open an SSE stream on the gateway with a stable session id,
    //    then consume frames on a background task.
    let session_id = "test-session-7a55da".to_string();
    let sse_resp = client
        .get(&gw_url)
        .header("accept", "text/event-stream")
        .header("mcp-session-id", &session_id)
        .send()
        .await
        .expect("SSE GET must succeed");
    assert!(sse_resp.status().is_success());
    let events = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let events_bg = events.clone();
    let sse_task = tokio::spawn(async move {
        use futures::StreamExt;
        let mut stream = sse_resp.bytes_stream();
        let mut buf = Vec::<u8>::new();
        while let Some(Ok(chunk)) = stream.next().await {
            buf.extend_from_slice(&chunk);
            // Drain complete SSE records ("\n\n"-terminated).
            while let Some(pos) = find_double_newline(&buf) {
                let record = buf.drain(..pos).collect::<Vec<u8>>();
                let _ = buf.drain(..2); // trailing "\n\n"
                if let Ok(text) = std::str::from_utf8(&record) {
                    events_bg.lock().await.push(text.to_owned());
                }
            }
        }
    });

    // 3. Subscribe via the SAME session id — the gateway must register
    //    the route for this client session.
    let sub_reply = client
        .post(&gw_url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("mcp-session-id", &session_id)
        .body(
            json!({
                "jsonrpc": "2.0", "id": 2,
                "method": "resources/subscribe",
                "params": {"uri": prefixed_scene}
            })
            .to_string(),
        )
        .send()
        .await
        .expect("subscribe POST must succeed");
    let sub_body: Value = serde_json::from_str(&sub_reply.text().await.unwrap()).unwrap();
    assert!(
        sub_body.get("error").is_none(),
        "subscribe returned error: {sub_body:#}"
    );

    // Give backend B's per-session fan-out a beat to observe the
    // subscription before we fire an update — the subscribe path
    // returns as soon as the backend stores the row, but the
    // `spawn_notifications_task` that pushes updates to session
    // broadcasts runs on a separate tokio task which may not have
    // been scheduled yet when we proceed. Poll the backend's
    // subscription table directly so we know it's hot before we
    // move on — avoids flakiness under parallel test load. Use a
    // generous 10 s budget to tolerate a fully-saturated CI runner
    // (the full workspace test suite has ~1500 tokio tasks alive
    // concurrently during `cargo nextest run --workspace`).
    wait_until(Duration::from_secs(10), "backend sub registered", || {
        let subs = resources_registry_b.sessions_subscribed_to("scene://current");
        if subs.is_empty() { None } else { Some(()) }
    })
    .await;

    // 4. Trigger a scene update on the backend — the real publisher
    //    that emits `notifications/resources/updated` on the backend's
    //    own SSE stream, which the gateway multiplexer must forward.
    resources_registry_b.set_scene(json!({"scene_name": "after_update"}));

    // 5. Wait up to 10 s for the prefixed URI to appear in an
    //    `resources/updated` frame on the gateway SSE stream. The
    //    real propagation path crosses several tokio tasks (backend
    //    notification fan-out → backend SSE broadcast → gateway
    //    subscriber `pump_stream` → `dispatch_resource_updated` →
    //    client sink) and under full workspace-level load each
    //    schedule point can slip by hundreds of ms.
    let deadline = Instant::now() + Duration::from_secs(10);
    let updated_uri = loop {
        let snapshot = events.lock().await.clone();
        if let Some(hit) = snapshot
            .iter()
            .find_map(|frame| extract_resources_updated_uri(frame))
        {
            break hit;
        }
        if Instant::now() > deadline {
            panic!("no notifications/resources/updated within 10s; frames so far: {snapshot:#?}",);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(
        updated_uri, prefixed_scene,
        "gateway must rewrite params.uri to the client-visible prefixed form",
    );

    // 6. Unsubscribe, drive another update, and assert no further
    //    `resources/updated` frame arrives within 2 s.
    let unsub_reply = client
        .post(&gw_url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("mcp-session-id", &session_id)
        .body(
            json!({
                "jsonrpc": "2.0", "id": 3,
                "method": "resources/unsubscribe",
                "params": {"uri": prefixed_scene}
            })
            .to_string(),
        )
        .send()
        .await
        .expect("unsubscribe POST must succeed");
    let unsub_body: Value = serde_json::from_str(&unsub_reply.text().await.unwrap()).unwrap();
    assert!(
        unsub_body.get("error").is_none(),
        "unsubscribe returned error: {unsub_body:#}"
    );

    // Wait for the backend's subscription table to drain.
    wait_until(Duration::from_secs(10), "backend sub cleared", || {
        let subs = resources_registry_b.sessions_subscribed_to("scene://current");
        if subs.is_empty() { Some(()) } else { None }
    })
    .await;

    // Take a baseline of updated-uris seen so far, then push another
    // scene change.
    let baseline: Vec<String> = {
        let snapshot = events.lock().await.clone();
        snapshot
            .iter()
            .filter_map(|f| extract_resources_updated_uri(f))
            .collect()
    };
    resources_registry_b.set_scene(json!({"scene_name": "after_unsubscribe"}));
    tokio::time::sleep(Duration::from_secs(3)).await;
    let post_unsubscribe: Vec<String> = {
        let snapshot = events.lock().await.clone();
        snapshot
            .iter()
            .filter_map(|f| extract_resources_updated_uri(f))
            .collect()
    };
    assert_eq!(
        post_unsubscribe.len(),
        baseline.len(),
        "after unsubscribe, no more resources/updated frames should arrive; \
         baseline={baseline:?} post_unsubscribe={post_unsubscribe:?}",
    );

    sse_task.abort();
    handle_b.shutdown().await;
    handle_a.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gateway_resources_list_changed_fires_when_backend_resource_set_changes() {
    use dcc_mcp_http::{ProducerContent, ResourceError, ResourceProducer, ResourceResult};
    use dcc_mcp_jsonrpc::McpResource;

    /// A toggleable producer: when `enabled` is false its `list()` is
    /// empty so the backend drops the `extra://` URI from its set.
    /// Changing the flag at runtime mutates the backend's aggregated
    /// resource fingerprint, which the gateway watcher must observe.
    struct ToggleProducer {
        enabled: Arc<std::sync::atomic::AtomicBool>,
    }
    impl ResourceProducer for ToggleProducer {
        fn scheme(&self) -> &str {
            "extra"
        }
        fn list(&self) -> Vec<McpResource> {
            if !self.enabled.load(std::sync::atomic::Ordering::SeqCst) {
                return Vec::new();
            }
            vec![McpResource {
                uri: "extra://dynamic".to_string(),
                name: "Dynamic extra".to_string(),
                description: Some("Added at runtime for #732 watcher test".into()),
                mime_type: Some("application/json".to_string()),
            }]
        }
        fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
            if uri == "extra://dynamic" && self.enabled.load(std::sync::atomic::Ordering::SeqCst) {
                Ok(ProducerContent::Text {
                    uri: uri.to_string(),
                    mime_type: "application/json".to_string(),
                    text: "{\"ok\":true}".to_string(),
                })
            } else {
                Err(ResourceError::NotFound(uri.to_string()))
            }
        }
    }

    let registry_dir = tempfile::tempdir().unwrap();
    let gw_port = pick_free_port();
    let action_registry = Arc::new(ActionRegistry::new());
    let cfg = McpHttpConfig::new(0)
        .with_name("maya-listchanged-e2e")
        .with_gateway(gw_port)
        .with_dcc_type("maya")
        .with_registry_dir(registry_dir.path());
    let server = McpHttpServer::new(action_registry, cfg);
    let toggle = Arc::new(std::sync::atomic::AtomicBool::new(false));
    server.resources().add_producer(Arc::new(ToggleProducer {
        enabled: toggle.clone(),
    }));
    let handle = server.start().await.expect("backend must start");
    wait_for_instance_watcher().await;
    assert!(handle.is_gateway);

    let gw_url = format!("http://127.0.0.1:{gw_port}/mcp");
    let client = reqwest::Client::new();
    let session_id = "listchanged-session".to_string();

    // Open an SSE stream first so the initial baseline
    // resources/list_changed (if any) is already consumed before we
    // mutate the producer.
    let sse_resp = client
        .get(&gw_url)
        .header("accept", "text/event-stream")
        .header("mcp-session-id", &session_id)
        .send()
        .await
        .expect("SSE GET must succeed");
    assert!(sse_resp.status().is_success());
    let events = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let events_bg = events.clone();
    let sse_task = tokio::spawn(async move {
        use futures::StreamExt;
        let mut stream = sse_resp.bytes_stream();
        let mut buf = Vec::<u8>::new();
        while let Some(Ok(chunk)) = stream.next().await {
            buf.extend_from_slice(&chunk);
            while let Some(pos) = find_double_newline(&buf) {
                let record = buf.drain(..pos).collect::<Vec<u8>>();
                let _ = buf.drain(..2);
                if let Ok(text) = std::str::from_utf8(&record) {
                    events_bg.lock().await.push(text.to_owned());
                }
            }
        }
    });

    // Wait a full watcher period so the initial fingerprint is
    // captured without firing a list_changed (empty → empty skip rule).
    tokio::time::sleep(Duration::from_secs(4)).await;
    let baseline_count = count_list_changed(&events.lock().await);

    // Flip the producer on — new `extra://` URI appears in the
    // backend's resources/list, the gateway watcher's fingerprint
    // changes, a list_changed should be emitted within ~3 s + jitter.
    toggle.store(true, std::sync::atomic::Ordering::SeqCst);

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let current = count_list_changed(&events.lock().await);
        if current > baseline_count {
            break;
        }
        if Instant::now() > deadline {
            let snapshot = events.lock().await.clone();
            panic!("no resources/list_changed within 15s after producer add; frames={snapshot:#?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    sse_task.abort();
    handle.shutdown().await;
}

// ── Local SSE helpers ────────────────────────────────────────────────────

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

/// If `frame` is a `data: {...notifications/resources/updated...}`
/// line, return the `params.uri` payload.
fn extract_resources_updated_uri(frame: &str) -> Option<String> {
    for line in frame.lines() {
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim_start();
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            continue;
        };
        if value.get("method").and_then(Value::as_str) == Some("notifications/resources/updated") {
            return value
                .get("params")
                .and_then(|p| p.get("uri"))
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
    }
    None
}

fn count_list_changed(frames: &[String]) -> usize {
    frames
        .iter()
        .filter(|f| f.contains("notifications/resources/list_changed"))
        .count()
}
