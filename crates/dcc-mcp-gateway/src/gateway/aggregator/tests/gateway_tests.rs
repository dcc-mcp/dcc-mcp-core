use super::helpers::*;
use crate::gateway::aggregator::*;
use serde_json::{Value, json};

#[tokio::test]
async fn aggregate_prompts_list_zero_backends_returns_empty_array() {
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

#[tokio::test]
async fn gateway_mcp_initialize_advertises_prompts_capability() {
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
    assert_eq!(load_payload["received_arguments"]["activate_groups"], true);
    assert_eq!(
        load_payload["new_tool_slugs"][0],
        "maya.aaaaaaaa.create_sphere"
    );
    assert!(load_payload["index_generation"].as_str().is_some());
    assert_eq!(load_payload["next_step"]["action"], "describe");
    assert_eq!(load_payload["next_step"]["mcp"]["tool"], "describe");
    assert_eq!(load_payload["next_step"]["rest"]["path"], "/v1/describe");

    let correlated_load = post_mcp_json(
        &client,
        &url,
        json!({
            "jsonrpc": "2.0",
            "id": 40,
            "method": "tools/call",
            "params": {
                "name": "load_skill",
                "arguments": {
                    "skill_name": "maya-primitives",
                    "dcc_type": "maya",
                    "target_tool_slug": tool_slug.clone(),
                    "meta": {
                        "search_id": search_payload["search_id"],
                        "index_generation": search_payload["index_generation"]
                    }
                }
            }
        }),
    )
    .await;
    assert_eq!(correlated_load["result"]["isError"], false);
    let correlated_payload = mcp_call_text_json(&correlated_load);
    assert_eq!(correlated_payload["compact_schema"]["tool_slug"], tool_slug);
    assert_eq!(
        correlated_payload["compact_schema"]["required"],
        json!(["radius"])
    );
    assert_eq!(
        correlated_payload["compact_schema"]["properties"]["radius"]["type"],
        "number"
    );
    assert_eq!(correlated_payload["next_step"]["action"], "call");
    assert_eq!(
        correlated_payload["next_step"]["schema_source"],
        "load_skill.compact_schema"
    );
    assert!(
        correlated_payload["received_arguments"]
            .get("target_tool_slug")
            .is_none(),
        "gateway-only target_tool_slug must not be forwarded to the backend"
    );

    let load_lazy = post_mcp_json(
        &client,
        &url,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "load_skill",
                "arguments": {
                    "skill_name": "maya-primitives",
                    "dcc_type": "maya",
                    "activate_groups": false
                }
            }
        }),
    )
    .await;
    assert_eq!(load_lazy["result"]["isError"], false);
    let load_lazy_payload = mcp_call_text_json(&load_lazy);
    assert_eq!(
        load_lazy_payload["received_arguments"]["activate_groups"],
        false
    );

    let single = post_mcp_json(
        &client,
        &url,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
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
    use std::sync::{Arc, Mutex};

    let state: Arc<Mutex<&'static str>> = Arc::new(Mutex::new("bake_animation"));
    let state_clone = state.clone();

    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
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

#[tokio::test]
async fn aggregate_resources_list_merges_admin_pointers_and_backend_resources() {
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

    assert!(
        uris.iter().any(|u| u.starts_with("dcc://maya/")),
        "admin pointer for maya instance missing: {uris:?}",
    );
    assert!(
        uris.iter().any(|u| u.starts_with("dcc://blender/")),
        "admin pointer for blender instance missing: {uris:?}",
    );

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

    assert!(
        !uris.contains(&"scene://current"),
        "unprefixed backend URI leaked: {uris:?}",
    );

    let _ = stop_a.send(());
    let _ = stop_b.send(());
}

#[tokio::test]
async fn aggregate_resources_list_fail_soft_when_one_backend_is_dead() {
    let (port_live, stop_live) = spawn_resources_backend(vec![
        json!({"uri": "scene://current", "name": "live scene", "mimeType": "application/json"}),
    ])
    .await;
    let dead_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = dead_listener.local_addr().unwrap().port();
    drop(dead_listener);

    let (gs, _dir, ids) =
        gateway_state_with_instances(&[("maya", port_live), ("blender", dead_port)]).await;
    let id_live = ids[0];

    let result = aggregate_resources_list(&gs).await;
    let resources = result["resources"]
        .as_array()
        .expect("resources must be an array");
    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();

    assert!(
        uris.contains(&&*format!("scene://{}/current", id8(&id_live))),
        "live backend's prefixed URI missing: {uris:?}",
    );
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
