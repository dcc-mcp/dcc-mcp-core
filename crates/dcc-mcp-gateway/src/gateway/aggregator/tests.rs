use super::*;

#[test]
fn skill_management_tool_defs_cover_all_six_tools() {
    let defs = skill_management_tool_defs();
    let names: Vec<&str> = defs
        .iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
        .collect();
    for expected in [
        "list_skills",
        "search_skills",
        "get_skill_info",
        "load_skill",
        "unload_skill",
    ] {
        assert!(names.contains(&expected), "missing tool def {expected}");
    }
    assert_eq!(defs.len(), 5, "expected exactly 5 skill-management tools");
}

#[test]
fn skill_management_tool_defs_all_declare_input_schema() {
    for def in skill_management_tool_defs() {
        let schema = def.get("inputSchema").expect("inputSchema present");
        assert_eq!(
            schema.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "schema for {} is not an object",
            def.get("name").unwrap()
        );
    }
}

#[test]
fn inject_instance_metadata_adds_annotations_to_object() {
    let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
    let mut value = json!({"existing": "field"});
    inject_instance_metadata(&mut value, &id, "maya");

    let obj = value.as_object().unwrap();
    assert_eq!(obj.get("existing").unwrap(), &json!("field"));
    assert_eq!(obj.get("_instance_id").unwrap(), &json!(id.to_string()));
    assert_eq!(obj.get("_instance_short").unwrap(), &json!("abcdef01"));
    assert_eq!(obj.get("_dcc_type").unwrap(), &json!("maya"));
}

#[test]
fn inject_instance_metadata_is_noop_for_non_objects() {
    let id = Uuid::new_v4();
    // Arrays and scalars cannot receive annotations — the helper must
    // silently skip them rather than panic.
    let mut arr = json!([1, 2, 3]);
    inject_instance_metadata(&mut arr, &id, "blender");
    assert_eq!(arr, json!([1, 2, 3]));

    let mut s = json!("scalar");
    inject_instance_metadata(&mut s, &id, "blender");
    assert_eq!(s, json!("scalar"));
}

#[test]
fn to_text_result_maps_ok_to_success() {
    let (text, is_error) = to_text_result(Ok("payload".to_string()));
    assert_eq!(text, "payload");
    assert!(!is_error);
}

#[test]
fn to_text_result_maps_err_to_error() {
    let (text, is_error) = to_text_result(Err("boom".to_string()));
    assert_eq!(text, "boom");
    assert!(is_error);
}

// ── #320: extract_job_id covers both sync (None) and async (#318) envelopes.

#[test]
fn extract_job_id_reads_structured_content_first() {
    let v = json!({
        "content": [],
        "structuredContent": {"job_id": "job-42", "status": "pending"},
        "isError": false,
    });
    assert_eq!(extract_job_id(&v).as_deref(), Some("job-42"));
}

#[test]
fn extract_job_id_falls_back_to_meta_dcc_jobid() {
    let v = json!({
        "content": [],
        "_meta": {"dcc": {"jobId": "job-99", "parentJobId": null}},
        "isError": false,
    });
    assert_eq!(extract_job_id(&v).as_deref(), Some("job-99"));
}

#[test]
fn extract_job_id_returns_none_for_sync_reply() {
    let v = json!({"content": [{"type": "text", "text": "ok"}], "isError": false});
    assert!(extract_job_id(&v).is_none());
}

// ── #321: async opt-in detection + envelope merging ────────────────

#[test]
fn meta_signals_async_dispatch_picks_up_async_flag() {
    let meta = json!({"dcc": {"async": true}});
    assert!(meta_signals_async_dispatch(Some(&meta)));
}

#[test]
fn meta_signals_async_dispatch_picks_up_progress_token() {
    let meta = json!({"progressToken": "tok"});
    assert!(meta_signals_async_dispatch(Some(&meta)));
}

#[test]
fn meta_signals_async_dispatch_is_false_for_sync_requests() {
    assert!(!meta_signals_async_dispatch(None));
    let meta = json!({"dcc": {"parentJobId": "abc"}});
    assert!(!meta_signals_async_dispatch(Some(&meta)));
}

#[test]
fn meta_wants_wait_for_terminal_reads_dcc_flag() {
    let meta = json!({"dcc": {"async": true, "wait_for_terminal": true}});
    assert!(meta_wants_wait_for_terminal(Some(&meta)));

    let meta = json!({"dcc": {"async": true}});
    assert!(!meta_wants_wait_for_terminal(Some(&meta)));
}

#[test]
fn strip_gateway_meta_flags_removes_wait_for_terminal_only() {
    let meta = json!({"dcc": {"async": true, "wait_for_terminal": true, "parentJobId": "p"}});
    let stripped = strip_gateway_meta_flags(meta);
    assert_eq!(stripped["dcc"]["async"], true);
    assert_eq!(stripped["dcc"]["parentJobId"], "p");
    assert!(stripped["dcc"].get("wait_for_terminal").is_none());
}

#[test]
fn merge_job_update_into_envelope_completed_sets_status_and_result() {
    let pending = json!({
        "content": [{"type": "text", "text": "Job x queued"}],
        "structuredContent": {"job_id": "x", "status": "pending", "_meta": {"dcc": {"jobId": "x"}}},
        "isError": false,
    });
    let update = json!({
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "x", "status": "completed", "result": {"rows": 42}}
    });
    let merged = merge_job_update_into_envelope(pending, &update, false);
    assert_eq!(merged["structuredContent"]["status"], "completed");
    assert_eq!(merged["structuredContent"]["result"]["rows"], 42);
    assert_eq!(
        merged["structuredContent"]["_meta"]["dcc"]["status"],
        "completed"
    );
    assert_eq!(merged["isError"], false);
}

#[test]
fn merge_job_update_into_envelope_failed_marks_is_error() {
    let pending = json!({
        "content": [{"type": "text", "text": "Job x queued"}],
        "structuredContent": {"job_id": "x", "status": "pending"},
        "isError": false,
    });
    let update = json!({
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "x", "status": "failed", "error": "boom"}
    });
    let merged = merge_job_update_into_envelope(pending, &update, false);
    assert_eq!(merged["structuredContent"]["status"], "failed");
    assert_eq!(merged["structuredContent"]["error"], "boom");
    assert_eq!(merged["isError"], true);
}

#[test]
fn merge_job_update_into_envelope_timeout_sets_timed_out_flag() {
    let pending = json!({
        "content": [{"type": "text", "text": "Job x queued"}],
        "structuredContent": {"job_id": "x", "status": "pending"},
        "isError": false,
    });
    let merged = merge_job_update_into_envelope(pending, &Value::Null, true);
    assert_eq!(
        merged["structuredContent"]["_meta"]["dcc"]["timed_out"],
        true
    );
    assert_eq!(merged["isError"], true);
}

#[tokio::test]
async fn aggregate_tools_list_does_not_publish_single_instance_bare_aliases() {
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(|| async {
                axum::Json(json!({
                    "jsonrpc": "2.0",
                    "id": "gw-1",
                    "result": {
                        "tools": [
                            {"name": "create_sphere", "description": "Create sphere", "inputSchema": {"type": "object"}}
                        ]
                    }
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let instance_id = {
        let r = registry.read().await;
        let entry =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("maya", "127.0.0.1", port);
        let id = entry.instance_id;
        r.register(entry).unwrap();
        id
    };
    let (yield_tx, _) = tokio::sync::watch::channel(false);
    let (events_tx, _) = tokio::sync::broadcast::channel::<String>(8);
    let gs = crate::gateway::GatewayState {
        registry,
        stale_timeout: std::time::Duration::from_secs(30),
        backend_timeout: std::time::Duration::from_secs(10),
        async_dispatch_timeout: std::time::Duration::from_secs(60),
        wait_terminal_timeout: std::time::Duration::from_secs(600),
        server_name: "test".into(),
        server_version: env!("CARGO_PKG_VERSION").into(),
        own_host: "127.0.0.1".into(),
        own_port: 0,
        http_client: reqwest::Client::new(),
        yield_tx: std::sync::Arc::new(yield_tx),
        events_tx: std::sync::Arc::new(events_tx),
        protocol_version: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        resource_subscriptions: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        pending_calls: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        adapter_version: None,
        adapter_dcc: None,
        tool_exposure: crate::gateway::GatewayToolExposure::Rest,
        cursor_safe_tool_names: true,
        capability_index: std::sync::Arc::new(crate::gateway::capability::CapabilityIndex::new()),
    };

    assert_eq!(gs.live_instances(&*gs.registry.read().await).len(), 1);

    let result = aggregate_tools_list(&gs, None).await;
    let names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();

    // In Rest mode (default), backend tools ARE fanned out under the
    // cursor-safe `i_<id8>__<name>` form, but bare aliases are not
    // published (the encoded form is the single canonical identifier).
    let prefix = format!("i_{}__", &instance_id.to_string().replace('-', "")[..8]);
    assert!(
        names.iter().any(|name| name.starts_with(&prefix)),
        "Rest mode must fan out backend tools with the cursor-safe prefix: {names:?}"
    );
    assert!(
        !names.contains(&"create_sphere"),
        "bare backend alias must not be published: {names:?}"
    );

    let _ = shutdown_tx.send(());
    server.await.unwrap();
}

// ── #731: prompts/list + prompts/get aggregation ─────────────────────
//
// Mirror of the tools aggregation tests above. Two fake backends with
// disjoint prompt sets exercise:
//   1. `aggregate_prompts_list` returns the merged set with correct
//      per-backend cursor-safe prefixes.
//   2. `route_prompts_get` decodes the prefix and routes to the
//      owning backend.
//   3. Zero-backend gateway returns `{"prompts": []}` instead of an
//      error — a hard acceptance criterion from the issue.

/// Spawn a tiny axum server that answers both `tools/list` (empty) and
/// `prompts/list` / `prompts/get` with canned fixtures.
///
/// The caller supplies the per-backend prompt name and a marker text
/// that the `prompts/get` route echoes back so we can assert the
/// request landed on the intended backend.
async fn spawn_prompts_backend(
    prompt_name: &'static str,
    echo_text: &'static str,
) -> (String, tokio::sync::oneshot::Sender<()>) {
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |body: axum::Json<Value>| async move {
                let method = body.get("method").and_then(Value::as_str).unwrap_or("");
                let id = body.get("id").cloned().unwrap_or(json!("gw-1"));
                let result: Value = match method {
                    "tools/list" => json!({"tools": []}),
                    "prompts/list" => json!({
                        "prompts": [{
                            "name": prompt_name,
                            "description": format!("Prompt from {echo_text}"),
                            "arguments": [],
                        }]
                    }),
                    "prompts/get" => {
                        let requested = body
                            .get("params")
                            .and_then(|p| p.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        json!({
                            "description": format!("Echo from {echo_text}"),
                            "messages": [{
                                "role": "user",
                                "content": {
                                    "type": "text",
                                    "text": format!("{echo_text}:{requested}"),
                                }
                            }]
                        })
                    }
                    _ => json!({}),
                };
                axum::Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result,
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (format!("127.0.0.1:{port}"), tx)
}

/// Build a GatewayState with the supplied registry — same shape as the
/// tools-aggregator test helper but extracted for reuse.
async fn make_gateway_state(
    registry: std::sync::Arc<
        tokio::sync::RwLock<dcc_mcp_transport::discovery::file_registry::FileRegistry>,
    >,
) -> crate::gateway::GatewayState {
    let (yield_tx, _) = tokio::sync::watch::channel(false);
    let (events_tx, _) = tokio::sync::broadcast::channel::<String>(8);
    crate::gateway::GatewayState {
        registry,
        stale_timeout: std::time::Duration::from_secs(30),
        backend_timeout: std::time::Duration::from_secs(10),
        async_dispatch_timeout: std::time::Duration::from_secs(60),
        wait_terminal_timeout: std::time::Duration::from_secs(600),
        server_name: "test".into(),
        server_version: env!("CARGO_PKG_VERSION").into(),
        own_host: "127.0.0.1".into(),
        own_port: 0,
        http_client: reqwest::Client::new(),
        yield_tx: std::sync::Arc::new(yield_tx),
        events_tx: std::sync::Arc::new(events_tx),
        protocol_version: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        resource_subscriptions: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        pending_calls: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        adapter_version: None,
        adapter_dcc: None,
        tool_exposure: crate::gateway::GatewayToolExposure::Rest,
        cursor_safe_tool_names: true,
        capability_index: std::sync::Arc::new(crate::gateway::capability::CapabilityIndex::new()),
    }
}

#[tokio::test]
async fn aggregate_prompts_list_zero_backends_returns_empty_array() {
    // Acceptance criterion: a gateway with no live backends must return
    // `{"prompts": []}` — never `Method not found`.
    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let gs = make_gateway_state(registry).await;

    let result = aggregate_prompts_list(&gs).await;
    assert_eq!(result["prompts"], json!([]));
}

#[tokio::test]
async fn aggregate_prompts_list_merges_and_prefixes_across_backends() {
    let (addr_a, stop_a) = spawn_prompts_backend("bake_animation", "maya-A").await;
    let (addr_b, stop_b) = spawn_prompts_backend("render_frame", "blender-B").await;

    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let (iid_a, iid_b) = {
        let r = registry.read().await;
        let (host_a, port_a) = parse_addr(&addr_a);
        let (host_b, port_b) = parse_addr(&addr_b);
        let entry_a =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("maya", host_a, port_a);
        let entry_b =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("blender", host_b, port_b);
        let ia = entry_a.instance_id;
        let ib = entry_b.instance_id;
        r.register(entry_a).unwrap();
        r.register(entry_b).unwrap();
        (ia, ib)
    };

    let gs = make_gateway_state(registry).await;
    let result = aggregate_prompts_list(&gs).await;

    let names: Vec<String> = result["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect();

    let short_a = &iid_a.to_string().replace('-', "")[..8];
    let short_b = &iid_b.to_string().replace('-', "")[..8];
    let expected_a = format!("i_{short_a}__bake_U_animation");
    let expected_b = format!("i_{short_b}__render_U_frame");

    assert!(
        names.iter().any(|n| n == &expected_a),
        "expected {expected_a} in {names:?}"
    );
    assert!(
        names.iter().any(|n| n == &expected_b),
        "expected {expected_b} in {names:?}"
    );
    assert_eq!(names.len(), 2, "merged list must be the union: {names:?}");

    let _ = stop_a.send(());
    let _ = stop_b.send(());
}

#[tokio::test]
async fn route_prompts_get_decodes_prefix_and_routes_to_owning_backend() {
    let (addr_a, stop_a) = spawn_prompts_backend("bake_animation", "maya-A").await;
    let (addr_b, stop_b) = spawn_prompts_backend("render_frame", "blender-B").await;

    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let (iid_a, iid_b) = {
        let r = registry.read().await;
        let (host_a, port_a) = parse_addr(&addr_a);
        let (host_b, port_b) = parse_addr(&addr_b);
        let entry_a =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("maya", host_a, port_a);
        let entry_b =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("blender", host_b, port_b);
        let ia = entry_a.instance_id;
        let ib = entry_b.instance_id;
        r.register(entry_a).unwrap();
        r.register(entry_b).unwrap();
        (ia, ib)
    };

    let gs = make_gateway_state(registry).await;
    let short_a = &iid_a.to_string().replace('-', "")[..8];
    let short_b = &iid_b.to_string().replace('-', "")[..8];
    let wire_a = format!("i_{short_a}__bake_U_animation");
    let wire_b = format!("i_{short_b}__render_U_frame");

    let res_a = route_prompts_get(&gs, &wire_a, None, Some("rid-a".into()))
        .await
        .expect("routing to backend A must succeed");
    let echo_a = res_a["messages"][0]["content"]["text"].as_str().unwrap();
    assert_eq!(
        echo_a, "maya-A:bake_animation",
        "backend A must have seen the decoded bare name"
    );

    let res_b = route_prompts_get(&gs, &wire_b, None, Some("rid-b".into()))
        .await
        .expect("routing to backend B must succeed");
    let echo_b = res_b["messages"][0]["content"]["text"].as_str().unwrap();
    assert_eq!(echo_b, "blender-B:render_frame");

    let _ = stop_a.send(());
    let _ = stop_b.send(());
}

#[tokio::test]
async fn route_prompts_get_with_unknown_prefix_returns_routing_error() {
    // `decode_tool_name` succeeds (valid 8-hex prefix shape) but no
    // backend owns that prefix — this path must surface a -32602
    // without touching any backend.
    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let gs = make_gateway_state(registry).await;

    let err = route_prompts_get(&gs, "i_deadbeef__whatever", None, None)
        .await
        .expect_err("unknown prefix must fail");
    assert_eq!(err.code(), -32602);
    assert!(err.message().contains("deadbeef"), "msg: {}", err.message());
}

/// Parse `127.0.0.1:12345` back into `(host, port)`.
fn parse_addr(addr: &str) -> (&str, u16) {
    let (h, p) = addr.rsplit_once(':').unwrap();
    (h, p.parse().unwrap())
}

#[tokio::test]
async fn gateway_mcp_initialize_advertises_prompts_capability() {
    // End-to-end contract check: POST /mcp initialize must include
    // `prompts: { listChanged: true }` in its capabilities object —
    // hard acceptance criterion for issue #731.
    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let gs = make_gateway_state(registry).await;
    let router = crate::gateway::build_gateway_router(gs);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let resp: Value = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-03-26"}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let caps = &resp["result"]["capabilities"];
    assert_eq!(
        caps["prompts"]["listChanged"],
        json!(true),
        "initialize response must advertise prompts.listChanged=true: {caps}"
    );

    // Zero-backend prompts/list MUST return `{"prompts": []}`, not a
    // -32601 Method not found (issue #731 acceptance criterion).
    let resp: Value = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .json(&json!({
            "jsonrpc": "2.0", "id": 2, "method": "prompts/list"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(resp.get("error").is_none(), "must not be an error: {resp}");
    assert_eq!(resp["result"]["prompts"], json!([]));

    let _ = shutdown_tx.send(());
    server.await.unwrap();
}

#[tokio::test]
async fn compute_prompts_fingerprint_changes_when_backend_prompt_set_mutates() {
    // The prompts watcher task broadcasts
    // `notifications/prompts/list_changed` iff this fingerprint
    // differs between polls. Verify that swapping a backend's
    // published prompt produces a different fingerprint — this is
    // the hysteresis unit the watcher's broadcast relies on.
    use std::sync::{Arc, Mutex};

    let state: Arc<Mutex<&'static str>> = Arc::new(Mutex::new("bake_animation"));
    let state_clone = state.clone();

    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |body: axum::Json<Value>| {
                let state = state_clone.clone();
                async move {
                    let method = body.get("method").and_then(Value::as_str).unwrap_or("");
                    let id = body.get("id").cloned().unwrap_or(json!("gw-1"));
                    let result: Value = match method {
                        "prompts/list" => {
                            let name = *state.lock().unwrap();
                            json!({
                                "prompts": [{
                                    "name": name,
                                    "description": "dynamic",
                                    "arguments": [],
                                }]
                            })
                        }
                        _ => json!({"tools": []}),
                    };
                    axum::Json(json!({"jsonrpc":"2.0","id":id,"result":result}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    {
        let r = registry.read().await;
        let entry =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("maya", "127.0.0.1", port);
        r.register(entry).unwrap();
    }
    let client = reqwest::Client::new();

    let fp_before = compute_prompts_fingerprint(
        &registry,
        std::time::Duration::from_secs(30),
        &client,
        std::time::Duration::from_secs(2),
    )
    .await;
    assert!(
        fp_before.contains("bake_animation"),
        "initial fingerprint should include the first prompt name: {fp_before}"
    );

    // Swap the prompt set on the backend and re-fingerprint — the
    // aggregated string must change, which is what drives the
    // watcher's broadcast decision.
    *state.lock().unwrap() = "render_frame";
    let fp_after = compute_prompts_fingerprint(
        &registry,
        std::time::Duration::from_secs(30),
        &client,
        std::time::Duration::from_secs(2),
    )
    .await;
    assert!(
        fp_after.contains("render_frame"),
        "post-swap fingerprint should include new prompt: {fp_after}"
    );
    assert_ne!(
        fp_before, fp_after,
        "mutation in backend prompt set must produce a different fingerprint"
    );

    let _ = shutdown_tx.send(());
    server.await.unwrap();
}

// ── #732: resources/list aggregation ──────────────────────────────────

/// Spawn a fake backend that answers `/health` green and serves a canned
/// `resources/list` payload. Returns `(port, shutdown_tx)`.
async fn spawn_resources_backend(resources: Vec<Value>) -> (u16, tokio::sync::oneshot::Sender<()>) {
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post({
                let resources = resources.clone();
                move |body: axum::Json<Value>| {
                    let resources = resources.clone();
                    async move {
                        let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");
                        let id = body.get("id").cloned().unwrap_or(json!("gw-test"));
                        let result = match method {
                            "resources/list" => json!({"resources": resources}),
                            _ => json!({}),
                        };
                        axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (port, tx)
}

/// Build a GatewayState around a shared registry, pre-filled with the
/// given `(dcc_type, port)` rows. Returns `(state, instance_ids)`.
async fn gateway_state_with_instances(
    instances: &[(&str, u16)],
) -> (
    crate::gateway::GatewayState,
    tempfile::TempDir,
    Vec<uuid::Uuid>,
) {
    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let mut ids = Vec::new();
    {
        let r = registry.read().await;
        for (dcc_type, port) in instances {
            let entry = dcc_mcp_transport::discovery::types::ServiceEntry::new(
                *dcc_type,
                "127.0.0.1",
                *port,
            );
            ids.push(entry.instance_id);
            r.register(entry).unwrap();
        }
    }
    let (yield_tx, _) = tokio::sync::watch::channel(false);
    let (events_tx, _) = tokio::sync::broadcast::channel::<String>(8);
    let state = crate::gateway::GatewayState {
        registry,
        stale_timeout: std::time::Duration::from_secs(30),
        backend_timeout: std::time::Duration::from_secs(10),
        async_dispatch_timeout: std::time::Duration::from_secs(60),
        wait_terminal_timeout: std::time::Duration::from_secs(600),
        server_name: "test".into(),
        server_version: env!("CARGO_PKG_VERSION").into(),
        own_host: "127.0.0.1".into(),
        own_port: 0,
        http_client: reqwest::Client::new(),
        yield_tx: std::sync::Arc::new(yield_tx),
        events_tx: std::sync::Arc::new(events_tx),
        protocol_version: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        resource_subscriptions: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        pending_calls: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        adapter_version: None,
        adapter_dcc: None,
        tool_exposure: crate::gateway::GatewayToolExposure::Rest,
        cursor_safe_tool_names: true,
        capability_index: std::sync::Arc::new(crate::gateway::capability::CapabilityIndex::new()),
    };
    (state, dir, ids)
}

fn id8(id: &uuid::Uuid) -> String {
    let mut s = id.simple().to_string();
    s.truncate(8);
    s
}

#[tokio::test]
async fn aggregate_resources_list_merges_admin_pointers_and_backend_resources() {
    // Two backends, disjoint resource sets. The gateway's
    // resources/list must return admin pointers ∪ each backend's
    // resources with the per-instance prefix.
    let (port_a, stop_a) = spawn_resources_backend(vec![
        json!({"uri": "scene://current", "name": "A scene", "mimeType": "application/json"}),
    ])
    .await;
    let (port_b, stop_b) = spawn_resources_backend(vec![
        json!({"uri": "capture://current_window", "name": "B capture", "mimeType": "image/png"}),
        json!({"uri": "audit://recent", "name": "B audit", "mimeType": "application/json"}),
    ])
    .await;

    let (gs, _dir, ids) =
        gateway_state_with_instances(&[("maya", port_a), ("blender", port_b)]).await;
    let id_a = ids[0];
    let id_b = ids[1];

    let result = aggregate_resources_list(&gs).await;
    let resources = result["resources"]
        .as_array()
        .expect("resources must be an array");
    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();

    // Admin pointers: one per instance.
    assert!(
        uris.iter().any(|u| u.starts_with("dcc://maya/")),
        "admin pointer for maya instance missing: {uris:?}",
    );
    assert!(
        uris.iter().any(|u| u.starts_with("dcc://blender/")),
        "admin pointer for blender instance missing: {uris:?}",
    );

    // Backend resources with prefix.
    let prefix_a = id8(&id_a);
    let prefix_b = id8(&id_b);
    assert!(
        uris.contains(&&*format!("scene://{prefix_a}/current")),
        "prefixed scene URI missing: {uris:?}",
    );
    assert!(
        uris.contains(&&*format!("capture://{prefix_b}/current_window")),
        "prefixed capture URI missing: {uris:?}",
    );
    assert!(
        uris.contains(&&*format!("audit://{prefix_b}/recent")),
        "prefixed audit URI missing: {uris:?}",
    );

    // No unprefixed backend URIs — the gateway must not leak raw
    // backend URIs that would collide across instances.
    assert!(
        !uris.contains(&"scene://current"),
        "unprefixed backend URI leaked: {uris:?}",
    );

    let _ = stop_a.send(());
    let _ = stop_b.send(());
}

#[tokio::test]
async fn aggregate_resources_list_fail_soft_when_one_backend_is_dead() {
    // One backend answers normally; the other's port is closed. The
    // gateway must still return the healthy backend's resources plus
    // the admin pointer for the dead backend — a dead backend does
    // not take down the whole list.
    let (port_live, stop_live) = spawn_resources_backend(vec![
        json!({"uri": "scene://current", "name": "live scene", "mimeType": "application/json"}),
    ])
    .await;
    // Pick a port that almost certainly has nothing listening.
    let dead_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = dead_listener.local_addr().unwrap().port();
    drop(dead_listener); // close it — now no one's home.

    let (gs, _dir, ids) =
        gateway_state_with_instances(&[("maya", port_live), ("blender", dead_port)]).await;
    let id_live = ids[0];

    let result = aggregate_resources_list(&gs).await;
    let resources = result["resources"]
        .as_array()
        .expect("resources must be an array");
    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();

    // Live backend's resource is present.
    assert!(
        uris.contains(&&*format!("scene://{}/current", id8(&id_live))),
        "live backend's prefixed URI missing: {uris:?}",
    );
    // Admin pointers for both instances are still present (fail-soft:
    // the registry row survives).
    assert!(
        uris.iter().any(|u| u.starts_with("dcc://maya/")),
        "live maya admin pointer missing: {uris:?}",
    );
    assert!(
        uris.iter().any(|u| u.starts_with("dcc://blender/")),
        "dead blender admin pointer missing: {uris:?}",
    );

    let _ = stop_live.send(());
}
