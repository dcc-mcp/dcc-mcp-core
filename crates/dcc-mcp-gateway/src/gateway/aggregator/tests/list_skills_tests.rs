//! Multi-host `list_skills` fan-out paging (issue PIP-3407).
//!
//! Split out of `tool_tests.rs`: that file sits close to the 2000-line test
//! gate, and these cases cover one contract — the gateway merges every live
//! host and pages the union exactly once.

use super::helpers::gateway_state_with_instances;
use dcc_mcp_skills::catalog::list_projection::MAX_LIST_SKILLS_LIMIT;
use serde_json::{Value, json};

/// `count` skills named `<dcc>-skill-<n>` for one host.
fn host_summaries(dcc: &str, count: usize) -> Vec<dcc_mcp_skills::catalog::SkillSummary> {
    use dcc_mcp_skills::catalog::SkillSummary;

    (0..count)
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
        .collect()
}

/// Backend that serves `count` skills through the real `list_skills`
/// projection, recording every argument set the gateway forwards.
async fn spawn_list_skills_backend(
    dcc: &'static str,
    count: usize,
    seen: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
) -> (u16, tokio::sync::oneshot::Sender<()>) {
    spawn_summary_backend(host_summaries(dcc, count), seen).await
}

/// Backend that serves an explicit summary set (so two hosts can publish
/// colliding skill names) through the real `list_skills` projection.
async fn spawn_summary_backend(
    summaries: Vec<dcc_mcp_skills::catalog::SkillSummary>,
    seen: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
) -> (u16, tokio::sync::oneshot::Sender<()>) {
    use dcc_mcp_skills::catalog::list_projection::build_list_skills_response;

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

/// Backend that answers every `tools/call` with canned `list_skills` pages and
/// an explicit MCP `isError` flag.
///
/// The real projection can never emit a truncated page without a usable
/// cursor, so those misbehaving-host paths have to be driven by a scripted
/// backend. `pages` is replayed in order; the last page repeats once the
/// script runs out.
async fn spawn_scripted_list_skills_backend(
    pages: Vec<Value>,
    is_error: bool,
) -> (u16, tokio::sync::oneshot::Sender<()>) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let pages = pages.clone();
                let calls = calls.clone();
                async move {
                    let index = calls.fetch_add(1, Ordering::SeqCst).min(pages.len() - 1);
                    let text = serde_json::to_string(&pages[index]).unwrap();
                    axum::Json(json!({
                        "jsonrpc": "2.0",
                        "id": body.get("id").cloned().unwrap_or(Value::Null),
                        "result": {
                            "content": [{"type": "text", "text": text}],
                            "isError": is_error,
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

/// Backend that answers every page with `truncated: true` and a strictly
/// increasing `next_offset`, so the walk never terminates on its own.
///
/// Returns the request counter so a test can assert exactly where the page
/// budget stopped the walk (PIP-3435).
async fn spawn_forever_paging_backend(
    total: usize,
    page: usize,
) -> (
    u16,
    tokio::sync::oneshot::Sender<()>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
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
                    let offset = body
                        .get("params")
                        .and_then(|p| p.get("arguments"))
                        .and_then(|a| a.get("offset"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    calls.fetch_add(1, Ordering::SeqCst);
                    let text = serde_json::to_string(&json!({
                        "skills": [{"name": format!("ghost-{offset}"), "dcc": "blender"}],
                        "total": total,
                        "truncated": true,
                        "next_offset": offset + page,
                    }))
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
    (port, shutdown_tx, counter)
}

/// Backend that answers every `tools/call` with a raw text payload the fan-out
/// cannot parse — the shape a proxy error page or a plain-string tool result
/// produces (PIP-3435).
async fn spawn_raw_text_backend(text: &'static str) -> (u16, tokio::sync::oneshot::Sender<()>) {
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| async move {
                axum::Json(json!({
                    "jsonrpc": "2.0",
                    "id": body.get("id").cloned().unwrap_or(Value::Null),
                    "result": {
                        "content": [{"type": "text", "text": text}],
                        "isError": false,
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
    (port, shutdown_tx)
}

/// Pull the per-instance entry the fan-out reported for one `dcc_type`.
fn instance_entry(payload: &Value, dcc: &str) -> Value {
    payload["instances"]
        .as_array()
        .unwrap_or_else(|| panic!("no instances in {payload:#}"))
        .iter()
        .find(|entry| entry.get("dcc_type").and_then(Value::as_str) == Some(dcc))
        .unwrap_or_else(|| panic!("no {dcc} instance in {payload:#}"))
        .clone()
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

/// PIP-3432 (P2): a backend that reports `truncated: true` but offers no usable
/// `next_offset` must be reported as a failed host, not merged as a short
/// catalogue that looks complete. Otherwise the caller sees a partial union
/// with `truncated: false` and no way to know skills went missing.
#[tokio::test]
async fn list_skills_fanout_rejects_truncation_without_a_usable_cursor() {
    use std::sync::{Arc, Mutex};

    let seen_good: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let (port_good, stop_good) = spawn_list_skills_backend("maya", 5, seen_good.clone()).await;

    // Every shape of unusable cursor a version-mismatched backend can emit:
    // missing, non-numeric, repeated (== offset) and decreasing.
    let broken_pages = vec![
        json!({"skills": [{"name": "broken-1", "dcc": "blender"}], "total": 60, "truncated": true}),
        json!({"skills": [], "total": 60, "truncated": true, "next_offset": "not-a-number"}),
        json!({"skills": [], "total": 60, "truncated": true, "next_offset": 0}),
    ];
    let (port_bad, stop_bad) = spawn_scripted_list_skills_backend(broken_pages, false).await;

    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_good), ("blender", port_bad)]).await;

    for broken_page in [
        "missing next_offset",
        "non-numeric next_offset",
        "repeated next_offset",
    ] {
        let (text, is_error) = crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(
            &gs,
            "list_skills",
            &json!({}),
        )
        .await;
        // One healthy host kept the fan-out from being a total failure.
        assert!(!is_error, "{broken_page}: {text}");
        let payload: Value = serde_json::from_str(&text).unwrap();

        let blender = instance_entry(&payload, "blender");
        let error = blender
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or_else(|| {
                panic!("{broken_page}: blender was not reported as failed: {blender:#}")
            });
        assert!(
            error.contains("usable next_offset"),
            "{broken_page}: unexpected error text {error}"
        );
        assert!(
            instance_entry(&payload, "maya").get("error").is_none(),
            "{broken_page}: the healthy host must not be marked failed"
        );

        // The broken host's rows must not reach the merged catalogue, and the
        // union must not claim to be complete at the wrong size.
        let names: Vec<&str> = payload["skills"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s.get("name").and_then(Value::as_str))
            .collect();
        assert_eq!(
            names.len(),
            5,
            "{broken_page}: only the healthy host may be merged: {names:?}"
        );
        assert!(
            names.iter().all(|n| n.starts_with("maya")),
            "{broken_page}: broken-host rows leaked into the union: {names:?}"
        );
        assert_eq!(payload["total"], 5, "{broken_page}: {payload:#}");
    }

    let _ = stop_good.send(());
    let _ = stop_bad.send(());
}

/// PIP-3432 (P2): a cursor that walks *backwards* is as unusable as a missing
/// one — following it would loop forever, and stopping silently would drop
/// everything past the current page.
#[tokio::test]
async fn list_skills_fanout_rejects_a_decreasing_cursor() {
    use std::sync::{Arc, Mutex};

    let seen_good: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let (port_good, stop_good) = spawn_list_skills_backend("maya", 2, seen_good.clone()).await;
    let (port_bad, stop_bad) = spawn_scripted_list_skills_backend(
        vec![
            json!({"skills": [{"name": "broken-1", "dcc": "blender"}], "total": 60, "truncated": true, "next_offset": 3}),
            json!({"skills": [], "total": 60, "truncated": true, "next_offset": 2}),
        ],
        false,
    )
    .await;

    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_good), ("blender", port_bad)]).await;

    let (text, is_error) =
        crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(&gs, "list_skills", &json!({}))
            .await;
    assert!(!is_error, "{text}");
    let payload: Value = serde_json::from_str(&text).unwrap();

    let blender = instance_entry(&payload, "blender");
    let error = blender
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("blender was not reported as failed: {blender:#}"));
    assert!(
        error.contains("usable next_offset"),
        "unexpected error text {error}"
    );
    assert_eq!(
        payload["total"], 2,
        "only the healthy host may be merged: {payload:#}"
    );

    let _ = stop_good.send(());
    let _ = stop_bad.send(());
}

/// PIP-3432 (P3): the fan-out used to read only the text of a backend result,
/// so a host that flagged `isError: true` was counted as healthy and whatever
/// it returned was merged into the catalogue.
#[tokio::test]
async fn list_skills_fanout_honours_the_backend_error_flag() {
    use std::sync::{Arc, Mutex};

    let seen_good: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let (port_good, stop_good) = spawn_list_skills_backend("maya", 4, seen_good.clone()).await;
    let (port_bad, stop_bad) = spawn_scripted_list_skills_backend(
        vec![json!({
            "skills": [{"name": "ghost-skill", "dcc": "blender"}],
            "total": 1,
            "truncated": false,
        })],
        true,
    )
    .await;

    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_good), ("blender", port_bad)]).await;

    let (text, is_error) =
        crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(&gs, "list_skills", &json!({}))
            .await;
    assert!(!is_error, "{text}");
    let payload: Value = serde_json::from_str(&text).unwrap();

    let blender = instance_entry(&payload, "blender");
    assert!(
        blender.get("error").is_some(),
        "an isError backend must be reported as failed: {blender:#}"
    );
    let names: Vec<&str> = payload["skills"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .collect();
    assert!(
        !names.contains(&"ghost-skill"),
        "a failed host's rows must not be merged: {names:?}"
    );
    assert_eq!(payload["total"], 4, "{payload:#}");

    let _ = stop_good.send(());
    let _ = stop_bad.send(());
}

/// PIP-3432 (P3): the same gap on the `search_skills` fan-out, which shares
/// [`flatten_skill_list_results`] with the non-walked paths.
#[tokio::test]
async fn search_skills_fanout_honours_the_backend_error_flag() {
    use std::sync::{Arc, Mutex};

    let seen_good: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let (port_good, stop_good) = spawn_list_skills_backend("maya", 3, seen_good.clone()).await;
    let (port_bad, stop_bad) = spawn_scripted_list_skills_backend(
        vec![json!({
            "skills": [{"name": "ghost-skill", "dcc": "blender"}],
            "total": 1,
        })],
        true,
    )
    .await;

    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_good), ("blender", port_bad)]).await;

    let (text, is_error) = crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(
        &gs,
        "search_skills",
        &json!({"query": "skill"}),
    )
    .await;
    assert!(!is_error, "{text}");
    let payload: Value = serde_json::from_str(&text).unwrap();

    let blender = instance_entry(&payload, "blender");
    assert!(
        blender.get("error").is_some(),
        "an isError backend must be reported as failed: {blender:#}"
    );
    let names: Vec<&str> = payload["skills"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .collect();
    assert!(
        !names.contains(&"ghost-skill"),
        "a failed host's rows must not be merged: {names:?}"
    );

    let _ = stop_good.send(());
    let _ = stop_bad.send(());
}

/// PIP-3432 (P3): two hosts publishing the same skill names must page in a
/// reproducible order. The union is sorted by name only, and the fan-out order
/// it would otherwise fall back to comes out of a `DashMap`, so without a
/// tie-breaker on the host identity a walk can reshuffle mid-page.
#[tokio::test]
async fn list_skills_fanout_pages_same_named_skills_stably() {
    use std::sync::{Arc, Mutex};

    // More colliding names than one page, so the tie-breaker has to hold
    // across a page boundary too.
    let names: Vec<String> = (0..30).map(|i| format!("shared-skill-{i:03}")).collect();
    let mut maya = host_summaries("maya", names.len());
    let mut blender = host_summaries("blender", names.len());
    for (index, name) in names.iter().enumerate() {
        maya[index].name = name.clone();
        blender[index].name = name.clone();
    }

    let seen_a: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_b: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let (port_a, stop_a) = spawn_summary_backend(maya, seen_a.clone()).await;
    let (port_b, stop_b) = spawn_summary_backend(blender, seen_b.clone()).await;
    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_a), ("blender", port_b)]).await;

    let mut rows: Vec<(String, String)> = Vec::new();
    let mut offset = 0usize;
    loop {
        let (text, is_error) = crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(
            &gs,
            "list_skills",
            &json!({"offset": offset}),
        )
        .await;
        assert!(!is_error, "{text}");
        let page: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(page["total"], names.len() * 2, "{page:#}");
        rows.extend(page["skills"].as_array().unwrap().iter().map(|row| {
            (
                row.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                row.get("dcc")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            )
        }));
        match page.get("next_offset").and_then(Value::as_u64) {
            Some(next) => offset = next as usize,
            None => break,
        }
        assert!(
            rows.len() <= names.len() * 2 + 25,
            "walk is not terminating"
        );
    }

    // Every (name, dcc) pair reached exactly once...
    assert_eq!(rows.len(), names.len() * 2, "walk lost or duplicated rows");
    let mut unique = rows.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), rows.len(), "walk duplicated rows");

    // ...and the whole walk is ordered, so a name collision never reshuffles
    // between pages.
    let mut sorted = rows.clone();
    sorted.sort();
    assert_eq!(
        rows, sorted,
        "merged rows are not in a stable order: {rows:?}"
    );
    for pair in rows.windows(2) {
        if pair[0].0 == pair[1].0 {
            assert!(
                pair[0].1 <= pair[1].1,
                "same-named rows out of order: {:?}",
                pair
            );
        }
    }

    let _ = stop_a.send(());
    let _ = stop_b.send(());
}

/// PIP-3435 (P3): a failed host must report `skill_count: 0`.
///
/// It used to report how many rows the walk had already collected, but those
/// rows are deliberately dropped from the union, so `skill_count` described a
/// contribution that never reached the caller.
#[tokio::test]
async fn list_skills_failed_host_reports_zero_skill_count() {
    use std::sync::{Arc, Mutex};

    let seen_good: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let (port_good, stop_good) = spawn_list_skills_backend("maya", 7, seen_good.clone()).await;
    // Two collected rows and no usable cursor: the walk fails while holding
    // rows the union is about to discard.
    let (port_bad, stop_bad) = spawn_scripted_list_skills_backend(
        vec![json!({
            "skills": [
                {"name": "dropped-1", "dcc": "blender"},
                {"name": "dropped-2", "dcc": "blender"},
            ],
            "total": 60,
            "truncated": true,
        })],
        false,
    )
    .await;

    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_good), ("blender", port_bad)]).await;

    let (text, is_error) =
        crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(&gs, "list_skills", &json!({}))
            .await;
    assert!(!is_error, "{text}");
    let payload: Value = serde_json::from_str(&text).unwrap();

    let blender = instance_entry(&payload, "blender");
    assert!(
        blender.get("error").is_some(),
        "blender must be reported as failed: {blender:#}"
    );
    assert_eq!(
        blender.get("skill_count").and_then(Value::as_u64),
        Some(0),
        "a failed host contributes 0 rows, not the rows it collected: {blender:#}"
    );

    // The invariant the fix restores: every row in the union is accounted for
    // by exactly one instance summary.
    let summed: u64 = payload["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            entry
                .get("skill_count")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        })
        .sum();
    assert_eq!(
        summed,
        payload["total"].as_u64().unwrap(),
        "sum(instances[].skill_count) must equal total: {payload:#}"
    );

    let _ = stop_good.send(());
    let _ = stop_bad.send(());
}

/// PIP-3435 (P3): a host that keeps answering `truncated: true` with a
/// strictly increasing `next_offset` must not page forever.
///
/// `backend_timeout` bounds one round trip, not the number of rounds, so
/// without a page budget this walk never returns.
#[tokio::test]
async fn list_skills_walk_stops_at_the_page_budget() {
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};

    let seen_good: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let (port_good, stop_good) = spawn_list_skills_backend("maya", 3, seen_good.clone()).await;

    // Claims a two-page catalogue, then offers another page on every request.
    let total = MAX_LIST_SKILLS_LIMIT * 2;
    let (port_bad, stop_bad, calls) =
        spawn_forever_paging_backend(total, MAX_LIST_SKILLS_LIMIT).await;

    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_good), ("blender", port_bad)]).await;

    let (text, is_error) =
        crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(&gs, "list_skills", &json!({}))
            .await;
    assert!(
        !is_error,
        "one healthy host keeps the fan-out usable: {text}"
    );

    // A truthful `total` of 100 at 50 rows a page is a two-page catalogue, so
    // the budget is 100 / 50 + 1 = 3 pages.
    let budget = total / MAX_LIST_SKILLS_LIMIT + 1;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        budget,
        "the walk did not stop at its page budget"
    );

    let payload: Value = serde_json::from_str(&text).unwrap();
    let blender = instance_entry(&payload, "blender");
    let error = blender
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("blender was not reported as failed: {blender:#}"));
    assert!(
        error.contains("page budget"),
        "unexpected error text {error}"
    );
    assert_eq!(
        payload["total"], 3,
        "only the healthy host may be merged: {payload:#}"
    );

    let _ = stop_good.send(());
    let _ = stop_bad.send(());
}

/// PIP-3435 (P3): a fan-out where no host returned parseable text used to
/// report success over an empty union — `ok_count` was incremented before the
/// payload was parsed, so no host was ever counted as failed.
#[tokio::test]
async fn search_skills_reports_error_when_no_host_returns_json() {
    let (port_a, stop_a) = spawn_raw_text_backend("not json at all").await;
    let (port_b, stop_b) = spawn_raw_text_backend("<html>502 Bad Gateway</html>").await;
    let (gs, _dir, _ids) =
        gateway_state_with_instances(&[("maya", port_a), ("blender", port_b)]).await;

    let (text, is_error) = crate::gateway::aggregator::skill_mgmt::skill_mgmt_dispatch(
        &gs,
        "search_skills",
        &json!({"query": "skill"}),
    )
    .await;
    assert!(
        is_error,
        "no host returned JSON, so the fan-out must not claim success: {text}"
    );

    let payload: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(payload["total"], 0, "{payload:#}");
    assert_eq!(
        payload["instances"].as_array().map(Vec::len),
        Some(2),
        "both hosts must be reported: {payload:#}"
    );

    let _ = stop_a.send(());
    let _ = stop_b.send(());
}
