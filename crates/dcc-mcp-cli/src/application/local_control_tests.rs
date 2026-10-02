use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use tempfile::TempDir;

use super::*;

#[test]
fn empty_query_without_limit_requests_the_complete_inventory() {
    assert_eq!(effective_search_limit(None, None), usize::MAX);
    assert_eq!(effective_search_limit(Some("spawn"), None), 25);
    assert_eq!(effective_search_limit(None, Some(7)), 7);
}

#[test]
fn local_tool_slug_round_trips() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    let slug = local_instance::local_tool_slug(&entry, "maya_scene__get_session_info");
    let parsed = parse_local_tool_slug(&slug);

    assert_eq!(parsed.dcc_type.as_deref(), Some("maya"));
    assert_eq!(
        parsed.instance_hint.as_deref(),
        Some(local_instance::instance_short(&entry).as_str())
    );
    assert_eq!(parsed.backend_tool, "maya_scene__get_session_info");
}

#[test]
fn local_call_retries_only_after_structured_unknown_sidecar_action() {
    assert!(should_retry_local_call_via_discovery(&json!({
        "success": false,
        "error": "unknown-action"
    })));
    assert!(!should_retry_local_call_via_discovery(&json!({
        "isError": true,
        "structuredContent": {"error": "dispatch-failed"}
    })));
}

#[test]
fn ui_control_calls_use_the_discovery_executor() {
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    entry.metadata.insert(
        "discovery_mcp_url".to_string(),
        "http://127.0.0.1:18081/mcp".to_string(),
    );

    assert_eq!(
        local_dispatch_mcp_url(&entry, "ui_control__snapshot"),
        "http://127.0.0.1:18081/mcp"
    );
    assert_eq!(
        local_dispatch_mcp_url(&entry, "maya_scene__get_session_info"),
        "http://127.0.0.1:18080/mcp"
    );
}

#[test]
fn load_skill_request_strips_local_routing_fields() {
    let request = split_load_skill_request(json!({
        "skill_name": "workflow",
        "dcc_type": "maya",
        "dcc": "maya-legacy",
        "instance_id": "abc12345",
        "target_tool_slug": "maya.abc12345.workflow__run",
        "meta": {"search_id": "search-1"},
        "_meta": {"response_format": "json"},
        "tool_group": "execution",
        "activate_groups": false
    }))
    .unwrap();

    assert_eq!(request.dcc_type.as_deref(), Some("maya"));
    assert_eq!(request.instance_id.as_deref(), Some("abc12345"));
    assert_eq!(
        request.target_tool_slug.as_deref(),
        Some("maya.abc12345.workflow__run")
    );
    assert_eq!(
        request.backend_body,
        json!({
            "skill_name": "workflow",
            "tool_group": "execution",
            "activate_groups": false,
            "strict_tool_group": true
        })
    );
}

#[test]
fn correlated_ungrouped_tool_does_not_request_strict_group_loading() {
    let request = split_load_skill_request(json!({
        "skill_name": "workflow",
        "target_tool_slug": "maya.abc12345.workflow__run"
    }))
    .unwrap();

    assert!(request.target_tool_slug.is_some());
    assert!(request.backend_body.get("strict_tool_group").is_none());
}

#[test]
fn local_post_load_hint_resolves_bare_declaration_to_canonical_action() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    let mut payload = json!({
        "loaded": true,
        "skill_name": "workflow",
        "registered_tools": ["workflow__run"],
        "tools": [{
            "name": "workflow__run",
            "inputSchema": {"type": "object", "properties": {}}
        }]
    });

    attach_local_post_load_hint(
        &mut payload,
        &entry,
        "blender.deadbeef.run",
        Some("workflow"),
    );

    assert_eq!(
        payload["next_step"]["arguments"]["tool_slug"],
        local_instance::local_tool_slug(&entry, "workflow__run")
    );
}

#[test]
fn local_no_arg_tool_requires_safety_hints_before_direct_call() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    let mut hits = Vec::new();
    extend_tool_hits(
        &mut hits,
        &entry,
        &json!({
            "tools": [{
                "name": "dangerous_reset",
                "description": "Reset the scene",
                "has_schema": false,
                "annotations": {"destructive_hint": true}
            }]
        }),
    );
    assert_eq!(hits[0]["next_step"]["action"], "describe");

    hits.clear();
    extend_tool_hits(
        &mut hits,
        &entry,
        &json!({
            "tools": [{
                "name": "dangerous_reset",
                "description": "Reset the scene",
                "has_schema": false,
                "annotations": {
                    "read_only_hint": false,
                    "destructive_hint": true,
                    "idempotent_hint": false,
                    "open_world_hint": false
                }
            }]
        }),
    );
    assert_eq!(hits[0]["next_step"]["action"], "call");
    assert_eq!(hits[0]["annotations"]["destructive_hint"], true);
}

#[test]
fn local_strict_object_schema_can_call_without_describe() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    let mut payload = json!({
        "loaded": true,
        "tools": [{
            "name": "workflow__run",
            "inputSchema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
                "additionalProperties": false
            },
            "annotations": {
                "read_only_hint": true,
                "destructive_hint": false,
                "idempotent_hint": true,
                "open_world_hint": false
            }
        }]
    });

    attach_local_post_load_hint(&mut payload, &entry, "workflow__run", Some("workflow"));

    assert_eq!(payload["next_step"]["action"], "call", "{payload}");
    assert_eq!(payload["compact_schema"]["complete_for_call"], true);
    assert_eq!(payload["compact_schema"]["has_schema"], true);
}

#[test]
fn local_complex_schema_requires_describe_even_with_safety_hints() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    for schema in [
        json!({"type": "object", "oneOf": []}),
        json!({"type": "object", "anyOf": []}),
        json!({"type": "object", "allOf": []}),
        json!({"type": "object", "$ref": "#/$defs/input", "$defs": {}}),
        json!({"type": "object", "additionalProperties": true}),
        json!({"type": "object", "additionalProperties": {"type": "string"}}),
        json!({"type": "object", "patternProperties": {}}),
        json!({"type": "object", "dependentRequired": {}}),
        json!({"type": "object", "if": {}, "then": {}}),
    ] {
        let mut payload = json!({
            "loaded": true,
            "tools": [{
                "name": "workflow__run",
                "inputSchema": schema,
                "annotations": {
                    "read_only_hint": true,
                    "destructive_hint": false,
                    "idempotent_hint": true,
                    "open_world_hint": false
                }
            }]
        });
        attach_local_post_load_hint(&mut payload, &entry, "workflow__run", Some("workflow"));
        assert_eq!(payload["next_step"]["action"], "describe", "{payload}");
        assert_eq!(payload["compact_schema"]["complete_for_call"], false);
        assert_eq!(payload["compact_schema"]["has_schema"], true);
    }
}

#[test]
fn reload_result_rejects_backend_failure_envelope() {
    assert!(!reload_result_succeeded(&json!({
        "success": false,
        "error": "no-source-file"
    })));
}

#[test]
fn call_result_rejects_nested_tool_failure_but_accepts_transport_success() {
    assert!(!call_result_succeeded(&json!({
        "success": true,
        "output": {"success": false, "message": "domain failure"}
    })));
    assert!(!call_result_succeeded(&json!({
        "success": false,
        "results": [{"ok": false}]
    })));
    assert!(!call_result_succeeded(&json!({
        "content": [{
            "type": "text",
            "text": r#"{"success":false,"message":"sidecar domain failure"}"#
        }],
        "isError": false
    })));
    assert!(call_result_succeeded(&json!({
        "success": true,
        "output": {"success": true}
    })));
}

#[test]
fn observable_discovery_catalog_satisfies_missing_readiness_bit() {
    let readiness = json!({
        "process": true,
        "dcc": true,
        "dispatcher": true
    });

    let reconciled = reconcile_catalog_readiness(readiness, true);

    assert_eq!(reconciled["skill_catalog"], true);
    assert_eq!(reconciled["skill_catalog_source"], "discovery_mcp");
    assert!(missing_required_fields(Some(&reconciled), &["skill_catalog".to_string()]).is_empty());
}

#[tokio::test]
async fn local_mcp_rejects_late_response_from_timed_out_previous_call() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::Json;
    use axum::Router;
    use axum::extract::State;
    use axum::routing::post;

    #[derive(Clone)]
    struct FixtureState {
        calls: Arc<AtomicUsize>,
        first_request_id: Arc<Mutex<Option<Value>>>,
        first_request_started: Arc<tokio::sync::Notify>,
        first_response_release: Arc<tokio::sync::Notify>,
    }

    async fn off_by_one_response(
        State(state): State<FixtureState>,
        Json(request): Json<Value>,
    ) -> Json<Value> {
        let call_index = state.calls.fetch_add(1, Ordering::SeqCst);
        let request_id = request.get("id").cloned().unwrap_or(Value::Null);
        if call_index == 0 {
            *state.first_request_id.lock().expect("first request id") = Some(request_id.clone());
            state.first_request_started.notify_one();
            state.first_response_release.notified().await;
            return Json(json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {"marker": "ALPHA"}
            }));
        }

        let stale_request_id = state
            .first_request_id
            .lock()
            .expect("first request id")
            .clone()
            .expect("the timed-out request reached the server");
        Json(json!({
            "jsonrpc": "2.0",
            "id": stale_request_id,
            "result": {"marker": "ALPHA"}
        }))
    }

    let state = FixtureState {
        calls: Arc::new(AtomicUsize::new(0)),
        first_request_id: Arc::new(Mutex::new(None)),
        first_request_started: Arc::new(tokio::sync::Notify::new()),
        first_response_release: Arc::new(tokio::sync::Notify::new()),
    };
    let first_request_started = state.first_request_started.clone();
    let first_response_release = state.first_response_release.clone();
    let app = Router::new()
        .route("/mcp", post(off_by_one_response))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind MCP fixture");
    let address = listener.local_addr().expect("MCP fixture address");
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mcp_url = format!("http://{address}/mcp");

    let slow_mcp_url = mcp_url.clone();
    let slow_request = tokio::spawn(async move {
        mcp_request(
            &HttpGateway::with_timeout(Duration::from_secs(1)),
            &slow_mcp_url,
            "tools/call",
            json!({
                "name": "maya_scripting__execute_python",
                "arguments": {"marker": "ALPHA"}
            }),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(1), first_request_started.notified())
        .await
        .expect("the slow request must reach the MCP handler before its timeout");
    let timeout_error = slow_request
        .await
        .expect("slow request task")
        .expect_err("the first slow request must time out");
    first_response_release.notify_one();
    let timeout_message = timeout_error.to_string();
    let timeout_request_id = timeout_message
        .split("request_id=")
        .nth(1)
        .and_then(|value| value.strip_suffix(')'))
        .expect("timeout error must expose its request id");
    let timeout_uuid = timeout_request_id
        .strip_prefix("dcc-mcp-cli-local-")
        .expect("timeout request id prefix");
    uuid::Uuid::parse_str(timeout_uuid).expect("timeout request id must end in a UUID");

    let error = mcp_request(
        &HttpGateway::with_timeout(Duration::from_secs(1)),
        &mcp_url,
        "tools/call",
        json!({
            "name": "3dsmax_scripting__execute_maxscript",
            "arguments": {"marker": "BRAVO"}
        }),
    )
    .await
    .expect_err("the second call must reject the first call's late response");

    assert!(error.to_string().contains("transport desync"), "{error}");
    server.abort();
}

#[tokio::test]
async fn local_mcp_jsonrpc_error_exposes_its_request_id() {
    use axum::Json;
    use axum::Router;
    use axum::routing::post;

    let app = Router::new().route(
        "/mcp",
        post(|Json(request): Json<Value>| async move {
            Json(json!({
                "jsonrpc": "2.0",
                "id": request.get("id").cloned().unwrap_or(Value::Null),
                "error": {"code": -32000, "message": "host failed"}
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind MCP fixture");
    let address = listener.local_addr().expect("MCP fixture address");
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let error = mcp_request(
        &HttpGateway::with_timeout(Duration::from_secs(1)),
        &format!("http://{address}/mcp"),
        "tools/call",
        json!({"name": "nuke_scripting__execute_python", "arguments": {}}),
    )
    .await
    .expect_err("JSON-RPC error must fail the request");
    let message = error.to_string();
    let request_id = message
        .split("request_id=")
        .nth(1)
        .and_then(|value| value.split(')').next())
        .expect("JSON-RPC error must expose its request id");
    let request_uuid = request_id
        .strip_prefix("dcc-mcp-cli-local-")
        .expect("JSON-RPC error request id prefix");
    uuid::Uuid::parse_str(request_uuid).expect("JSON-RPC error request id must end in a UUID");
    assert!(message.contains("host failed"), "{message}");
    server.abort();
}

#[tokio::test]
async fn local_call_retry_exposes_the_discovery_attempt_request_id() {
    use std::sync::{Arc, Mutex};

    use axum::Json;
    use axum::Router;
    use axum::extract::State;
    use axum::routing::post;
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;
    use tempfile::TempDir;

    #[derive(Clone)]
    struct FixtureState {
        unknown_action: bool,
        request_ids: Arc<Mutex<Vec<String>>>,
    }

    async fn tools_call(
        State(state): State<FixtureState>,
        Json(request): Json<Value>,
    ) -> Json<Value> {
        let request_id = request
            .get("id")
            .and_then(Value::as_str)
            .expect("local MCP request string id")
            .to_string();
        state
            .request_ids
            .lock()
            .expect("request ids")
            .push(request_id.clone());
        let result = if state.unknown_action {
            json!({"success": false, "error": "unknown-action"})
        } else {
            json!({"success": true, "message": "discovery handled call"})
        };
        Json(json!({"jsonrpc": "2.0", "id": request_id, "result": result}))
    }

    async fn spawn_fixture(
        unknown_action: bool,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let request_ids = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/mcp", post(tools_call))
            .with_state(FixtureState {
                unknown_action,
                request_ids: request_ids.clone(),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind MCP fixture");
        let address = listener.local_addr().expect("MCP fixture address");
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}/mcp"), request_ids, server)
    }

    let (dispatch_url, dispatch_ids, dispatch_server) = spawn_fixture(true).await;
    let (discovery_url, discovery_ids, discovery_server) = spawn_fixture(false).await;
    let registry_dir = TempDir::new().expect("registry tempdir");
    let registry = FileRegistry::new(registry_dir.path()).expect("file registry");
    let mut entry = ServiceEntry::new("3dsmax", "127.0.0.1", 0);
    entry.metadata.insert("mcp_url".to_string(), dispatch_url);
    entry
        .metadata
        .insert("discovery_mcp_url".to_string(), discovery_url);
    registry.register(entry).expect("register local DCC");

    let output = call_local(
        registry_dir.path().to_path_buf(),
        "3dsmax_scripting__execute_maxscript".to_string(),
        Some("3dsmax".to_string()),
        None,
        json!({"script": "\"BRAVO\"", "confirm_execution": true}),
        None,
        Duration::from_secs(1),
    )
    .await
    .expect("discovery retry must succeed");

    let dispatch_id = dispatch_ids.lock().expect("dispatch ids")[0].clone();
    let discovery_id = discovery_ids.lock().expect("discovery ids")[0].clone();
    assert_ne!(dispatch_id, discovery_id);
    assert_eq!(output["request_id"], discovery_id);
    assert_eq!(output["result"]["message"], "discovery handled call");
    dispatch_server.abort();
    discovery_server.abort();
}

#[test]
fn gateway_dispatchability_requires_skill_ownership() {
    // The gateway capability index is built from the backend's skill
    // catalog, so only skill-owned actions are reachable through it.
    assert!(is_gateway_dispatchable("list_objects", Some("maya-scene")));
    // A `<skill>__<tool>` name carries its own ownership.
    assert!(is_gateway_dispatchable("maya_scene__list_objects", None));
    // Neither ownership signal: local-only, would 404 through the gateway.
    assert!(!is_gateway_dispatchable("create_locator", None));
    assert!(!is_gateway_dispatchable("list_objects", None));
    // Degenerate qualified names are not ownership.
    assert!(!is_gateway_dispatchable("__list_objects", None));
    assert!(!is_gateway_dispatchable("maya_scene__", None));
}

#[test]
fn tool_hits_are_marked_with_gateway_dispatchability() {
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18080);
    let payload = json!({
        "tools": [
            {"name": "maya_scene__list_objects", "description": "skill owned"},
            {"name": "create_locator", "description": "no skill"}
        ]
    });

    let mut hits = Vec::new();
    extend_tool_hits(&mut hits, &entry, &payload);

    assert_eq!(hits.len(), 2);
    let by_name = |name: &str| {
        hits.iter()
            .find(|hit| hit["backend_tool"] == name)
            .unwrap_or_else(|| panic!("missing hit for {name}"))
    };
    assert_eq!(
        by_name("maya_scene__list_objects")["gateway_dispatchable"],
        true
    );
    assert_eq!(by_name("create_locator")["gateway_dispatchable"], false);
}

#[tokio::test]
async fn gateway_only_filter_selects_or_inverts_the_dispatchable_set() {
    // Drives the production filter through `search_local` against a stub
    // backend, so deleting the `retain` from `search_local` fails this
    // test. A test that re-implements the predicate on a hand-written
    // `Vec<Value>` has no such power.
    use axum::Json;
    use axum::Router;
    use axum::routing::post;
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;
    use tempfile::TempDir;

    let tools = vec![
        json!({"name": "maya_scene__list_objects", "description": "skill owned"}),
        json!({"name": "create_locator", "description": "no skill"}),
    ];
    let app = Router::new().route(
        "/mcp",
        post(move |Json(request): Json<Value>| {
            let tools = tools.clone();
            async move {
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").cloned().unwrap_or(Value::Null),
                    "result": {"tools": tools, "nextCursor": Value::Null},
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind MCP fixture");
    let address = listener.local_addr().expect("MCP fixture address");
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let registry_dir = TempDir::new().expect("registry tempdir");
    let registry = FileRegistry::new(registry_dir.path()).expect("file registry");
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    entry
        .metadata
        .insert("mcp_url".to_string(), format!("http://{address}/mcp"));
    registry.register(entry).expect("register local DCC");

    let search = |gateway_only: Option<bool>| {
        let registry_dir = registry_dir.path().to_path_buf();
        async move {
            search_local(
                registry_dir,
                SearchRequest {
                    query: None,
                    dcc_type: None,
                    instance_id: None,
                    limit: None,
                    gateway_only,
                },
            )
            .await
            .expect("local search")
        }
    };

    let all = search(None).await;
    assert_eq!(all["total"], 2, "without the flag both tools are listed");

    let dispatchable = search(Some(true)).await;
    assert_eq!(
        dispatchable["truncated"], false,
        "filtering is not truncation: nothing was withheld from the caller"
    );
    assert_eq!(
        dispatchable["total_matches"], 1,
        "the counts must describe the filtered set, not the raw inventory"
    );
    let dispatchable = dispatchable["hits"].as_array().expect("hits");
    assert_eq!(dispatchable.len(), 1, "only the skill-owned tool routes");
    assert_eq!(
        dispatchable[0]["backend_tool"], "maya_scene__list_objects",
        "a `<skill>__<tool>` name carries its own ownership"
    );

    // `--gateway-only=false` is the difference: what this CLI advertises
    // from the raw `tools/list` that the gateway index cannot route to.
    let local_only = search(Some(false)).await;
    assert_eq!(local_only["truncated"], false);
    assert_eq!(local_only["total_matches"], 1);
    let local_only = local_only["hits"].as_array().expect("hits");
    assert_eq!(local_only.len(), 1);
    assert_eq!(
        local_only[0]["backend_tool"], "create_locator",
        "an unowned raw tool is local-only"
    );

    server.abort();
}

/// Stub backend that answers both `tools/list` and `search_tools`.
///
/// `search_tools` honours the `limit` it is given the way a real adapter
/// does (`discovery.rs` truncates the hit list before replying), so a test
/// that only stubs `tools/list` cannot see the filter/limit ordering bug.
async fn spawn_search_stub(tools: Vec<Value>) -> (String, tokio::task::JoinHandle<()>) {
    use axum::Json;
    use axum::Router;
    use axum::routing::post;

    let app = Router::new().route(
        "/mcp",
        post(move |Json(request): Json<Value>| {
            let tools = tools.clone();
            async move {
                let method = request.get("method").and_then(Value::as_str);
                let params = request.get("params").cloned().unwrap_or(Value::Null);
                let requested_limit = params
                    .get("arguments")
                    .and_then(|args| args.get("limit"))
                    .and_then(Value::as_u64)
                    .map(|limit| limit as usize)
                    .unwrap_or(usize::MAX);
                // `tools/list` is the unfiltered inventory; `search_tools` is
                // the ranked-and-capped one.
                let mut listed = if method == Some("tools/call") {
                    tools.clone()
                } else {
                    tools.clone()
                };
                listed.truncate(requested_limit);
                let payload = json!({
                    "tools": listed,
                    "total_matches": tools.len(),
                    "truncated": listed.len() < tools.len(),
                });
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").cloned().unwrap_or(Value::Null),
                    "result": {
                        "content": [{"type": "text", "text": payload.to_string()}],
                    },
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind MCP fixture");
    let address = listener.local_addr().expect("MCP fixture address");
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{address}/mcp"), server)
}

/// Register one stub instance and keep its `FileRegistry` alive.
///
/// The registry owns the sentinel file handle that marks the row as owned by
/// this process; dropping it lets `read_alive` prune the entry, which would
/// make the search see an empty inventory.
async fn register_stub_instance(mcp_url: &str) -> (TempDir, FileRegistry) {
    let registry_dir = TempDir::new().expect("registry tempdir");
    let registry = FileRegistry::new(registry_dir.path()).expect("file registry");
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 0);
    entry
        .metadata
        .insert("mcp_url".to_string(), mcp_url.to_string());
    registry.register(entry).expect("register local DCC");
    (registry_dir, registry)
}

#[tokio::test]
async fn gateway_only_sees_the_whole_inventory_before_the_limit_truncates_it() {
    // The filter must run on the full inventory, not on the backend's
    // already-truncated page. With `--limit 1` the stub hands back only its
    // first hit (`create_locator`, not gateway-routable), so a filter applied
    // after the backend's truncation would report zero dispatchable tools
    // even though one exists.
    let (mcp_url, server) = spawn_search_stub(vec![
        json!({"name": "create_locator", "description": "no skill"}),
        json!({"name": "maya_scene__list_objects", "description": "skill owned"}),
        json!({"name": "maya_scene__get_session_info", "description": "skill owned"}),
    ])
    .await;
    let (registry_dir, _registry) = register_stub_instance(&mcp_url).await;

    let output = search_local(
        registry_dir.path().to_path_buf(),
        SearchRequest {
            query: Some("scene".to_string()),
            dcc_type: None,
            instance_id: None,
            limit: Some(1),
            gateway_only: Some(true),
        },
    )
    .await
    .expect("filtered local search");

    let hits = output["hits"].as_array().expect("hits");
    assert_eq!(
        hits.len(),
        1,
        "the limit applies after the filter, so the dispatchable hit survives; got {output}"
    );
    // Hits are sorted by slug, so the first dispatchable row is
    // `maya_scene__get_session_info`; what matters is that it is one of the
    // skill-owned tools and not the unowned `create_locator` the backend
    // would have returned first.
    assert_eq!(
        hits[0]["backend_tool"], "maya_scene__get_session_info",
        "the surviving hit must be a dispatchable one; got {output}"
    );
    assert_eq!(
        output["total_matches"], 2,
        "two of the three tools are dispatchable, and the count is the \
         filtered size rather than the backend's unfiltered total"
    );
    assert_eq!(
        output["truncated"], true,
        "the limit really did cut the filtered set, so say so"
    );

    server.abort();
}

#[tokio::test]
async fn gateway_only_without_a_limit_reports_no_truncation() {
    // Same stub, no `--limit`: filtering removes a row but withholds
    // nothing, so `truncated` must stay false and `total_matches` must be
    // the filtered size (2), not the raw inventory size (3).
    let (mcp_url, server) = spawn_search_stub(vec![
        json!({"name": "create_locator", "description": "no skill"}),
        json!({"name": "maya_scene__list_objects", "description": "skill owned"}),
        json!({"name": "maya_scene__get_session_info", "description": "skill owned"}),
    ])
    .await;
    let (registry_dir, _registry) = register_stub_instance(&mcp_url).await;

    let output = search_local(
        registry_dir.path().to_path_buf(),
        SearchRequest {
            query: Some("scene".to_string()),
            dcc_type: None,
            instance_id: None,
            limit: None,
            gateway_only: Some(true),
        },
    )
    .await
    .expect("filtered local search");

    let hits = output["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 2, "both dispatchable tools are returned");
    assert_eq!(output["total_matches"], 2);
    assert_eq!(
        output["truncated"], false,
        "nothing was withheld: the excluded row was filtered, not truncated"
    );

    server.abort();
}
