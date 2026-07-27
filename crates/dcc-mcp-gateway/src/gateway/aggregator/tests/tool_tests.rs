use super::helpers::*;
use crate::gateway::aggregator::skill_mgmt::skill_management_tool_defs;
use crate::gateway::aggregator::*;
use serde_json::{Value, json};
use uuid::Uuid;

async fn spawn_latency_search_backend(
    action: &'static str,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    slow: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> (u16, tokio::sync::oneshot::Sender<()>) {
    use std::sync::atomic::Ordering;

    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/v1/search",
            axum::routing::post(move || {
                let calls = calls.clone();
                let slow = slow.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    if slow.load(Ordering::SeqCst) {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    axum::Json(json!({
                        "total": 1,
                        "hits": [{
                            "skill": "latency-test",
                            "action": action,
                            "summary": action,
                            "loaded": true,
                            "has_schema": false
                        }]
                    }))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });
    (port, shutdown_tx)
}

async fn spawn_skill_search_backend(
    skill_name: &'static str,
) -> (
    u16,
    tokio::sync::oneshot::Sender<()>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let skill_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let calls = skill_calls.clone();
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/v1/search",
            axum::routing::post(move || async move {
                axum::Json(json!({
                    "total": 1,
                    "hits": [{
                        "skill": skill_name,
                        "action": format!("{skill_name}_tool"),
                        "summary": skill_name,
                        "loaded": true,
                        "has_schema": false
                    }]
                }))
            }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let id = body.get("id").cloned().unwrap_or(Value::Null);
                    axum::Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string(&json!({
                                    "skills": [{
                                        "name": skill_name,
                                        "description": skill_name,
                                        "loaded": false,
                                    }],
                                    "total": 1,
                                })).unwrap()
                            }],
                            "isError": false,
                        }
                    }))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });
    (port, shutdown_tx, skill_calls)
}

#[tokio::test]
async fn unified_skill_search_honors_the_instance_filter() {
    use std::sync::atomic::Ordering;

    let (first_port, stop_first, first_calls) = spawn_skill_search_backend("skill-a").await;
    let (second_port, stop_second, second_calls) = spawn_skill_search_backend("skill-b").await;
    let (gs, _dir, ids) =
        gateway_state_with_instances(&[("maya", first_port), ("maya", second_port)]).await;

    for kind in ["skill", "all"] {
        let text = crate::gateway::tools::tool_search(
            &gs,
            &json!({
                "kind": kind,
                "query": "skill",
                "dcc_type": "maya",
                "instance_id": ids[0].to_string(),
            }),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let payload: Value = serde_json::from_str(&text).unwrap();
        let skills = if kind == "all" {
            &payload["skills"]["skills"]
        } else {
            &payload["skills"]
        };
        assert_eq!(skills.as_array().unwrap().len(), 1, "{payload:#}");
        if kind == "all" {
            assert_eq!(
                skills[0]["next_step"]["arguments"]["skill_name"], "skill-a",
                "{payload:#}"
            );
        } else {
            assert_eq!(skills[0]["name"], "skill-a", "{payload:#}");
        }
        assert!(!text.contains("skill-b"), "{text}");
    }

    assert_eq!(first_calls.load(Ordering::SeqCst), 2);
    assert_eq!(second_calls.load(Ordering::SeqCst), 0);
    let _ = stop_first.send(());
    let _ = stop_second.send(());
}

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
    let gs = make_gateway_state(registry).await;

    assert_eq!(gs.live_instances(&*gs.registry.read().await).len(), 1);

    let result = aggregate_tools_list(&gs, None).await;
    let names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();

    let prefix = format!("i_{}__", &instance_id.to_string().replace('-', "")[..8]);
    assert!(
        !names.iter().any(|name| name.starts_with(&prefix)),
        "gateway must not fan out backend tools under any prefix: {names:?}"
    );
    assert!(
        !names.contains(&"create_sphere"),
        "bare backend tool name must not appear on the gateway surface: {names:?}"
    );
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

#[tokio::test]
async fn load_skill_backend_payload_failure_is_not_decorated_as_loaded() {
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
                let id = body.get("id").cloned().unwrap_or(Value::Null);
                axum::Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string(&json!({
                                "success": false,
                                "message": "Unknown sidecar action: load_skill",
                                "error": "unknown-action"
                            })).unwrap()
                        }],
                        "isError": false
                    }
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
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
    let gs = make_gateway_state(registry).await;

    let (text, is_error) = skill_mgmt_dispatch(
        &gs,
        "load_skill",
        &json!({"skill_name": "maya-mgear", "dcc_type": "maya"}),
    )
    .await;

    assert!(is_error, "backend success=false payload must fail: {text}");
    let payload: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(payload["success"], false);
    assert_eq!(payload["error"], "unknown-action");
    assert!(
        payload.get("loaded").is_none(),
        "gateway must not decorate a failed backend load as loaded=true: {payload}"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn load_skill_refreshes_only_its_target_instance() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (target_port, stop_target, _) = spawn_canonical_workflow_backend().await;
    let sibling_searches = std::sync::Arc::new(AtomicUsize::new(0));
    let sibling_app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/v1/search",
            axum::routing::post({
                let sibling_searches = sibling_searches.clone();
                move || {
                    let sibling_searches = sibling_searches.clone();
                    async move {
                        sibling_searches.fetch_add(1, Ordering::SeqCst);
                        axum::Json(json!({"total": 0, "hits": []}))
                    }
                }
            }),
        );
    let sibling_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let sibling_port = sibling_listener.local_addr().unwrap().port();
    let (stop_sibling, sibling_shutdown) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(sibling_listener, sibling_app)
            .with_graceful_shutdown(async {
                let _ = sibling_shutdown.await;
            })
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let (gs, _dir, ids) =
        gateway_state_with_instances(&[("maya", target_port), ("maya", sibling_port)]).await;
    crate::gateway::capability_service::refresh_all_live_backends(
        &gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    let checkpoint_before = gs.capability_index.refresh_checkpoint.lock().await.clone();
    assert!(checkpoint_before.is_some());
    sibling_searches.store(0, Ordering::SeqCst);
    let (text, is_error) = skill_mgmt_dispatch(
        &gs,
        "load_skill",
        &json!({
            "skill_name": "maya-primitives",
            "dcc_type": "maya",
            "instance_id": ids[0].to_string(),
        }),
    )
    .await;

    assert!(!is_error, "{text}");
    assert_eq!(
        sibling_searches.load(Ordering::SeqCst),
        0,
        "loading one Maya instance must not refresh its sibling Maya backend"
    );
    assert_eq!(
        *gs.capability_index.refresh_checkpoint.lock().await,
        checkpoint_before
    );
    let _ = stop_target.send(());
    let _ = stop_sibling.send(());
}

#[tokio::test]
async fn periodic_capability_refreshes_share_one_backend_fetch() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let calls_for_route = calls.clone();
    let app = axum::Router::new().route(
        "/v1/search",
        axum::routing::post(move || {
            let calls = calls_for_route.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                axum::Json(json!({
                    "total": 1,
                    "hits": [{
                        "skill": "maya-scene",
                        "action": "maya_scene__get_selection",
                        "summary": "Get the current selection",
                        "loaded": true,
                        "has_schema": false
                    }]
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });

    let (gs, _dir, _) = gateway_state_with_instances(&[("maya", port)]).await;
    let first = crate::gateway::capability_service::refresh_all_live_backends(
        &gs,
        crate::gateway::capability::RefreshReason::Periodic,
    );
    let second = crate::gateway::capability_service::refresh_all_live_backends(
        &gs,
        crate::gateway::capability::RefreshReason::Periodic,
    );
    tokio::join!(first, second);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    crate::gateway::capability_service::refresh_all_live_backends(
        &gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    crate::gateway::capability_service::refresh_all_live_backends_now(
        &gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    crate::gateway::capability_service::refresh_all_live_backends(
        &gs,
        crate::gateway::capability::RefreshReason::ToolsListChanged,
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn targeted_search_does_not_refresh_unrelated_slow_dcc() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let houdini_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let maya_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let (houdini_port, stop_houdini) = spawn_latency_search_backend(
        "houdini_fast_tool",
        houdini_calls.clone(),
        std::sync::Arc::new(AtomicBool::new(false)),
    )
    .await;
    let (maya_port, stop_maya) = spawn_latency_search_backend(
        "maya_slow_tool",
        maya_calls.clone(),
        std::sync::Arc::new(AtomicBool::new(true)),
    )
    .await;
    let (gs, _dir, _) =
        gateway_state_with_instances(&[("houdini", houdini_port), ("maya", maya_port)]).await;
    let query = json!({"query": "fast tool", "dcc_type": "houdini"});

    let first = crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None);
    let second = crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None);
    let (first, second) = tokio::join!(first, second);
    let first = first.unwrap();
    let second = second.unwrap();

    assert!(first.contains("houdini_fast_tool"), "{first}");
    assert!(second.contains("houdini_fast_tool"), "{second}");
    assert_eq!(
        houdini_calls.load(Ordering::SeqCst),
        1,
        "concurrent cold searches should share one backend refresh"
    );
    assert_eq!(
        maya_calls.load(Ordering::SeqCst),
        0,
        "a targeted Houdini search must not touch an unrelated Maya backend"
    );
    let _ = stop_houdini.send(());
    let _ = stop_maya.send(());
}

#[tokio::test]
async fn missing_slug_describe_refreshes_only_its_owning_dcc() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let houdini_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let maya_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let (houdini_port, stop_houdini) = spawn_latency_search_backend(
        "houdini_describe_tool",
        houdini_calls.clone(),
        std::sync::Arc::new(AtomicBool::new(false)),
    )
    .await;
    let (maya_port, stop_maya) = spawn_latency_search_backend(
        "maya_unrelated_tool",
        maya_calls.clone(),
        std::sync::Arc::new(AtomicBool::new(false)),
    )
    .await;
    let (gs, _dir, ids) =
        gateway_state_with_instances(&[("houdini", houdini_port), ("maya", maya_port)]).await;
    let missing_slug =
        crate::gateway::capability::tool_slug("houdini", &ids[0], "houdini_describe_tool");

    crate::gateway::capability_service::refresh_for_describe(&gs, &missing_slug).await;

    assert_eq!(houdini_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        maya_calls.load(Ordering::SeqCst),
        0,
        "a valid missing Houdini slug must not refresh unrelated Maya discovery"
    );
    let _ = stop_houdini.send(());
    let _ = stop_maya.send(());
}

#[tokio::test]
async fn cached_search_rebinds_followup_correlation_and_telemetry() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (port, stop) = spawn_latency_search_backend(
        "cached_correlation_tool",
        calls,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .await;
    let (gs, _dir, _) = gateway_state_with_instances(&[("houdini", port)]).await;
    let query = json!({"query": "cached correlation", "dcc_type": "houdini"});

    let first: Value = serde_json::from_str(
        &crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
            .await
            .unwrap(),
    )
    .unwrap();
    let second: Value = serde_json::from_str(
        &crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
            .await
            .unwrap(),
    )
    .unwrap();

    assert_eq!(second["search_cache_hit"], true);
    assert_ne!(first["search_id"], second["search_id"]);
    let search_id = second["search_id"].as_str().unwrap();
    let hit = &second["hits"][0];
    assert_eq!(
        hit["next_step"]["arguments"]["meta"]["search_id"],
        search_id
    );
    assert_eq!(
        hit["next_step"]["mcp"]["arguments"]["meta"]["search_id"],
        search_id
    );
    assert_eq!(hit["next_step"]["mcp"]["_meta"]["search_id"], search_id);
    assert_eq!(
        hit["next_step"]["rest"]["body"]["meta"]["search_id"],
        search_id
    );
    assert!(
        gs.search_telemetry
            .selected_hit(search_id, hit["tool_slug"].as_str(), None)
            .is_some()
    );

    let _ = stop.send(());
}

#[tokio::test]
async fn warm_search_serves_cached_snapshot_while_backend_revalidates() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let slow = std::sync::Arc::new(AtomicBool::new(false));
    let (port, stop) =
        spawn_latency_search_backend("cached_houdini_tool", calls.clone(), slow.clone()).await;
    let maya_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let (maya_port, stop_maya) = spawn_latency_search_backend(
        "maya_cold_tool",
        maya_calls.clone(),
        std::sync::Arc::new(AtomicBool::new(false)),
    )
    .await;
    let (gs, _dir, ids) =
        gateway_state_with_instances(&[("houdini", port), ("maya", maya_port)]).await;
    let query = json!({"query": "cached houdini", "dcc_type": "houdini"});

    let cold = crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    assert!(cold.contains("cached_houdini_tool"), "{cold}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    slow.store(true, Ordering::SeqCst);
    gs.capability_index.expire_search_refresh_for_test(ids[0]);
    let warm = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None),
    )
    .await
    .expect("warm search must not wait for a slow backend")
    .unwrap();

    assert!(warm.contains("cached_houdini_tool"), "{warm}");
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while calls.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("stale snapshot should trigger background revalidation");

    let maya_query = json!({"query": "cold tool", "dcc_type": "maya"});
    let maya = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::gateway::tools::tool_search_tools(&gs, &maya_query, None, None, None),
    )
    .await
    .expect("slow Houdini revalidation must not block a cold Maya search")
    .unwrap();
    assert!(maya.contains("maya_cold_tool"), "{maya}");
    assert_eq!(maya_calls.load(Ordering::SeqCst), 1);
    let _ = stop.send(());
    let _ = stop_maya.send(());
}

#[tokio::test]
async fn warm_search_does_not_queue_refresh_behind_a_busy_instance_gate() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let (port, stop) = spawn_latency_search_backend(
        "busy_gate_tool",
        calls.clone(),
        std::sync::Arc::new(AtomicBool::new(false)),
    )
    .await;
    let (gs, _dir, ids) = gateway_state_with_instances(&[("houdini", port)]).await;
    let query = json!({"query": "busy gate", "dcc_type": "houdini"});

    crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    gs.capability_index.expire_search_refresh_for_test(ids[0]);

    let gate = gs.capability_index.refresh_gate(ids[0]);
    let guard = gate.lock().await;
    crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    tokio::task::yield_now().await;
    drop(guard);

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "stale-while-revalidate must skip a busy gate instead of queuing work"
    );
    let _ = stop.send(());
}

#[tokio::test]
async fn concurrent_warm_searches_start_only_one_background_refresh() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let slow = std::sync::Arc::new(AtomicBool::new(false));
    let (port, stop) =
        spawn_latency_search_backend("warm_concurrency_tool", calls.clone(), slow.clone()).await;
    let (gs, _dir, ids) = gateway_state_with_instances(&[("houdini", port)]).await;
    let query = json!({"query": "warm concurrency", "dcc_type": "houdini"});

    crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    slow.store(true, Ordering::SeqCst);
    gs.capability_index.expire_search_refresh_for_test(ids[0]);
    let searches =
        (0..32).map(|_| crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None));
    let results = futures::future::join_all(searches).await;
    assert!(results.into_iter().all(|result| result.is_ok()));

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while calls.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("one background refresh should start");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "one cold refresh plus one shared stale refresh is expected"
    );
    let _ = stop.send(());
}

#[tokio::test]
async fn skill_mutation_waits_for_inflight_stale_refresh_before_calling_backend() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let loaded = std::sync::Arc::new(AtomicBool::new(false));
    let search_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let stale_started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release_stale = std::sync::Arc::new(tokio::sync::Notify::new());
    let load_called = std::sync::Arc::new(tokio::sync::Notify::new());

    let loaded_for_search = loaded.clone();
    let search_calls_for_route = search_calls.clone();
    let stale_started_for_route = stale_started.clone();
    let release_stale_for_route = release_stale.clone();
    let loaded_for_call = loaded.clone();
    let load_called_for_route = load_called.clone();
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/v1/search",
            axum::routing::post(move || {
                let loaded = loaded_for_search.clone();
                let calls = search_calls_for_route.clone();
                let stale_started = stale_started_for_route.clone();
                let release_stale = release_stale_for_route.clone();
                async move {
                    let request_number = calls.fetch_add(1, Ordering::SeqCst) + 1;
                    if request_number == 2 {
                        stale_started.notify_one();
                        release_stale.notified().await;
                    }
                    let action = if request_number >= 3 && loaded.load(Ordering::SeqCst) {
                        "new_loaded_tool"
                    } else {
                        "old_snapshot_tool"
                    };
                    axum::Json(json!({
                        "total": 1,
                        "hits": [{
                            "skill": "gate-test",
                            "action": action,
                            "summary": action,
                            "loaded": true,
                            "has_schema": false
                        }]
                    }))
                }
            }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let loaded = loaded_for_call.clone();
                let load_called = load_called_for_route.clone();
                async move {
                    loaded.store(true, Ordering::SeqCst);
                    load_called.notify_one();
                    let id = body.get("id").cloned().unwrap_or(Value::Null);
                    axum::Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string(&json!({
                                    "loaded": true,
                                    "skill_name": "gate-test",
                                    "registered_tools": ["new_loaded_tool"]
                                })).unwrap()
                            }],
                            "isError": false
                        }
                    }))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stop_rx.await;
            })
            .await
            .ok();
    });

    let (gs, _dir, ids) = gateway_state_with_instances(&[("houdini", port)]).await;
    let query = json!({"query": "snapshot tool", "dcc_type": "houdini"});
    crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    gs.capability_index.expire_search_refresh_for_test(ids[0]);

    let stale_started_wait = stale_started.notified();
    crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), stale_started_wait)
        .await
        .expect("stale refresh should hold the instance gate");

    let mutation_state = gs.clone();
    let mutation = tokio::spawn(async move {
        skill_mgmt_dispatch(
            &mutation_state,
            "load_skill",
            &json!({
                "skill_name": "gate-test",
                "dcc_type": "houdini",
                "instance_id": ids[0].to_string(),
            }),
        )
        .await
    });
    let called_while_stale = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        load_called.notified(),
    )
    .await
    .is_ok();

    release_stale.notify_one();
    let (text, is_error) = mutation.await.unwrap();
    assert!(
        !is_error,
        "load_skill should succeed after stale refresh: {text}"
    );
    assert!(
        !called_while_stale,
        "skill mutation must share the instance gate with stale refresh"
    );
    assert!(
        gs.capability_index
            .snapshot()
            .records
            .iter()
            .any(|record| record.backend_tool == "new_loaded_tool"),
        "post-mutation refresh must leave the new capability indexed"
    );
    let _ = stop.send(());
}

#[tokio::test]
async fn search_evicts_cached_rows_for_instances_no_longer_live() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (port, stop) = spawn_latency_search_backend(
        "offline_houdini_tool",
        calls,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .await;
    let (gs, _dir, ids) = gateway_state_with_instances(&[("houdini", port)]).await;
    let query = json!({"query": "offline tool", "dcc_type": "houdini"});

    let cold = crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    assert!(cold.contains("offline_houdini_tool"), "{cold}");

    {
        let registry = gs.registry.read().await;
        let entry = registry
            .list_all()
            .into_iter()
            .find(|entry| entry.instance_id == ids[0])
            .unwrap();
        registry.deregister(&entry.key()).unwrap();
    }
    let after = crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    assert!(!after.contains("offline_houdini_tool"), "{after}");
    assert!(gs.capability_index.fingerprint_for(ids[0]).is_none());
    let _ = stop.send(());
}

#[tokio::test]
async fn targeted_search_evicts_cached_rows_for_unreachable_instances() {
    use dcc_mcp_transport::discovery::types::ServiceStatus;

    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (port, stop) = spawn_latency_search_backend(
        "unreachable_houdini_tool",
        calls,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .await;
    let (gs, _dir, ids) = gateway_state_with_instances(&[("houdini", port)]).await;
    let query = json!({"query": "unreachable tool", "dcc_type": "houdini"});

    let cold = crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    assert!(cold.contains("unreachable_houdini_tool"), "{cold}");

    {
        let registry = gs.registry.read().await;
        let mut entry = registry
            .list_all()
            .into_iter()
            .find(|entry| entry.instance_id == ids[0])
            .unwrap();
        registry.deregister(&entry.key()).unwrap();
        entry.status = ServiceStatus::Unreachable;
        registry.register(entry).unwrap();
    }

    let after = crate::gateway::tools::tool_search_tools(&gs, &query, None, None, None)
        .await
        .unwrap();
    assert!(!after.contains("unreachable_houdini_tool"), "{after}");
    assert!(gs.capability_index.fingerprint_for(ids[0]).is_none());
    let _ = stop.send(());
}

#[tokio::test]
async fn load_skill_for_sidecar_row_uses_discovery_endpoint() {
    let loaded = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let discovery_load_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let loaded_for_search = loaded.clone();
    let loaded_for_call = loaded.clone();
    let discovery_calls_for_route = discovery_load_calls.clone();
    let discovery_app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/v1/search",
            axum::routing::post(move || {
                let loaded = loaded_for_search.load(std::sync::atomic::Ordering::SeqCst);
                async move {
                    let hits = if loaded {
                        json!([
                            {
                                "skill": "maya-primitives",
                                "action": "maya_primitives__create_cube",
                                "summary": "Create a cube",
                                "loaded": true,
                                "has_schema": true
                            },
                            {
                                "skill": "maya-primitives",
                                "action": "maya_primitives__create_sphere",
                                "summary": "Create a sphere",
                                "loaded": true,
                                "has_schema": true
                            }
                        ])
                    } else {
                        json!([{
                            "skill": "maya-primitives",
                            "action": "create_sphere",
                            "summary": "Create a sphere",
                            "loaded": false,
                            "has_schema": true
                        }])
                    };
                    axum::Json(json!({
                        "total": hits.as_array().map_or(0, Vec::len),
                        "hits": hits
                    }))
                }
            }),
        )
        .route(
            "/v1/describe",
            axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
                let action = body
                    .get("tool_slug")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                axum::Json(json!({
                    "entry": {
                        "slug": format!("maya.maya-primitives.{action}"),
                        "skill": "maya-primitives",
                        "action": action,
                        "dcc": "maya",
                        "summary": "Create a primitive",
                        "loaded": true,
                        "has_schema": true,
                        "scope": "repo"
                    },
                    "description": "Create a primitive",
                    "input_schema": {
                        "type": "object",
                        "properties": {"radius": {"type": "number"}},
                        "required": ["radius"]
                    },
                    "annotations": {
                        "readOnlyHint": false,
                        "destructiveHint": false,
                        "idempotentHint": false,
                        "openWorldHint": false
                    },
                    "metadata": {"dcc": {"execution": "sync"}}
                }))
            }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let loaded = loaded_for_call.clone();
                let calls = discovery_calls_for_route.clone();
                async move {
                    let id = body.get("id").cloned().unwrap_or(Value::Null);
                    let name = body
                        .pointer("/params/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if name == "load_skill" {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        loaded.store(true, std::sync::atomic::Ordering::SeqCst);
                        axum::Json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [{
                                    "type": "text",
                                    "text": serde_json::to_string(&json!({
                                        "loaded": true,
                                        "skill_name": "maya-primitives",
                                        "dcc_type": "maya",
                                        "registered_tools": [
                                            "maya_primitives__create_cube",
                                            "maya_primitives__create_sphere"
                                        ]
                                    })).unwrap()
                                }],
                                "isError": false
                            }
                        }))
                    } else {
                        axum::Json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [{"type": "text", "text": format!("unexpected discovery tool: {name}")}],
                                "isError": true
                            }
                        }))
                    }
                }
            }),
        );
    let discovery_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let discovery_port = discovery_listener.local_addr().unwrap().port();
    let (stop_discovery_tx, stop_discovery_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(discovery_listener, discovery_app)
            .with_graceful_shutdown(async {
                let _ = stop_discovery_rx.await;
            })
            .await
            .ok();
    });
    let sidecar_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sidecar_calls_for_route = sidecar_calls.clone();
    let sidecar_app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let calls = sidecar_calls_for_route.clone();
                async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let id = body.get("id").cloned().unwrap_or(Value::Null);
                    let name = body
                        .pointer("/params/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let payload = if name == "maya_primitives__create_sphere" {
                        json!({
                            "success": true,
                            "created": "sphere"
                        })
                    } else {
                        json!({
                            "success": false,
                            "message": format!("Unknown sidecar action: {name}"),
                            "error": "unknown-action"
                        })
                    };
                    axum::Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string(&payload).unwrap()
                            }],
                            "isError": payload["success"] == false
                        }
                    }))
                }
            }),
        );
    let sidecar_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let sidecar_port = sidecar_listener.local_addr().unwrap().port();
    let (stop_sidecar_tx, stop_sidecar_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(sidecar_listener, sidecar_app)
            .with_graceful_shutdown(async {
                let _ = stop_sidecar_rx.await;
            })
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let instance_id = {
        let r = registry.read().await;
        let mut entry = dcc_mcp_transport::discovery::types::ServiceEntry::new(
            "maya",
            "127.0.0.1",
            sidecar_port,
        );
        entry.metadata.insert(
            crate::gateway::http_registration::MCP_URL_METADATA_KEY.to_string(),
            format!("http://127.0.0.1:{sidecar_port}/mcp"),
        );
        entry.metadata.insert(
            crate::gateway::http_registration::DISCOVERY_MCP_URL_METADATA_KEY.to_string(),
            format!("http://127.0.0.1:{discovery_port}/mcp"),
        );
        entry.metadata.insert(
            crate::gateway::http_registration::ROLE_METADATA_KEY.to_string(),
            crate::gateway::http_registration::ROLE_PER_DCC_SIDECAR.to_string(),
        );
        let id = entry.instance_id;
        r.register(entry).unwrap();
        id
    };
    let gs = make_gateway_state(registry).await;
    crate::gateway::capability_service::refresh_all_live_backends(
        &gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    let search_args = json!({
        "query": "sphere",
        "dcc_type": "maya",
        "instance_id": instance_id.to_string(),
    });
    let search_text = crate::gateway::tools::tool_search_tools(&gs, &search_args, None, None, None)
        .await
        .unwrap();
    let search_payload: Value = serde_json::from_str(&search_text).unwrap();
    let load_args = search_payload["hits"][0]["next_step"]["arguments"].clone();
    let searched_target = load_args["target_tool_slug"].as_str().unwrap().to_string();
    assert!(searched_target.ends_with(".create_sphere"));

    let (text, is_error) = crate::gateway::tools::tool_load_skill(&gs, &load_args).await;
    assert!(!is_error, "load_skill must use discovery endpoint: {text}");
    assert_eq!(
        discovery_load_calls.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        sidecar_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "skill lifecycle calls must not hit the sidecar dispatch endpoint"
    );
    let load_payload: Value = serde_json::from_str(&text).unwrap();
    let canonical_target = load_payload["next_step"]["arguments"]["tool_slug"]
        .as_str()
        .unwrap();
    assert_ne!(canonical_target, searched_target);
    assert!(canonical_target.ends_with(".maya_primitives__create_sphere"));
    assert_eq!(
        load_payload["compact_schema"]["tool_slug"],
        canonical_target
    );
    assert_eq!(
        load_payload["compact_schema"]["required"],
        json!(["radius"])
    );

    let call_result = crate::gateway::capability_service::call_service(
        &gs,
        canonical_target,
        json!({"radius": 2.0}),
        None,
        None,
        None,
    )
    .await
    .expect("canonical post-load target must be callable");
    assert!(call_result.to_string().contains("sphere"), "{call_result}");
    assert_eq!(
        sidecar_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "post-load call must route only the resolved canonical action"
    );
    let _ = stop_discovery_tx.send(());
    let _ = stop_sidecar_tx.send(());
}

#[tokio::test]
async fn load_skill_preserves_existing_index_when_v1_search_fails() {
    // Regression test for issue #1659:
    // refresh_instance must preserve the existing capability index when
    // POST /v1/search returns an error, instead of upserting empty
    // records which would delete the instance's entire tool slice.
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
                let id = body.get("id").cloned().unwrap_or(Value::Null);
                axum::Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json!({
                                "loaded": true,
                                "skill_name": "maya-mgear",
                                "dcc_type": "maya",
                                "registered_tools": [
                                    "maya_mgear__inspect",
                                    "maya_mgear__list_joints",
                                ],
                            })).unwrap()
                        }],
                        "isError": false
                    }
                }))
            }),
        )
        .route(
            "/v1/call",
            axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
                axum::Json(json!({
                    "success": true,
                    "called": body.get("tool_slug").cloned().unwrap_or(Value::Null),
                    "arguments": body.get("arguments").cloned().unwrap_or_else(|| json!({})),
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let (gs, _dir, ids) = gateway_state_with_instances(&[("maya", port)]).await;
    let iid = ids[0];

    use crate::gateway::capability::{CapabilityRecord, tool_slug};
    use dcc_mcp_gateway_core::capability::compute_fingerprint;
    use dcc_mcp_gateway_core::capability::index::InstanceFingerprint;

    let old_records = vec![
        CapabilityRecord::new(
            tool_slug("maya", &iid, "project_save"),
            "project_save".into(),
            "project_save".into(),
            Some("maya-scene".into()),
            "save the current Maya scene",
            vec![],
            "maya".into(),
            iid,
            true,
            true,
            None,
        ),
        CapabilityRecord::new(
            tool_slug("maya", &iid, "scene_open"),
            "scene_open".into(),
            "scene_open".into(),
            Some("maya-scene".into()),
            "open a Maya scene",
            vec![],
            "maya".into(),
            iid,
            true,
            true,
            None,
        ),
    ];
    let fp = compute_fingerprint(&old_records);
    gs.capability_index
        .upsert_instance(iid, old_records, InstanceFingerprint(fp.0));

    let snap_before = gs.capability_index.snapshot();
    assert!(
        snap_before
            .records
            .iter()
            .any(|r| r.backend_tool == "project_save"),
        "pre-existing project_save must be in index before load_skill"
    );
    assert_eq!(
        snap_before.records.len(),
        2,
        "only 2 pre-existing tools before load_skill"
    );

    let (text, is_error) = skill_mgmt_dispatch(
        &gs,
        "load_skill",
        &json!({"skill_name": "maya-mgear", "dcc_type": "maya"}),
    )
    .await;

    assert!(!is_error, "load_skill must succeed: {text}");
    let payload: Value = serde_json::from_str(&text).unwrap();

    let snap = gs.capability_index.snapshot();
    assert!(
        snap.records
            .iter()
            .any(|r| r.backend_tool == "project_save"),
        "project_save must survive after /v1/search 404 during load_skill"
    );
    assert!(
        snap.records.iter().any(|r| r.backend_tool == "scene_open"),
        "scene_open must survive after /v1/search 404"
    );
    assert!(
        snap.records
            .iter()
            .any(|r| r.backend_tool == "maya_mgear__inspect"),
        "new tool maya_mgear__inspect must be injected via Layer 1"
    );
    assert!(
        snap.records
            .iter()
            .any(|r| r.backend_tool == "maya_mgear__list_joints"),
        "new tool maya_mgear__list_joints must be injected"
    );
    assert_eq!(
        snap.records.len(),
        4,
        "index must contain old tools (2) + new tools (2) = 4 records; got {}",
        snap.records.len()
    );

    let query = crate::gateway::capability_service::parse_search_payload(&json!({
        "query": "mgear",
        "dcc_type": "maya",
        "instance_id": iid.to_string(),
        "loaded_only": true,
    }));
    let hits = crate::gateway::capability_service::search_service(&gs.capability_index, &query);
    let injected_slug = hits
        .iter()
        .find(|hit| hit.record.backend_tool == "maya_mgear__inspect")
        .map(|hit| hit.record.tool_slug.clone())
        .expect("gateway search must find the injected mGear tool");

    let call_result = crate::gateway::capability_service::call_service(
        &gs,
        &injected_slug,
        json!({"detail": true}),
        None,
        None,
        None,
    )
    .await
    .expect("gateway call must route the injected slug");
    assert_eq!(call_result["success"], true);
    assert_eq!(call_result["called"], "maya_mgear__inspect");

    let slugs = payload["new_tool_slugs"].as_array().unwrap();
    assert!(
        slugs.iter().any(|s| s
            .as_str()
            .is_some_and(|s| s.contains("maya_mgear__inspect"))),
        "new_tool_slugs must include maya_mgear__inspect: {slugs:?}"
    );
    assert!(
        slugs.iter().any(|s| s
            .as_str()
            .is_some_and(|s| s.contains("maya_mgear__list_joints"))),
        "new_tool_slugs must include maya_mgear__list_joints: {slugs:?}"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn rest_targeted_load_scopes_legacy_group_fallback_to_the_requested_skill() {
    // Regression test for issue #1664:
    // A dispatch-only sidecar registered with `role = "per-dcc-sidecar"` and
    // **no** `discovery_mcp_url` must still accept `load_skill` calls through
    // its `/mcp` endpoint.
    let load_skill_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let group_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let load_calls = load_skill_calls.clone();
    let activate_calls = group_calls.clone();
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let load_calls = load_calls.clone();
                let activate_calls = activate_calls.clone();
                async move {
                    let id = body.get("id").cloned().unwrap_or(Value::Null);
                    let name = body
                        .pointer("/params/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if name == "load_skill" {
                        load_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    } else if name == "activate_tool_group" {
                        activate_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        assert_eq!(
                            body.pointer("/params/arguments/skill_name")
                                .and_then(Value::as_str),
                            Some("maya-mgear")
                        );
                        assert_eq!(
                            body.pointer("/params/arguments/group_name")
                                .and_then(Value::as_str),
                            Some("inspection")
                        );
                    }
                    axum::Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string(&json!({
                                    "loaded": true,
                                    "skill_name": "maya-mgear",
                                    "dcc_type": "maya",
                                    // Legacy backends report active groups globally. Here
                                    // `inspection` belongs to the already-loaded sibling
                                    // skill, not the requested `maya-mgear` skill.
                                    "already_loaded": ["maya-scene"],
                                    "newly_loaded": ["maya-mgear"],
                                    "active_groups": ["inspection"],
                                    "registered_tools": [
                                        "maya_mgear__inspect",
                                        "maya_mgear__list_joints",
                                    ],
                                })).unwrap()
                            }],
                            "isError": false
                        }
                    }))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
        dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
    ));
    let instance_id = {
        let r = registry.read().await;
        let mut entry =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("maya", "127.0.0.1", port);
        entry.metadata.insert(
            crate::gateway::http_registration::MCP_URL_METADATA_KEY.to_string(),
            format!("http://127.0.0.1:{port}/mcp"),
        );
        // IMPORTANT: No DISCOVERY_MCP_URL_METADATA_KEY — this is a
        // dispatch-only sidecar without a separate discovery endpoint.
        entry.metadata.insert(
            crate::gateway::http_registration::ROLE_METADATA_KEY.to_string(),
            crate::gateway::http_registration::ROLE_PER_DCC_SIDECAR.to_string(),
        );
        let id = entry.instance_id;
        r.register(entry).unwrap();
        id
    };
    let gs = make_gateway_state(registry).await;

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::ACCEPT,
        "application/json".parse().unwrap(),
    );
    let response = crate::gateway::handlers::handle_v1_load_skill(
        axum::extract::State(gs.clone()),
        headers,
        axum::Json(json!({
            "skill_name": "maya-mgear",
            "dcc_type": "maya",
            "instance_id": instance_id.to_string(),
            "tool_group": "inspection",
        })),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        load_skill_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "load_skill must be routed to the sidecar's /mcp endpoint"
    );
    assert_eq!(
        group_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "legacy sidecars must receive one activate_tool_group fallback"
    );

    let snap = gs.capability_index.snapshot();
    assert!(
        snap.records
            .iter()
            .any(|r| r.backend_tool == "maya_mgear__inspect"),
        "maya_mgear__inspect must be indexed after load_skill"
    );
    assert!(
        snap.records
            .iter()
            .any(|r| r.backend_tool == "maya_mgear__list_joints"),
        "maya_mgear__list_joints must be indexed after load_skill"
    );

    assert_eq!(payload["activated_groups"], json!(["inspection"]));
    let slugs = payload["new_tool_slugs"].as_array().unwrap();
    assert!(
        slugs.iter().any(|s| s
            .as_str()
            .is_some_and(|s| s.contains("maya_mgear__inspect"))),
        "new_tool_slugs must include maya_mgear__inspect: {slugs:?}"
    );

    let _ = shutdown_tx.send(());
}
