use serde_json::{Value, json};

/// Spawn a tiny axum server that answers both `tools/list` (empty) and
/// `prompts/list` / `prompts/get` with canned fixtures.
///
/// After #818 phase 2 the gateway contacts backends over REST (`/v1/*`),
/// so the mock serves `GET /v1/prompts` and `GET /v1/prompts/{name}`.
///
/// The caller supplies the per-backend prompt name and a marker text
/// that the `GET /v1/prompts/{name}` route echoes back so we can assert
/// the request landed on the intended backend.
pub(crate) async fn spawn_prompts_backend(
    prompt_name: &'static str,
    echo_text: &'static str,
) -> (String, tokio::sync::oneshot::Sender<()>) {
    use axum::extract::Path;
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
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

/// Build a GatewayState with the supplied registry.
pub(crate) async fn make_gateway_state(
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
            crate::gateway::mdns_registration::MdnsInstanceRegistry::default(),
        )),
        relay_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::relay_registration::RelayInstanceRegistry::default(),
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
        adapter_version: None,
        adapter_dcc: None,
        capability_index: std::sync::Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        search_cache: std::sync::Arc::new(
            crate::gateway::capability::search_cache::SearchCache::new(Default::default()),
        ),
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
        auth: std::sync::Arc::new(crate::gateway::security::GatewayAuth::disabled()),
        update_manifest_url: None,
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
    }
}

pub(crate) async fn spawn_canonical_workflow_backend() -> (u16, tokio::sync::oneshot::Sender<()>) {
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
                    let received_arguments = body
                        .get("params")
                        .and_then(|params| params.get("arguments"))
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "loaded": true,
                                "skill_name": "maya-primitives",
                                "dcc_type": "maya",
                                "activated_groups": ["core"],
                                "received_arguments": received_arguments,
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

pub(crate) async fn post_mcp_json(client: &reqwest::Client, url: &str, body: Value) -> Value {
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

pub(crate) fn mcp_call_text_json(response: &Value) -> Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("tools/call response text");
    serde_json::from_str(text).unwrap_or_else(|_| json!({"text": text}))
}

/// Parse `127.0.0.1:12345` back into `(host, port)`.
pub(crate) fn parse_addr(addr: &str) -> (&str, u16) {
    let (h, p) = addr.rsplit_once(':').unwrap();
    (h, p.parse().unwrap())
}

/// Spawn a fake backend that answers `/health` green and serves a canned
/// `GET /v1/resources` payload. Returns `(port, shutdown_tx)`.
pub(crate) async fn spawn_resources_backend(
    resources: Vec<Value>,
) -> (u16, tokio::sync::oneshot::Sender<()>) {
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
/// given `(dcc_type, port)` rows. Returns `(state, _dir, instance_ids)`.
pub(crate) async fn gateway_state_with_instances(
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
            crate::gateway::mdns_registration::MdnsInstanceRegistry::default(),
        )),
        relay_instance_registry: std::sync::Arc::new(parking_lot::RwLock::new(
            crate::gateway::relay_registration::RelayInstanceRegistry::default(),
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
        adapter_version: None,
        adapter_dcc: None,
        capability_index: std::sync::Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        search_cache: std::sync::Arc::new(
            crate::gateway::capability::search_cache::SearchCache::new(Default::default()),
        ),
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
        auth: std::sync::Arc::new(crate::gateway::security::GatewayAuth::disabled()),
        update_manifest_url: None,
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
    };
    (state, dir, ids)
}

pub(crate) fn id8(id: &uuid::Uuid) -> String {
    let mut s = id.simple().to_string();
    s.truncate(8);
    s
}
