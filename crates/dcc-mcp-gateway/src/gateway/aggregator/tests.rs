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
