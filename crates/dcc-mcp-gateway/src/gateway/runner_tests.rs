//! Tests for the gateway runner — registration identity, challenger
//! election, and the live-snapshot merge applied on every heartbeat.
//!
//! Kept in a separate file so `runner.rs` stays within the file-size
//! budget (issue #842); `#[path]` keeps it a child module, which is what
//! lets it reach the runner's private helpers.

use super::*;
use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;

#[test]
fn registration_identity_rejects_external_endpoint_replacement() {
    let expected = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let mut replaced = expected.clone();
    replaced.port = 18813;
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();
    registry.register(expected.clone()).unwrap();
    let observed = registry.get(&expected.key()).unwrap();
    assert!(same_registration_identity(&registry, &expected, &observed));
    assert!(!same_registration_identity(&registry, &expected, &replaced));
    let mut wrong_sentinel = observed;
    wrong_sentinel.sentinel_path = Some(dir.path().join("locks/other.sentinel"));
    assert!(!same_registration_identity(
        &registry,
        &expected,
        &wrong_sentinel
    ));
}

#[test]
fn cooperative_yield_fallback_reads_structured_optional_capability() {
    let detail = cooperative_yield_fallback_detail(
        reqwest::StatusCode::CONFLICT,
        r#"{"error":{"kind":"optional-capability-unsupported","message":"poll instead"}}"#,
    );

    assert_eq!(
        detail.error_kind.as_deref(),
        Some("optional-capability-unsupported")
    );
    assert_eq!(detail.message, "poll instead");
    assert!(detail.optional_capability_miss);
}

#[test]
fn cooperative_yield_fallback_legacy_404_is_non_fatal() {
    let detail = cooperative_yield_fallback_detail(reqwest::StatusCode::NOT_FOUND, "");

    assert_eq!(detail.error_kind, None);
    assert!(detail.optional_capability_miss);
    assert!(
        detail.message.contains("optional capability miss"),
        "legacy detail should mark the fallback as optional: {}",
        detail.message
    );
}

#[test]
fn cooperative_yield_probe_skips_known_same_or_newer_gateway() {
    assert!(should_probe_cooperative_yield("0.17.8", ""));
    assert!(should_probe_cooperative_yield("0.17.9", "0.17.8"));
    assert!(!should_probe_cooperative_yield("0.17.8", "0.17.8"));
    assert!(!should_probe_cooperative_yield("0.17.7", "0.17.8"));
}

#[test]
fn healthy_resident_suppresses_version_preemption() {
    assert_eq!(challenger_reason(ResidentGatewayHealth::Healthy), None);
}

#[test]
fn readyz_requires_explicit_ok_true() {
    assert!(readyz_body_is_healthy(br#"{"ok":true}"#));
    assert!(!readyz_body_is_healthy(br#"{"ok":false}"#));
    assert!(!readyz_body_is_healthy(br#"{}"#));
    assert!(!readyz_body_is_healthy(b"not-json"));
    assert!(!readyz_body_is_healthy(b""));
}

#[tokio::test]
async fn hanging_resident_readyz_is_bounded_and_unhealthy() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let holder = tokio::spawn(async move {
        if let Ok((_stream, _peer)) = listener.accept().await {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    let started = std::time::Instant::now();
    let observed =
        probe_resident_gateway_health("127.0.0.1", port, Duration::from_millis(100)).await;
    assert_eq!(observed, ResidentGatewayHealth::Unhealthy);
    assert!(started.elapsed() < Duration::from_secs(1));
    holder.abort();
}

#[tokio::test]
async fn legacy_health_fallback_requires_http_200() {
    let app = Router::new()
        .route("/v1/readyz", get(|| async { StatusCode::NOT_FOUND }))
        .route("/health", get(|| async { StatusCode::NO_CONTENT }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let observed =
        probe_resident_gateway_health("127.0.0.1", port, Duration::from_millis(500)).await;
    assert_eq!(observed, ResidentGatewayHealth::Unhealthy);
    server.abort();
}

#[tokio::test]
async fn legacy_health_fallback_accepts_http_200() {
    let app = Router::new()
        .route("/v1/readyz", get(|| async { StatusCode::NOT_FOUND }))
        .route("/health", get(|| async { StatusCode::OK }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let observed =
        probe_resident_gateway_health("127.0.0.1", port, Duration::from_millis(500)).await;
    assert_eq!(observed, ResidentGatewayHealth::Healthy);
    server.abort();
}

#[test]
fn unhealthy_resident_enters_challenger_mode_even_without_version_advantage() {
    assert_eq!(
        challenger_reason(ResidentGatewayHealth::Unhealthy),
        Some("Resident gateway failed application readiness probe — entering challenger mode")
    );
}

#[test]
fn missing_resident_still_recovers_time_wait_race() {
    // No sentinel: /health fails (connection refused) → Unhealthy → challenger
    assert_eq!(
        challenger_reason(ResidentGatewayHealth::Unhealthy),
        Some("Resident gateway failed application readiness probe — entering challenger mode")
    );
}

#[tokio::test]
async fn challenger_waits_for_startup_readback_before_registry_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();
    let runner = GatewayRunner::new(GatewayConfig {
        host: "127.0.0.1".to_string(),
        gateway_port: port,
        registry_dir: Some(dir.path().to_path_buf()),
        challenger_poll_interval_secs: 60,
        challenger_timeout_secs: 120,
        ..GatewayConfig::default()
    })
    .unwrap();
    let (ready_tx, ready_rx) = tokio::sync::watch::channel(false);
    let challenger = runner.spawn_challenger_loop("2.0.0", "1.0.0", Some(ready_rx));

    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        runner
            .registry
            .list_instances_async(GATEWAY_SENTINEL_DCC_TYPE.to_string())
            .await
            .unwrap()
            .is_empty(),
        "challenger must not publish a sentinel before startup readback"
    );

    ready_tx.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        while runner
            .registry
            .list_instances_async(GATEWAY_SENTINEL_DCC_TYPE.to_string())
            .await
            .unwrap()
            .is_empty()
        {
            // Avoid monopolizing the current-thread runtime or registry
            // storage lock when nextest runs the workspace under load.
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("challenger sentinel should appear after startup readback");
    challenger.abort();
    drop(occupied);
}

// Issue #2500 — the heartbeat path a Python adapter drives through
// `McpServerHandle.update_gateway_extras` must merge JSON-typed extras
// into the FileRegistry row and clear keys on `Value::Null`.
#[test]
fn apply_live_snapshot_merges_typed_extras_and_clears_on_null() {
    let mut entry = ServiceEntry::new("auroraview", "127.0.0.1", 18812);
    entry
        .extras
        .insert("host_dcc".to_string(), serde_json::json!("maya-2024"));
    entry
        .extras
        .insert("remove_me".to_string(), serde_json::json!(1));

    let mut extras = std::collections::HashMap::new();
    extras.insert("cdp_port".to_string(), serde_json::json!(9222));
    extras.insert("enabled".to_string(), serde_json::json!(true));
    extras.insert("remove_me".to_string(), serde_json::Value::Null);

    apply_live_snapshot(
        &mut entry,
        &LiveSnapshot {
            extras,
            ..LiveSnapshot::default()
        },
    );

    assert_eq!(entry.extras.get("cdp_port"), Some(&serde_json::json!(9222)));
    assert_eq!(entry.extras.get("enabled"), Some(&serde_json::json!(true)));
    assert_eq!(
        entry.extras.get("host_dcc"),
        Some(&serde_json::json!("maya-2024")),
        "unrelated extras must survive the merge"
    );
    assert!(!entry.extras.contains_key("remove_me"));
}

#[test]
fn healthy_gateway_without_registry_sentinel_coexists() {
    // PIP-2509: standalone gateway daemon holds the port but may not write
    // a __gateway__ row into this adapter's FileRegistry. A successful
    // /health probe must run as a plain DCC instance, not challenger mode.
    assert_eq!(challenger_reason(ResidentGatewayHealth::Healthy), None);
}
