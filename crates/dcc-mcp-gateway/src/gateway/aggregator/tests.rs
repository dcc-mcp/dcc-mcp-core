use super::skill_mgmt::skill_management_tool_defs;
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
        "activate_tool_group",
        "deactivate_tool_group",
    ] {
        assert!(names.contains(&expected), "missing tool def {expected}");
    }
    assert_eq!(defs.len(), 7, "expected exactly 7 skill-management tools");
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

#[tokio::test]
async fn aggregate_tools_list_returns_only_minimal_gateway_surface() {
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
        http_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::http_registration::HttpInstanceRegistry::default(),
        )),
        mdns_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::mdns_discovery::MdnsInstanceRegistry::default(),
        )),
        relay_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::relay_discovery::RelayInstanceRegistry::default(),
        )),
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
        client_attribution: std::sync::Arc::new(
            crate::gateway::caller_attribution::ClientAttributionStore::default(),
        ),
        pending_calls: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        policy: std::sync::Arc::new(crate::gateway::GatewayPolicy::default()),
        security: std::sync::Arc::new(crate::gateway::GatewaySecurityPolicy::disabled()),
        adapter_version: None,
        adapter_dcc: None,
        capability_index: std::sync::Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        event_log: std::sync::Arc::new(crate::gateway::event_log::EventLog::new()),
        #[cfg(feature = "prometheus")]
        gateway_metrics: std::sync::Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        middleware_chain: std::sync::Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
        instance_diagnostics: std::sync::Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: std::sync::Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        search_telemetry: std::sync::Arc::new(
            crate::gateway::search_telemetry::SearchTelemetryStore::new(),
        ),
        debug_routes_enabled: false,
    };

    assert_eq!(gs.live_instances(&*gs.registry.read().await).len(), 1);

    let result = aggregate_tools_list(&gs, None).await;
    let names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();

    // The gateway MCP surface is the canonical four-tool workflow. Per-action
    // backend tools are NOT published here — agents discover them through
    // `search` / `describe`, activate with `load_skill`, then execute via
    // `call` (or the equivalent REST plane).
    let prefix = format!("i_{}__", &instance_id.to_string().replace('-', "")[..8]);
    assert!(
        !names.iter().any(|name| name.starts_with(&prefix)),
        "gateway must not fan out backend tools under any prefix: {names:?}"
    );
    assert!(
        !names.contains(&"create_sphere"),
        "bare backend tool name must not appear on the gateway surface: {names:?}"
    );
    // Positive assertion: advertised gateway MCP surface is bounded and stable.
    for expected in ["search", "describe", "load_skill", "call"] {
        assert!(
            names.contains(&expected),
            "missing core gateway tool {expected} in: {names:?}",
        );
    }
    assert_eq!(
        names.len(),
        4,
        "gateway tools/list must expose exactly the four workflow tools: {names:?}"
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
/// After #818 phase 2 the gateway contacts backends over REST (`/v1/*`),
/// so the mock serves `GET /v1/prompts` and `GET /v1/prompts/{name}`.
///
/// The caller supplies the per-backend prompt name and a marker text
/// that the `GET /v1/prompts/{name}` route echoes back so we can assert
/// the request landed on the intended backend.
async fn spawn_prompts_backend(
    prompt_name: &'static str,
    echo_text: &'static str,
) -> (String, tokio::sync::oneshot::Sender<()>) {
    use axum::extract::Path;
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        // GET /v1/prompts — list all prompts (REST, replaces prompts/list JSON-RPC)
        .route(
            "/v1/prompts",
            axum::routing::get(move || async move {
                axum::Json(json!({
                    "total": 1,
                    "prompts": [{
                        "name": prompt_name,
                        "description": format!("Prompt from {echo_text}"),
                        "arguments": [],
                    }]
                }))
            }),
        )
        // GET /v1/prompts/{name} — render a single prompt (REST, replaces prompts/get JSON-RPC)
        .route(
            "/v1/prompts/{name}",
            axum::routing::get(move |Path(requested): Path<String>| async move {
                axum::Json(json!({
                    "description": format!("Echo from {echo_text}"),
                    "messages": [{
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!("{echo_text}:{requested}"),
                        }
                    }]
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
        http_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::http_registration::HttpInstanceRegistry::default(),
        )),
        mdns_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::mdns_discovery::MdnsInstanceRegistry::default(),
        )),
        relay_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::relay_discovery::RelayInstanceRegistry::default(),
        )),
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
        client_attribution: std::sync::Arc::new(
            crate::gateway::caller_attribution::ClientAttributionStore::default(),
        ),
        pending_calls: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        policy: std::sync::Arc::new(crate::gateway::GatewayPolicy::default()),
        security: std::sync::Arc::new(crate::gateway::GatewaySecurityPolicy::disabled()),
        adapter_version: None,
        adapter_dcc: None,
        capability_index: std::sync::Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        event_log: std::sync::Arc::new(crate::gateway::event_log::EventLog::new()),
        #[cfg(feature = "prometheus")]
        gateway_metrics: std::sync::Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        middleware_chain: std::sync::Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
        instance_diagnostics: std::sync::Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: std::sync::Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        search_telemetry: std::sync::Arc::new(
            crate::gateway::search_telemetry::SearchTelemetryStore::new(),
        ),
        debug_routes_enabled: false,
    }
}

async fn spawn_canonical_workflow_backend() -> (u16, tokio::sync::oneshot::Sender<()>) {
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/v1/search",
            axum::routing::post(|| async {
                axum::Json(json!({
                    "total": 1,
                    "hits": [{
                        "action": "create_sphere",
                        "skill": "maya-primitives",
                        "summary": "Create a polygon sphere in the current scene.",
                        "loaded": true,
                        "has_schema": true,
                        "annotations": {
                            "readOnlyHint": false,
                            "destructiveHint": false,
                            "openWorldHint": true
                        },
                        "metadata": {
                            "dcc": {
                                "affinity": "main",
                                "execution": "in-process"
                            }
                        }
                    }]
                }))
            }),
        )
        .route(
            "/v1/describe",
            axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
                let tool_slug = body
                    .get("tool_slug")
                    .and_then(Value::as_str)
                    .unwrap_or("create_sphere");
                axum::Json(json!({
                    "entry": {
                        "slug": tool_slug,
                        "skill": "maya-primitives",
                        "action": "create_sphere",
                        "dcc": "maya",
                        "loaded": true
                    },
                    "description": "Create a polygon sphere in the current scene.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "radius": {"type": "number", "minimum": 0.0}
                        },
                        "required": ["radius"]
                    },
                    "annotations": {
                        "readOnlyHint": false,
                        "destructiveHint": false,
                        "openWorldHint": true
                    },
                    "metadata": {
                        "dcc": {
                            "affinity": "main",
                            "execution": "in-process"
                        }
                    }
                }))
            }),
        )
        .route(
            "/v1/call",
            axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
                axum::Json(json!({
                    "content": [{
                        "type": "text",
                        "text": format!(
                            "called {} with {}",
                            body.get("tool_slug").and_then(Value::as_str).unwrap_or(""),
                            body.get("arguments").cloned().unwrap_or_else(|| json!({}))
                        )
                    }],
                    "isError": false
                }))
            }),
        )
        .route(
            "/mcp",
            axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
                let id = body.get("id").cloned().unwrap_or(Value::Null);
                let name = body
                    .get("params")
                    .and_then(|params| params.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let result = if name == "load_skill" {
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "loaded": true,
                                "skill_name": "maya-primitives",
                                "dcc_type": "maya",
                                "activated_groups": ["core"],
                            })).unwrap()
                        }],
                        "isError": false
                    })
                } else {
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("unexpected backend MCP tool: {name}")
                        }],
                        "isError": true
                    })
                };
                axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
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
    (port, tx)
}

async fn post_mcp_json(client: &reqwest::Client, url: &str, body: Value) -> Value {
    client
        .post(url)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

fn mcp_call_text_json(response: &Value) -> Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("tools/call response text");
    serde_json::from_str(text).unwrap_or_else(|_| json!({"text": text}))
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
async fn aggregate_prompts_list_reports_failed_backend_without_hiding_healthy_prompts() {
    let (addr_a, stop_a) = spawn_prompts_backend("bake_animation", "maya-A").await;
    let (addr_b, stop_b) = spawn_prompts_backend("render_frame", "blender-B").await;

    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    {
        let r = registry.read().await;
        let (host_a, port_a) = parse_addr(&addr_a);
        let (host_b, port_b) = parse_addr(&addr_b);
        r.register(dcc_mcp_transport::discovery::types::ServiceEntry::new(
            "maya", host_a, port_a,
        ))
        .unwrap();
        r.register(dcc_mcp_transport::discovery::types::ServiceEntry::new(
            "blender", host_b, port_b,
        ))
        .unwrap();
    }

    let _ = stop_b.send(());
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let gs = make_gateway_state(registry).await;
    let result = aggregate_prompts_list(&gs).await;

    let prompts = result["prompts"].as_array().unwrap();
    assert_eq!(
        prompts.len(),
        1,
        "healthy backend prompt must remain visible"
    );
    assert!(
        prompts[0]["name"]
            .as_str()
            .unwrap()
            .ends_with("__bake_U_animation")
    );
    let diagnostics = &result["_meta"]["dcc.prompt_diagnostics"];
    assert_eq!(diagnostics["failed_backend_count"], json!(1));
    assert_eq!(diagnostics["prompt_count"], json!(1));
    assert!(
        diagnostics["backends"]
            .as_array()
            .unwrap()
            .iter()
            .any(|backend| backend["status"] == json!("error"))
    );

    let _ = stop_a.send(());
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
async fn gateway_mcp_four_tool_workflow_covers_search_describe_load_and_call() {
    let (backend_port, stop_backend) = spawn_canonical_workflow_backend().await;
    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    {
        let r = registry.read().await;
        let mut entry = dcc_mcp_transport::discovery::types::ServiceEntry::new(
            "maya",
            "127.0.0.1",
            backend_port,
        );
        entry.instance_id = uuid::Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        r.register(entry).unwrap();
    }

    let gs = make_gateway_state(registry).await;
    let router = crate::gateway::build_gateway_router(gs);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway_port = listener.local_addr().unwrap().port();
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
    let url = format!("http://127.0.0.1:{gateway_port}/mcp");

    let list = post_mcp_json(
        &client,
        &url,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(names, ["search", "describe", "load_skill", "call"]);

    let search = post_mcp_json(
        &client,
        &url,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "search",
                "arguments": {"query": "sphere", "dcc_type": "maya", "limit": 5}
            }
        }),
    )
    .await;
    assert_eq!(search["result"]["isError"], false);
    let search_payload = mcp_call_text_json(&search);
    let tool_slug = search_payload["hits"][0]["tool_slug"]
        .as_str()
        .expect("search returns a tool_slug")
        .to_string();

    let describe = post_mcp_json(
        &client,
        &url,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "describe",
                "arguments": {"tool_slug": tool_slug.clone()}
            }
        }),
    )
    .await;
    assert_eq!(describe["result"]["isError"], false);
    let describe_payload = mcp_call_text_json(&describe);
    assert_eq!(describe_payload["required"], json!(["radius"]));
    assert_eq!(
        describe_payload["tool"]["inputSchema"]["properties"]["radius"]["type"],
        "number"
    );

    let load = post_mcp_json(
        &client,
        &url,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "load_skill",
                "arguments": {"skill_name": "maya-primitives", "dcc_type": "maya"}
            }
        }),
    )
    .await;
    assert_eq!(load["result"]["isError"], false);
    let load_payload = mcp_call_text_json(&load);
    assert_eq!(load_payload["loaded"], true);
    assert_eq!(load_payload["skill_name"], "maya-primitives");
    assert_eq!(load_payload["dcc_type"], "maya");
    assert_eq!(
        load_payload["instance_id"],
        "aaaaaaaa-0000-0000-0000-000000000001"
    );
    assert_eq!(load_payload["activated_groups"], json!(["core"]));
    assert_eq!(
        load_payload["new_tool_slugs"][0],
        "maya.aaaaaaaa.create_sphere"
    );
    assert!(load_payload["index_generation"].as_str().is_some());
    assert_eq!(load_payload["next_step"]["action"], "describe");
    assert_eq!(load_payload["next_step"]["mcp"]["tool"], "describe");
    assert_eq!(load_payload["next_step"]["rest"]["path"], "/v1/describe");

    let single = post_mcp_json(
        &client,
        &url,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "call",
                "arguments": {"tool_slug": tool_slug.clone(), "arguments": {"radius": 2.0}}
            }
        }),
    )
    .await;
    assert_eq!(single["result"]["isError"], false);
    let single_payload = mcp_call_text_json(&single);
    assert_eq!(single_payload["isError"], false);
    assert!(
        single_payload["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("radius")
    );

    let batch = post_mcp_json(
        &client,
        &url,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "call",
                "arguments": {
                    "calls": [
                        {"tool_slug": tool_slug.clone(), "arguments": {"radius": 1.0}},
                        {"tool_slug": tool_slug, "arguments": {"radius": 3.0}}
                    ],
                    "stop_on_error": true
                }
            }
        }),
    )
    .await;
    assert_eq!(batch["result"]["isError"], false);
    let batch_payload = mcp_call_text_json(&batch);
    assert_eq!(batch_payload["success"], true);
    assert_eq!(batch_payload["results"].as_array().unwrap().len(), 2);

    let _ = shutdown_tx.send(());
    server.await.unwrap();
    let _ = stop_backend.send(());
}

#[tokio::test]
async fn gateway_mcp_concurrent_initialize_completes_within_one_second() {
    // Issue #1009 — N concurrent initialize handshakes must not queue past 1s
    // on an idle gateway (no DCC backends, no lock contention).
    use std::time::{Duration, Instant};

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
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}/mcp");
    let concurrent = 16usize;
    let started = Instant::now();
    let responses = futures::future::join_all((0..concurrent).map(|id| {
        let client = client.clone();
        let url = url.clone();
        async move {
            client
                .post(&url)
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "initialize",
                    "params": {"protocolVersion": "2025-03-26"}
                }))
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    }))
    .await;

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "concurrent initialize took {:?}",
        started.elapsed()
    );
    for (idx, resp) in responses.iter().enumerate() {
        assert!(
            resp.get("result").is_some(),
            "initialize[{idx}] failed: {resp}"
        );
    }

    let _ = shutdown_tx.send(());
    server.await.unwrap();
}

#[tokio::test]
async fn gateway_mcp_initialize_does_not_wait_for_protocol_cache_lock() {
    // The protocol-version cache is diagnostic state, not a handshake
    // dependency. If another task holds the lock, initialize must still
    // complete normally so multiple MCP clients do not queue behind it.
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let gs = make_gateway_state(registry).await;
    let lock = gs.protocol_version.clone();
    let hold = tokio::spawn(async move {
        let _guard = lock.write().await;
        tokio::time::sleep(Duration::from_secs(6)).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

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
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let resp: Value = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "initialize",
            "params": {"protocolVersion": "2025-03-26"}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(resp["result"]["protocolVersion"], json!("2025-03-26"));
    assert!(resp.get("error").is_none(), "initialize failed: {resp}");

    hold.abort();
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
        // REST endpoint replacing prompts/list JSON-RPC (#818 phase 2)
        .route(
            "/v1/prompts",
            axum::routing::get(move || {
                let state = state_clone.clone();
                async move {
                    let name = *state.lock().unwrap();
                    axum::Json(json!({
                        "total": 1,
                        "prompts": [{
                            "name": name,
                            "description": "dynamic",
                            "arguments": [],
                        }]
                    }))
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
/// `GET /v1/resources` payload (REST, replaces `resources/list` JSON-RPC
/// after #818 phase 2). Returns `(port, shutdown_tx)`.
async fn spawn_resources_backend(resources: Vec<Value>) -> (u16, tokio::sync::oneshot::Sender<()>) {
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/v1/resources",
            axum::routing::get({
                let resources = resources.clone();
                move || {
                    let resources = resources.clone();
                    async move {
                        axum::Json(json!({
                            "total": resources.len(),
                            "resources": resources,
                        }))
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
        http_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::http_registration::HttpInstanceRegistry::default(),
        )),
        mdns_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::mdns_discovery::MdnsInstanceRegistry::default(),
        )),
        relay_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::relay_discovery::RelayInstanceRegistry::default(),
        )),
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
        client_attribution: std::sync::Arc::new(
            crate::gateway::caller_attribution::ClientAttributionStore::default(),
        ),
        pending_calls: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        policy: std::sync::Arc::new(crate::gateway::GatewayPolicy::default()),
        security: std::sync::Arc::new(crate::gateway::GatewaySecurityPolicy::disabled()),
        adapter_version: None,
        adapter_dcc: None,
        capability_index: std::sync::Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        event_log: std::sync::Arc::new(crate::gateway::event_log::EventLog::new()),
        #[cfg(feature = "prometheus")]
        gateway_metrics: std::sync::Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        middleware_chain: std::sync::Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
        instance_diagnostics: std::sync::Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: std::sync::Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        search_telemetry: std::sync::Arc::new(
            crate::gateway::search_telemetry::SearchTelemetryStore::new(),
        ),
        debug_routes_enabled: false,
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
