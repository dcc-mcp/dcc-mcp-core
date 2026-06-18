use super::helpers::*;
use crate::gateway::aggregator::skill_mgmt::skill_management_tool_defs;
use crate::gateway::aggregator::*;
use serde_json::{Value, json};
use uuid::Uuid;

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
                    axum::Json(json!({
                        "total": 1,
                        "hits": [{
                            "skill": "maya-primitives",
                            "action": "maya_primitives__create_sphere",
                            "summary": "Create a sphere",
                            "loaded": loaded,
                            "has_schema": true
                        }]
                    }))
                }
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
                                        "registered_tools": ["maya_primitives__create_sphere"]
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
                    axum::Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string(&json!({
                                    "success": false,
                                    "message": format!("Unknown sidecar action: {name}"),
                                    "error": "unknown-action"
                                })).unwrap()
                            }],
                            "isError": false
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
    let unloaded_query = crate::gateway::capability_service::parse_search_payload(&json!({
        "query": "sphere",
        "dcc_type": "maya",
        "instance_id": instance_id.to_string(),
    }));
    let unloaded_hits =
        crate::gateway::capability_service::search_service(&gs.capability_index, &unloaded_query);
    assert_eq!(unloaded_hits.len(), 1);
    assert!(
        !unloaded_hits[0].record.loaded,
        "pre-load search must surface the unloaded skill hint"
    );
    let (text, is_error) = skill_mgmt_dispatch(
        &gs,
        "load_skill",
        &json!({
            "skill_name": "maya-primitives",
            "dcc_type": "maya",
            "instance_id": instance_id.to_string(),
        }),
    )
    .await;
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
    let loaded_query = crate::gateway::capability_service::parse_search_payload(&json!({
        "query": "sphere",
        "dcc_type": "maya",
        "instance_id": instance_id.to_string(),
        "loaded_only": true,
    }));
    let loaded_hits =
        crate::gateway::capability_service::search_service(&gs.capability_index, &loaded_query);
    assert!(
        loaded_hits.iter().any(|hit| {
            hit.record.loaded && hit.record.backend_tool == "maya_primitives__create_sphere"
        }),
        "post-load refresh must surface the callable skill tool"
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
async fn load_skill_for_dispatch_only_sidecar_without_discovery_url() {
    // Regression test for issue #1664:
    // A dispatch-only sidecar registered with `role = "per-dcc-sidecar"` and
    // **no** `discovery_mcp_url` must still accept `load_skill` calls through
    // its `/mcp` endpoint.
    let load_skill_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = load_skill_calls.clone();
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let calls = calls.clone();
                async move {
                    let id = body.get("id").cloned().unwrap_or(Value::Null);
                    let name = body
                        .pointer("/params/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if name == "load_skill" {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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

    let (text, is_error) = skill_mgmt_dispatch(
        &gs,
        "load_skill",
        &json!({
            "skill_name": "maya-mgear",
            "dcc_type": "maya",
            "instance_id": instance_id.to_string(),
        }),
    )
    .await;

    assert!(
        !is_error,
        "load_skill must succeed for dispatch-only sidecar without discovery_mcp_url: {text}"
    );
    assert_eq!(
        load_skill_calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "load_skill must be routed to the sidecar's /mcp endpoint"
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

    let payload: Value = serde_json::from_str(&text).unwrap();
    let slugs = payload["new_tool_slugs"].as_array().unwrap();
    assert!(
        slugs.iter().any(|s| s
            .as_str()
            .is_some_and(|s| s.contains("maya_mgear__inspect"))),
        "new_tool_slugs must include maya_mgear__inspect: {slugs:?}"
    );

    let _ = shutdown_tx.send(());
}
