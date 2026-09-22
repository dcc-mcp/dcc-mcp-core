//! Multi-host `list_skills` fan-out paging (issue PIP-3407).
//!
//! Split out of `tool_tests.rs`: that file sits close to the 2000-line test
//! gate, and these cases cover one contract — the gateway merges every live
//! host and pages the union exactly once.

use super::helpers::gateway_state_with_instances;
use dcc_mcp_skills::catalog::list_projection::MAX_LIST_SKILLS_LIMIT;
use serde_json::{Value, json};

/// Backend that serves `count` skills through the real `list_skills`
/// projection, recording every argument set the gateway forwards.
async fn spawn_list_skills_backend(
    dcc: &'static str,
    count: usize,
    seen: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
) -> (u16, tokio::sync::oneshot::Sender<()>) {
    use dcc_mcp_skills::catalog::{SkillSummary, list_projection::build_list_skills_response};

    let summaries: Vec<SkillSummary> = (0..count)
        .map(|i| SkillSummary {
            name: format!("{dcc}-skill-{i:03}"),
            description: "x".repeat(80),
            search_hint: String::new(),
            tags: Vec::new(),
            dcc: dcc.to_string(),
            version: "1.0.0".to_string(),
            tool_count: 1,
            tool_names: vec![format!("{dcc}_tool_{i}")],
            loaded: false,
            status: "discovered".to_string(),
            missing_dependencies: Vec::new(),
            scope: "repo".to_string(),
            path_source: "project".to_string(),
            implicit_invocation: true,
            layer: Some("domain".to_string()),
            stage: Some("model".to_string()),
            runtime: None,
        })
        .collect();

    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let seen = seen.clone();
                let summaries = summaries.clone();
                async move {
                    let args = body
                        .get("params")
                        .and_then(|p| p.get("arguments"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    if let Ok(mut guard) = seen.lock() {
                        guard.push(args.clone());
                    }
                    let text = serde_json::to_string(&build_list_skills_response(summaries, &args))
                        .unwrap();
                    axum::Json(json!({
                        "jsonrpc": "2.0",
                        "id": body.get("id").cloned().unwrap_or(Value::Null),
                        "result": {
                            "content": [{"type": "text", "text": text}],
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
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (port, shutdown_tx)
}

/// PIP-3407: a default `list_skills` fan-out must return one bounded page of
/// the *merged* catalogue, and walking `next_offset` must reach every skill of
/// every host exactly once.
#[tokio::test]
async fn list_skills_fanout_pages_the_merged_catalogue() {
    use std::sync::{Arc, Mutex};

    let seen_a: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_b: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_c: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let (port_a, stop_a) = spawn_list_skills_backend("maya", 29, seen_a.clone()).await;
    let (port_b, stop_b) = spawn_list_skills_backend("blender", 36, seen_b.clone()).await;
    let (port_c, stop_c) = spawn_list_skills_backend("houdini", 43, seen_c.clone()).await;
    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_a), ("blender", port_b), ("houdini", port_c)])
            .await;

    let (text, is_error) =
        crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(&gs, "list_skills", &json!({}))
            .await;
    assert!(!is_error, "{text}");
    let first: Value = serde_json::from_str(&text).unwrap();

    // Bounded: 108 merged skills, one page of 25.
    assert_eq!(first["total"], 108, "{first:#}");
    assert_eq!(first["skills"].as_array().unwrap().len(), 25, "{first:#}");
    assert_eq!(first["truncated"], true, "{first:#}");
    assert_eq!(first["next_offset"], 25, "{first:#}");
    assert!(first["next_step"].as_str().is_some(), "{first:#}");

    // Paging happens once, on the merged result: backends are drained with
    // the gateway's own bounded page size, never with the *caller's*
    // `limit`/`offset` applied to each host's own catalogue.
    for seen in [&seen_a, &seen_b, &seen_c] {
        let args = seen.lock().unwrap();
        assert_eq!(args.len(), 1, "a host this small fits in one page");
        assert_eq!(
            args[0].get("limit").and_then(Value::as_u64),
            Some(MAX_LIST_SKILLS_LIMIT as u64),
            "backend page must be the internal cap, not the caller's: {}",
            args[0]
        );
        assert_eq!(
            args[0].get("offset").and_then(Value::as_u64),
            Some(0),
            "the walk must start at the first page: {}",
            args[0]
        );
        assert!(
            args[0].get("fields").and_then(Value::as_array).is_some(),
            "backends must be asked for every field: {}",
            args[0]
        );
    }

    // Walk every page: the union must be the whole merged catalogue.
    let mut names: Vec<String> = first["skills"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    let mut offset = 25usize;
    loop {
        let (page_text, page_error) = crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(
            &gs,
            "list_skills",
            &json!({"offset": offset}),
        )
        .await;
        assert!(!page_error, "{page_text}");
        let page: Value = serde_json::from_str(&page_text).unwrap();
        assert_eq!(page["total"], 108, "{page:#}");
        names.extend(
            page["skills"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|s| s.get("name").and_then(Value::as_str))
                .map(str::to_string),
        );
        match page.get("next_offset").and_then(Value::as_u64) {
            Some(next) => offset = next as usize,
            None => break,
        }
        assert!(names.len() <= 108 + 25, "walk is not terminating");
    }

    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(names.len(), 108, "paging duplicated rows: {names:?}");
    assert_eq!(unique.len(), 108, "paging lost rows: {unique:?}");
    for dcc in ["maya", "blender", "houdini"] {
        assert!(
            unique.iter().any(|n| n.starts_with(dcc)),
            "{dcc} skills missing from the walk"
        );
    }

    let _ = stop_a.send(());
    let _ = stop_b.send(());
    let _ = stop_c.send(());
}

/// A host whose catalogue is larger than one backend page must still be
/// drained completely: the gateway walks each host with bounded pages and
/// follows `next_offset`, so no skill may be dropped just because it lives
/// past the first page of its own host.
#[tokio::test]
async fn list_skills_fanout_walks_a_host_larger_than_one_page() {
    use std::sync::{Arc, Mutex};

    let big = MAX_LIST_SKILLS_LIMIT * 2 + 7; // 107 skills -> 3 backend pages
    let seen_big: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_small: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let (port_big, stop_big) = spawn_list_skills_backend("maya", big, seen_big.clone()).await;
    let (port_small, stop_small) =
        spawn_list_skills_backend("blender", 12, seen_small.clone()).await;
    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_big), ("blender", port_small)]).await;

    let mut names: Vec<String> = Vec::new();
    let mut offset = 0usize;
    let mut rounds = 0usize;
    loop {
        rounds += 1;
        let (text, is_error) = crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(
            &gs,
            "list_skills",
            &json!({"offset": offset, "limit": MAX_LIST_SKILLS_LIMIT}),
        )
        .await;
        assert!(!is_error, "{text}");
        let page: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            page["total"].as_u64().unwrap() as usize,
            big + 12,
            "every page reports the merged catalogue size"
        );
        names.extend(
            page["skills"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|s| s.get("name").and_then(Value::as_str))
                .map(str::to_string),
        );
        match page.get("next_offset").and_then(Value::as_u64) {
            Some(next) => {
                let next = next as usize;
                assert!(next > offset, "next_offset did not advance");
                offset = next;
            }
            None => break,
        }
        assert!(
            names.len() <= big + 12 + MAX_LIST_SKILLS_LIMIT,
            "not terminating"
        );
    }

    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        big + 12,
        "a host larger than one page lost or duplicated skills"
    );

    // The fan-out is stateless, so every caller page re-walks each host:
    // the big host costs 3 bound pages per dispatch, the small one 1.
    assert_eq!(
        rounds,
        3,
        "{} skills at pages of {MAX_LIST_SKILLS_LIMIT}",
        big + 12
    );
    let big_calls = seen_big.lock().unwrap().len();
    let small_calls = seen_small.lock().unwrap().len();
    assert_eq!(
        big_calls,
        3 * rounds,
        "{big} skills need 3 bounded pages per dispatch"
    );
    assert_eq!(
        small_calls, rounds,
        "a host that fits one page must cost one request per dispatch"
    );

    let _ = stop_big.send(());
    let _ = stop_small.send(());
}
