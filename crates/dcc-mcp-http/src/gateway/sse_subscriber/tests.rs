use super::*;

#[test]
fn backoff_delay_starts_near_initial() {
    let d = backoff_delay(1);
    // first attempt — base 100 ms ± 25 %
    assert!(d >= Duration::from_millis(75));
    assert!(d <= Duration::from_millis(125));
}

#[test]
fn backoff_delay_grows_exponentially_and_caps() {
    // large attempt — must not exceed RECONNECT_MAX + 25 %
    let cap_with_jitter = (RECONNECT_MAX.as_millis() as f32 * 1.25) as u128;
    for attempt in 1..30u32 {
        let d = backoff_delay(attempt).as_millis();
        assert!(
            d <= cap_with_jitter,
            "attempt={attempt} delay={d}ms exceeds cap {cap_with_jitter}ms"
        );
    }
    // At attempt=20 we are definitely saturated near the cap.
    let d = backoff_delay(20).as_millis();
    let floor = (RECONNECT_MAX.as_millis() as f32 * 0.75) as u128;
    assert!(d >= floor, "saturated backoff={d}ms below floor {floor}ms");
}

#[test]
fn progress_token_key_distinguishes_string_and_number_tokens() {
    let s = progress_token_key(&Value::String("abc".into()));
    let n = progress_token_key(&serde_json::json!(42));
    let n_str = progress_token_key(&Value::String("42".into()));
    assert_ne!(s, n);
    assert_ne!(n, n_str);
}

#[test]
fn parse_sse_record_extracts_json_from_data_field() {
    let rec = b"data: {\"method\":\"notifications/progress\",\"params\":{\"progress\":1}}";
    let v = parse_sse_record(rec).expect("valid record");
    assert_eq!(v["method"], "notifications/progress");
}

#[test]
fn parse_sse_record_handles_multiline_data_and_id_field() {
    // Two `data:` lines must be concatenated with '\n' per
    // WHATWG SSE spec. We check both that the parse does not
    // panic on a multi-line record and that non-data lines
    // (`id:`, `event:`) are skipped.
    let rec = b"id: 7\nevent: message\ndata: {\"a\":1,\ndata: \"b\":2}";
    let v = parse_sse_record(rec).expect("multi-line data: joins cleanly");
    assert_eq!(v["a"], 1);
    assert_eq!(v["b"], 2);
}

#[test]
fn parse_sse_record_returns_none_for_record_without_data_field() {
    let rec = b"event: endpoint\n";
    assert!(parse_sse_record(rec).is_none());
}

fn empty_inner() -> SubscriberManagerInner {
    SubscriberManagerInner {
        backends: DashMap::new(),
        job_routes: DashMap::new(),
        session_jobs: DashMap::new(),
        request_to_job: DashMap::new(),
        progress_token_routes: DashMap::new(),
        backend_inflight: DashMap::new(),
        client_sinks: DashMap::new(),
        job_event_buses: DashMap::new(),
        http_client: reqwest::Client::new(),
        route_ttl: DEFAULT_ROUTE_TTL,
        max_routes_per_session: DEFAULT_MAX_ROUTES_PER_SESSION,
    }
}

fn test_route(sid: &str) -> JobRoute {
    JobRoute {
        client_session_id: sid.to_string(),
        backend_id: "http://backend/mcp".into(),
        tool: "test_tool".into(),
        created_at: Utc::now(),
        parent_job_id: None,
    }
}

#[test]
fn resolve_target_prefers_progress_token_for_progress_notifications() {
    let inner = empty_inner();
    inner.progress_token_routes.insert(
        progress_token_key(&Value::String("tok".into())),
        "sessA".into(),
    );
    let note = serde_json::json!({
        "method": "notifications/progress",
        "params": {"progressToken": "tok", "progress": 5, "total": 10}
    });
    assert_eq!(resolve_target(&inner, &note).as_deref(), Some("sessA"));
}

#[test]
fn resolve_target_uses_job_id_for_job_updated() {
    let inner = empty_inner();
    inner
        .job_routes
        .insert("jid-42".into(), test_route("sessB"));
    let note = serde_json::json!({
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "jid-42", "status": "running"}
    });
    assert_eq!(resolve_target(&inner, &note).as_deref(), Some("sessB"));
}

#[test]
fn resolve_target_returns_none_when_unknown() {
    let inner = empty_inner();
    let note = serde_json::json!({
        "method": "notifications/progress",
        "params": {"progressToken": "no-such-token"}
    });
    assert!(resolve_target(&inner, &note).is_none());
}

// #321: per-job broadcast delivery — unit tests here, end-to-end
// wiring is covered by `gateway/tests.rs`.

#[tokio::test]
async fn job_event_channel_receives_published_notifications() {
    let mgr = SubscriberManager::default();
    let mut rx = mgr.job_event_channel("job-1");
    let note = serde_json::json!({
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "job-1", "status": "completed"}
    });
    mgr.publish_job_event("job-1", &note);
    let delivered = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("recv did not time out")
        .expect("bus delivered");
    assert_eq!(delivered["params"]["status"], "completed");
}

#[tokio::test]
async fn job_event_channel_publishes_only_to_requested_job() {
    let mgr = SubscriberManager::default();
    let mut rx_a = mgr.job_event_channel("job-a");
    let mut rx_b = mgr.job_event_channel("job-b");
    let note = serde_json::json!({
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "job-a", "status": "running"}
    });
    mgr.publish_job_event("job-a", &note);
    assert!(rx_a.try_recv().is_ok());
    assert!(rx_b.try_recv().is_err());
}

#[tokio::test]
async fn deliver_publishes_to_job_event_bus_even_without_route() {
    // The waiter path does NOT require `bind_job` — it subscribes to
    // the per-job bus directly before the reply arrives. `deliver`
    // must therefore publish to the bus regardless of whether a
    // client-session route exists.
    let mgr = SubscriberManager::default();
    let mut rx = mgr.job_event_channel("job-x");
    let backend = "http://127.0.0.1:0/mcp".to_string();
    let shared = Arc::new(BackendShared::new(backend.clone()));
    let note = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "job-x", "status": "completed"}
    });
    mgr.deliver(note, &shared);
    let delivered = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("recv did not time out")
        .expect("bus delivered");
    assert_eq!(delivered["params"]["status"], "completed");
}

#[test]
fn forget_job_bus_removes_the_broadcast() {
    let mgr = SubscriberManager::default();
    let _rx = mgr.job_event_channel("job-1");
    assert!(mgr.inner.job_event_buses.contains_key("job-1"));
    mgr.forget_job_bus("job-1");
    assert!(!mgr.inner.job_event_buses.contains_key("job-1"));
}

#[tokio::test]
async fn manager_buffers_then_flushes_after_job_binding() {
    // Stand up a manager, register a client, hand-feed a notification
    // whose job_id mapping is not yet known, then bind the mapping
    // and assert the buffered event is delivered.
    let mgr = SubscriberManager::default();
    let mut rx = mgr.register_client("sess1");
    let backend = "http://127.0.0.1:0/mcp".to_string();
    // Fake a backend entry so buffer operations resolve.
    let shared = Arc::new(BackendShared::new(backend.clone()));
    mgr.inner.backends.insert(
        backend.clone(),
        BackendSubscriber {
            url: backend.clone(),
            task: None,
            shared: shared.clone(),
        },
    );

    let note = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "job-1", "status": "running"}
    });
    mgr.deliver(note.clone(), &shared);
    assert_eq!(mgr.pending_count(&backend), 1, "buffered while unresolved");
    assert!(rx.try_recv().is_err(), "nothing delivered yet");

    mgr.bind_job("job-1", "sess1", &backend);
    // After bind, the flush is triggered synchronously.
    assert_eq!(mgr.pending_count(&backend), 0, "buffer drained");
    let event = rx
        .try_recv()
        .expect("event should have been flushed to sink");
    assert!(event.contains("notifications/$/dcc.jobUpdated"));
}

#[tokio::test]
async fn manager_emits_gateway_reconnect_to_inflight_sessions() {
    let mgr = SubscriberManager::default();
    let mut rx = mgr.register_client("sess1");
    let backend = "http://127.0.0.1:0/mcp".to_string();
    let shared = Arc::new(BackendShared::new(backend.clone()));
    mgr.inner.backends.insert(
        backend.clone(),
        BackendSubscriber {
            url: backend.clone(),
            task: None,
            shared,
        },
    );
    mgr.bind_job("job-x", "sess1", &backend);

    mgr.emit_gateway_reconnect(&backend);

    let event = rx.try_recv().expect("gatewayReconnect should be delivered");
    assert!(event.contains("notifications/$/dcc.gatewayReconnect"));
    assert!(event.contains(&backend));
}

#[tokio::test]
async fn manager_drops_events_for_forgotten_client() {
    let mgr = SubscriberManager::default();
    let _rx = mgr.register_client("sess1");
    mgr.forget_client("sess1");

    let backend = "http://127.0.0.1:0/mcp".to_string();
    let shared = Arc::new(BackendShared::new(backend.clone()));
    mgr.inner.backends.insert(
        backend.clone(),
        BackendSubscriber {
            url: backend.clone(),
            task: None,
            shared: shared.clone(),
        },
    );
    mgr.bind_job("job-gone", "sess1", &backend);
    let note = serde_json::json!({
        "jsonrpc":"2.0",
        "method":"notifications/$/dcc.jobUpdated",
        "params":{"job_id":"job-gone","status":"running"}
    });
    // Must not panic; simply drops.
    mgr.deliver(note, &shared);
}

#[test]
fn pending_buffer_evicts_oldest_when_full() {
    let mgr = SubscriberManager::default();
    let backend = "http://127.0.0.1:0/mcp".to_string();
    let shared = Arc::new(BackendShared::new(backend.clone()));
    mgr.inner.backends.insert(
        backend.clone(),
        BackendSubscriber {
            url: backend.clone(),
            task: None,
            shared: shared.clone(),
        },
    );
    for i in 0..(PENDING_BUFFER_CAP + 5) {
        let note = serde_json::json!({
            "method":"notifications/$/dcc.jobUpdated",
            "params":{"job_id": format!("j{i}"), "status":"running"}
        });
        mgr.deliver(note, &shared);
    }
    assert_eq!(
        mgr.pending_count(&backend),
        PENDING_BUFFER_CAP,
        "buffer is bounded"
    );
}

// ── #322 JobRoute store ─────────────────────────────────────────────

#[test]
fn bind_job_route_populates_all_fields() {
    let mgr = SubscriberManager::default();
    mgr.bind_job_route("j1", "sessA", "http://back/mcp", "my_tool", Some("parent"))
        .unwrap();
    let route = mgr.job_route("j1").expect("route present");
    assert_eq!(route.client_session_id, "sessA");
    assert_eq!(route.backend_id, "http://back/mcp");
    assert_eq!(route.tool, "my_tool");
    assert_eq!(route.parent_job_id.as_deref(), Some("parent"));
}

#[test]
fn bind_request_to_job_resolves_back_to_route() {
    let mgr = SubscriberManager::default();
    mgr.bind_job_route("j1", "sessA", "http://back/mcp", "t", None)
        .unwrap();
    mgr.bind_request_to_job("\"req-7\"", "j1");
    let jid = mgr.job_id_for_request("\"req-7\"").expect("mapping");
    assert_eq!(jid, "j1");
    let route = mgr.job_route(&jid).unwrap();
    assert_eq!(route.backend_id, "http://back/mcp");
}

#[test]
fn children_of_returns_every_child_of_parent() {
    let mgr = SubscriberManager::default();
    mgr.bind_job_route("c1", "s", "http://a/mcp", "t", Some("P"))
        .unwrap();
    mgr.bind_job_route("c2", "s", "http://b/mcp", "t", Some("P"))
        .unwrap();
    mgr.bind_job_route("other", "s", "http://c/mcp", "t", Some("Q"))
        .unwrap();
    let mut kids: Vec<String> = mgr.children_of("P").into_iter().map(|(j, _)| j).collect();
    kids.sort();
    assert_eq!(kids, vec!["c1".to_string(), "c2".to_string()]);
}

#[test]
fn per_session_cap_rejects_overflow() {
    let mgr = SubscriberManager::with_limits(reqwest::Client::new(), Duration::from_secs(60), 2);
    assert!(
        mgr.bind_job_route("j1", "sess", "http://b/mcp", "t", None)
            .is_ok()
    );
    assert!(
        mgr.bind_job_route("j2", "sess", "http://b/mcp", "t", None)
            .is_ok()
    );
    let err = mgr
        .bind_job_route("j3", "sess", "http://b/mcp", "t", None)
        .expect_err("cap should reject");
    matches!(err, BindJobError::TooManyInFlight { .. });
}

#[test]
fn terminal_status_auto_evicts_route() {
    let mgr = SubscriberManager::default();
    let backend = "http://127.0.0.1:0/mcp".to_string();
    let shared = Arc::new(BackendShared::new(backend.clone()));
    mgr.bind_job_route("jT", "sess", &backend, "t", None)
        .unwrap();
    assert!(mgr.job_route("jT").is_some());
    let note = serde_json::json!({
        "method": "notifications/$/dcc.jobUpdated",
        "params": {"job_id": "jT", "status": "completed"}
    });
    mgr.deliver(note, &shared);
    assert!(
        mgr.job_route("jT").is_none(),
        "route should be auto-evicted on completion"
    );
}

#[test]
fn run_route_gc_once_evicts_stale_routes() {
    // TTL=0 disables GC (per spec); use 1 ms so routes older than
    // 1 ms are stale.
    let mgr = SubscriberManager::with_limits(reqwest::Client::new(), Duration::from_millis(1), 0);
    mgr.bind_job_route("old", "s", "http://b/mcp", "t", None)
        .unwrap();
    // Force the created_at far into the past.
    if let Some(mut e) = mgr.inner.job_routes.get_mut("old") {
        e.value_mut().created_at = Utc::now() - chrono::Duration::seconds(10);
    }
    let evicted = mgr.run_route_gc_once();
    assert_eq!(evicted, 1);
    assert!(mgr.job_route("old").is_none());
}

#[test]
fn forget_job_cleans_up_reverse_indexes() {
    let mgr = SubscriberManager::default();
    mgr.bind_job_route("j1", "sess", "http://b/mcp", "t", None)
        .unwrap();
    mgr.bind_request_to_job("\"rid\"", "j1");
    assert!(mgr.job_route("j1").is_some());
    mgr.forget_job("j1");
    assert!(mgr.job_route("j1").is_none());
    assert!(mgr.job_id_for_request("\"rid\"").is_none());
    assert!(
        mgr.inner
            .session_jobs
            .get("sess")
            .is_none_or(|s| !s.contains("j1"))
    );
}
