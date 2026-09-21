//! Focused tests for the persisted feedback aggregation API.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::{RwLock, broadcast, watch};
use tower::ServiceExt;

use crate::gateway::admin::application::router::build_admin_router;
use crate::gateway::admin::state::AdminState;
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;

fn make_gateway_state(registry_dir: &std::path::Path) -> GatewayState {
    let registry = Arc::new(FileRegistry::new(registry_dir).unwrap());
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(8);
    GatewayState {
        ingress: Arc::new(crate::gateway::http_limits::GatewayIngressState::from_env()),
        resilience: Arc::new(Default::default()),
        registry,
        http_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::http_registration::HttpInstanceRegistry::default(),
        )),
        mdns_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::mdns_registration::MdnsInstanceRegistry::default(),
        )),
        relay_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::relay_registration::RelayInstanceRegistry::default(),
        )),
        stale_timeout: Duration::from_secs(30),
        backend_timeout: Duration::from_secs(10),
        async_dispatch_timeout: Duration::from_secs(60),
        wait_terminal_timeout: Duration::from_secs(600),
        server_name: "test-gateway".into(),
        server_version: "0.0.0-test".into(),
        own_host: "127.0.0.1".into(),
        own_port: 9765,
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        client_attribution: Arc::new(
            crate::gateway::caller_attribution::ClientAttributionStore::default(),
        ),
        pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools: false,
        policy: Arc::new(crate::gateway::GatewayPolicy::default()),
        adapter_version: None,
        adapter_dcc: None,
        capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        search_cache: Arc::new(crate::gateway::capability::search_cache::SearchCache::new(
            Default::default(),
        )),
        event_log: Arc::new(Default::default()),
        #[cfg(feature = "prometheus")]
        gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
        instance_diagnostics: Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        search_telemetry: Arc::new(crate::gateway::search_telemetry::SearchTelemetryStore::new()),
        debug_routes_enabled: false,
        auth: Arc::new(crate::gateway::security::GatewayAuth::disabled()),
        update_manifest_url: None,
        gateway_persist: false,
        gateway_idle_timeout_secs: 30,
        semantic_search_enabled: false,
        #[cfg(feature = "admin-persist-sqlite")]
        admin_sqlite_lane: None,
    }
}

fn admin_router(registry_dir: &std::path::Path) -> Router {
    build_admin_router(AdminState::new(make_gateway_state(registry_dir)))
}

async fn body_json(router: Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

fn feedback_dir(registry: &TempDir) -> std::path::PathBuf {
    let path = registry.path().join("feedback");
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[tokio::test]
async fn feedback_endpoint_aggregates_rotated_files_filters_and_limits_newest_first() {
    let registry = tempfile::tempdir().unwrap();
    let feedback = feedback_dir(&registry);
    let now = now_secs();
    std::fs::write(
        feedback.join("maya-101.jsonl.1"),
        format!(
            "{}\nnot-json\n",
            json!({
                "id": "maya-old",
                "timestamp": now - 60.0,
                "tool_name": "maya.mesh.inspect",
                "intent": "Inspect the mesh",
                "blocker": "No UV set",
                "severity": "blocked",
                "dcc_type": "maya"
            })
        ),
    )
    .unwrap();
    std::fs::write(
        feedback.join("maya-101.jsonl"),
        format!(
            "{}\n{}\n{}\n",
            json!({
                "id": "maya-new",
                "timestamp": now - 5.0,
                "tool_name": "maya.render.start",
                "intent": "Render a frame",
                "blocker": "Renderer unavailable",
                "severity": "blocked",
                "dcc_type": "maya"
            }),
            json!({
                "id": "maya-suggestion",
                "timestamp": now - 1.0,
                "tool_name": "maya.render.start",
                "intent": "Render a frame",
                "blocker": "Expose samples",
                "severity": "suggestion",
                "dcc_type": "maya"
            }),
            json!({
                "id": "maya-old",
                "timestamp": now - 60.0,
                "tool_name": "maya.mesh.inspect",
                "intent": "Inspect the mesh",
                "blocker": "No UV set",
                "severity": "blocked",
                "dcc_type": "maya"
            })
        ),
    )
    .unwrap();
    std::fs::write(
        feedback.join("blender-202.jsonl"),
        format!(
            "{}\n",
            json!({
                "id": "blender-new",
                "timestamp": now,
                "tool_name": "blender.render.start",
                "intent": "Render a frame",
                "blocker": "Renderer unavailable",
                "severity": "blocked",
                "dcc_type": "blender"
            })
        ),
    )
    .unwrap();

    let (status, body) = body_json(
        admin_router(registry.path()),
        "/api/feedback?dcc=maya&severity=blocked&range=24h&limit=1",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);
    assert_eq!(body["source"], "registry-jsonl");
    assert_eq!(body["total"], 2);
    assert_eq!(body["count"], 1);
    assert_eq!(body["truncated"], true);
    assert_eq!(body["skipped_invalid"], 1);
    assert_eq!(body["deduplicated"], 1);
    assert_eq!(body["files_scanned"], 3);
    assert_eq!(body["filters"]["dcc"], "maya");
    assert_eq!(body["filters"]["severity"], "blocked");
    assert_eq!(body["filters"]["range"], "24h");
    assert_eq!(body["filters"]["limit"], 1);
    assert_eq!(body["entries"][0]["id"], "maya-new");
}

#[tokio::test]
async fn feedback_endpoint_skips_oversized_lines_without_hiding_valid_records() {
    let registry = tempfile::tempdir().unwrap();
    let feedback = feedback_dir(&registry);
    let valid = json!({
        "id": "valid",
        "timestamp": now_secs(),
        "tool_name": "houdini.scene.inspect",
        "intent": "Inspect scene",
        "blocker": "No output node",
        "severity": "workaround_found"
    });
    let contents = format!(
        "{{\"oversized\":\"{}\"}}\n{valid}\n",
        "x".repeat(1024 * 1024)
    );
    std::fs::write(feedback.join("houdini-303.jsonl"), contents).unwrap();

    let (status, body) = body_json(
        admin_router(registry.path()),
        "/api/feedback?range=7d&dcc=houdini&limit=100",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 1);
    assert_eq!(body["skipped_invalid"], 1);
    assert_eq!(body["entries"][0]["id"], "valid");
    assert_eq!(body["entries"][0]["dcc_type"], "houdini");
}

#[tokio::test]
async fn feedback_endpoint_rejects_unbounded_or_unknown_queries() {
    let registry = tempfile::tempdir().unwrap();
    for uri in [
        "/api/feedback?range=30d",
        "/api/feedback?limit=1001",
        "/api/feedback?severity=critical",
    ] {
        let (status, body) = body_json(admin_router(registry.path()), uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "uri={uri} body={body}");
        assert_eq!(body["success"], false);
        assert_eq!(body["error"]["kind"], "invalid-feedback-query");
    }
}

/// Post two findings, drop the gateway (simulating a restart), then read both
/// back from a fresh admin state bound to the same SQLite file (#2253-E1).
#[cfg(feature = "admin-persist-sqlite")]
#[tokio::test]
async fn feedback_survives_a_gateway_restart_via_sqlite() {
    use crate::gateway::admin::sqlite_lane::AdminSqliteLane;

    let registry = tempfile::tempdir().unwrap();
    let db_path = registry.path().join("admin.sqlite");

    {
        let lane = AdminSqliteLane::spawn(db_path.clone(), 30).expect("spawn lane");
        let mut gateway = make_gateway_state(registry.path());
        gateway.admin_sqlite_lane = Some(lane);
        let app = crate::gateway::router::build_gateway_router(gateway);

        for intent in ["Render a frame", "Inspect the mesh"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/feedback")
                        .header(axum::http::header::CONTENT_TYPE, "application/json")
                        .body(axum::body::Body::from(
                            serde_json::to_vec(&json!({
                                "tool_name": "maya.render.start",
                                "intent": intent,
                                "blocker": "Renderer unavailable",
                                "severity": "blocked",
                                "dcc_type": "maya",
                                "instance_id": "aaaaaaaa-0000-0000-0000-000000000000",
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED, "intent={intent}");
        }
        // Dropping the router and the lane shuts the writer thread down and
        // flushes the queue — the same thing a process exit does.
    }

    let lane = AdminSqliteLane::spawn(db_path, 30).expect("respawn lane");
    let state =
        AdminState::new(make_gateway_state(registry.path())).with_admin_sqlite_lane(Some(lane));
    let (status, body) = body_json(
        build_admin_router(state),
        "/api/feedback?range=all&limit=100",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);
    assert_eq!(body["source"], "sqlite", "no JSONL mirror exists here");
    assert_eq!(body["total"], 2, "both reports survive the restart: {body}");
    assert_eq!(body["count"], 2);
    assert!(
        body["entries"][0]["timestamp"].as_f64() >= body["entries"][1]["timestamp"].as_f64(),
        "entries are ordered newest first: {body}"
    );
    let ids: Vec<&str> = body["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    assert_ne!(ids[0], ids[1], "each report keeps its own feedback_id");
}

/// The JSONL mirror stays readable when it holds rows SQLite does not (#2253-E1).
#[cfg(feature = "admin-persist-sqlite")]
#[tokio::test]
async fn feedback_reads_jsonl_mirror_when_sqlite_has_no_rows() {
    use crate::gateway::admin::sqlite_lane::AdminSqliteLane;

    let registry = tempfile::tempdir().unwrap();
    let feedback = feedback_dir(&registry);
    let now = now_secs();
    std::fs::write(
        feedback.join("maya-101.jsonl"),
        format!(
            "{}\n",
            json!({
                "id": "legacy-mirror-row",
                "timestamp": now - 5.0,
                "tool_name": "maya.render.start",
                "intent": "Render a frame",
                "blocker": "Renderer unavailable",
                "severity": "blocked",
                "dcc_type": "maya"
            })
        ),
    )
    .unwrap();

    let lane =
        AdminSqliteLane::spawn(registry.path().join("admin.sqlite"), 30).expect("spawn lane");
    let state =
        AdminState::new(make_gateway_state(registry.path())).with_admin_sqlite_lane(Some(lane));
    let (status, body) = body_json(
        build_admin_router(state),
        "/api/feedback?range=24h&limit=100",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["source"], "registry-jsonl", "old rows stay visible");
    assert_eq!(body["total"], 1);
    assert_eq!(body["entries"][0]["id"], "legacy-mirror-row");
}

/// Post one report, then write the client-side JSONL mirror line for it.
///
/// This is the shape a Python client produces today: the gateway persists the
/// row and the client mirrors it after receiving the 201, so the mirror carries
/// a later `timestamp`. SQLite has to win that collision — otherwise `source`
/// would claim the durable table served the response while every entry in it
/// actually came from the mirror (#2253-E1).
#[cfg(feature = "admin-persist-sqlite")]
#[tokio::test]
async fn sqlite_row_wins_when_the_jsonl_mirror_also_holds_the_report() {
    use crate::gateway::admin::sqlite_lane::AdminSqliteLane;

    let registry = tempfile::tempdir().unwrap();
    let db_path = registry.path().join("admin.sqlite");

    let feedback_id = {
        let lane = AdminSqliteLane::spawn(db_path.clone(), 30).expect("spawn lane");
        let mut gateway = make_gateway_state(registry.path());
        gateway.admin_sqlite_lane = Some(lane);
        let app = crate::gateway::router::build_gateway_router(gateway);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/feedback")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&json!({
                            "tool_name": "maya.render.start",
                            "intent": "Render a frame",
                            "blocker": "Renderer unavailable",
                            "severity": "blocked",
                            "dcc_type": "maya",
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let receipt: Value = serde_json::from_slice(&bytes).unwrap();
        // Dropping the router shuts the writer thread down and flushes the
        // queue, so the row is durable before the mirror line is written.
        receipt["feedback_id"].as_str().unwrap().to_string()
    };

    let feedback = feedback_dir(&registry);
    std::fs::write(
        feedback.join("maya-101.jsonl"),
        format!(
            "{}\n",
            json!({
                "id": feedback_id,
                // The mirror is stamped after the receipt round-trip, so it is
                // always newer than the row the gateway persisted.
                "timestamp": now_secs() + 5.0,
                "tool_name": "maya.render.start",
                "intent": "Render a frame",
                "blocker": "Renderer unavailable",
                "severity": "blocked",
                "dcc_type": "maya",
                "mirror_only_marker": true,
            })
        ),
    )
    .unwrap();

    let lane = AdminSqliteLane::spawn(db_path, 30).expect("respawn lane");
    let state =
        AdminState::new(make_gateway_state(registry.path())).with_admin_sqlite_lane(Some(lane));
    let (status, body) = body_json(
        build_admin_router(state),
        "/api/feedback?range=all&limit=100",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["source"], "sqlite",
        "the mirror duplicate is dropped, so only durable rows are returned: {body}"
    );
    assert_eq!(body["total"], 1, "the report is not double-counted: {body}");
    assert_eq!(body["deduplicated"], 1, "the mirror line is the duplicate");
    assert_eq!(body["entries"][0]["id"], feedback_id);
    assert_eq!(
        body["entries"][0]["mirror_only_marker"],
        Value::Null,
        "the returned entry is the SQLite row, not the mirror line: {body}"
    );
    assert!(
        body["entries"][0]["recorded_at"].is_string(),
        "the SQLite envelope survives: {body}"
    );
}

/// A legacy report that omits `dcc_type` is still findable on the SQLite path.
///
/// `FeedbackReport` skips the field when it is unset, so the legacy column
/// falls back to `gateway` while the stored record would carry no `dcc_type`
/// at all — which used to make the admin reader drop a row its own SQL had
/// just selected (#2253-E1).
#[cfg(feature = "admin-persist-sqlite")]
#[tokio::test]
async fn legacy_report_without_dcc_type_is_queryable_via_sqlite() {
    use crate::gateway::admin::sqlite_lane::AdminSqliteLane;

    let registry = tempfile::tempdir().unwrap();
    let db_path = registry.path().join("admin.sqlite");

    {
        let lane = AdminSqliteLane::spawn(db_path.clone(), 30).expect("spawn lane");
        let mut gateway = make_gateway_state(registry.path());
        gateway.admin_sqlite_lane = Some(lane);
        let app = crate::gateway::router::build_gateway_router(gateway);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/feedback")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&json!({
                            "tool_name": "gateway.registry__list",
                            "intent": "List the registry",
                            "blocker": "The registry file was locked",
                            "severity": "blocked",
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let lane = AdminSqliteLane::spawn(db_path, 30).expect("respawn lane");
    let state =
        AdminState::new(make_gateway_state(registry.path())).with_admin_sqlite_lane(Some(lane));
    let (status, body) = body_json(build_admin_router(state), "/api/feedback?dcc=gateway").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["source"], "sqlite", "no JSONL mirror exists here");
    assert_eq!(body["total"], 1, "?dcc=gateway finds the report: {body}");
    assert_eq!(body["entries"][0]["dcc_type"], "gateway");
    assert_eq!(body["entries"][0]["tool_name"], "gateway.registry__list");
}
