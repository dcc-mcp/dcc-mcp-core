use super::*;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceStatus};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, watch};

fn test_gateway_state(reg: Arc<RwLock<FileRegistry>>) -> GatewayState {
    test_gateway_state_with_own(reg, "127.0.0.1", 9765)
}

fn test_gateway_state_with_own(
    reg: Arc<RwLock<FileRegistry>>,
    own_host: &str,
    own_port: u16,
) -> GatewayState {
    test_gateway_state_with_own_and_unknown(reg, own_host, own_port, false)
}

fn test_gateway_state_with_own_and_unknown(
    reg: Arc<RwLock<FileRegistry>>,
    own_host: &str,
    own_port: u16,
    allow_unknown_tools: bool,
) -> GatewayState {
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(8);
    GatewayState {
        registry: reg,
        http_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::http_registration::HttpInstanceRegistry::default(),
        )),

        mdns_instance_registry: Arc::new(parking_lot::RwLock::new(
            crate::gateway::mdns_registration::MdnsInstanceRegistry::default(),
        )),
        stale_timeout: Duration::from_secs(30),
        backend_timeout: Duration::from_secs(10),
        async_dispatch_timeout: Duration::from_secs(60),
        wait_terminal_timeout: Duration::from_secs(600),
        server_name: "test".into(),
        server_version: env!("CARGO_PKG_VERSION").into(),
        own_host: own_host.to_string(),
        own_port,
        http_client: reqwest::Client::new(),
        yield_tx: Arc::new(yield_tx),
        events_tx: Arc::new(events_tx),
        protocol_version: Arc::new(RwLock::new(None)),
        resource_subscriptions: Arc::new(RwLock::new(HashMap::new())),
        client_attribution: Arc::new(
            crate::gateway::caller_attribution::ClientAttributionStore::default(),
        ),
        pending_calls: Arc::new(RwLock::new(HashMap::new())),
        subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
        allow_unknown_tools,
        policy: Arc::new(crate::gateway::GatewayPolicy::default()),
        adapter_version: None,
        adapter_dcc: None,
        capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
        event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
        #[cfg(feature = "prometheus")]
        gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        middleware_chain: Arc::new(MiddlewareChain::new()),
        instance_diagnostics: Arc::new(
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
        ),
        traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
        search_telemetry: Arc::new(crate::gateway::search_telemetry::SearchTelemetryStore::new()),
        debug_routes_enabled: false,
    }
}

// Regression test for the sibling of issue #230: the `__gateway__` sentinel
// must never appear in user-facing DCC instance listings (e.g.
// `list_dcc_instances`, `get_dcc_instance`, `connect_to_dcc`). Exposing
// it would invite agents to `connect_to_dcc("__gateway__")` and loop
// requests back through the gateway facade.
#[tokio::test]
async fn test_live_instances_excludes_gateway_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
        sentinel.version = Some(env!("CARGO_PKG_VERSION").into());
        r.register(sentinel).unwrap();

        let maya = ServiceEntry::new("maya", "127.0.0.1", 18812);
        r.register(maya).unwrap();
    }

    let gs = test_gateway_state(registry.clone());
    let live = gs.live_instances(&*registry.read().await);
    assert_eq!(live.len(), 1, "only the maya row should be returned");
    assert_eq!(live[0].dcc_type, "maya");
    assert!(
        !live.iter().any(|e| e.dcc_type == GATEWAY_SENTINEL_DCC_TYPE),
        "gateway sentinel must never appear in user-facing listings"
    );
}

#[tokio::test]
async fn live_instances_includes_http_registered_rows() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let gs = test_gateway_state(registry.clone());
    let instance_id = uuid::Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap();
    {
        let mut http_registry = gs.http_instance_registry.write();
        http_registry
            .register(
                crate::gateway::http_registration::HttpInstanceRegistrationRequest {
                    instance_id: instance_id.to_string(),
                    dcc_type: "photoshop".to_string(),
                    mcp_url: "http://remote.example:28765/mcp".to_string(),
                    capabilities_fingerprint: None,
                    adapter_version: Some("2.0.0".to_string()),
                    scene: Some("comp.psd".to_string()),
                    ttl_secs: None,
                },
                std::time::SystemTime::now(),
            )
            .unwrap();
    }

    let registry = registry.read().await;
    let live = gs.live_instances(&registry);
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].instance_id, instance_id);
    let row = gs.instance_json(&live[0]);
    assert_eq!(row["source"], "http");
    assert_eq!(row["mcp_url"], "http://remote.example:28765/mcp");
}

#[tokio::test]
async fn live_instances_includes_mdns_rows_and_http_wins_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let gs = test_gateway_state(registry.clone());
    let instance_id = uuid::Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap();
    let now = std::time::SystemTime::now();

    let mut mdns_entry = ServiceEntry::new("maya", "192.168.1.20", 8765);
    mdns_entry.instance_id = instance_id;
    mdns_entry.metadata.insert(
        crate::gateway::http_registration::REGISTRY_SOURCE_METADATA_KEY.to_string(),
        crate::gateway::http_registration::SOURCE_MDNS.to_string(),
    );
    mdns_entry.metadata.insert(
        crate::gateway::http_registration::MCP_URL_METADATA_KEY.to_string(),
        "http://192.168.1.20:8765/mcp".to_string(),
    );
    gs.mdns_instance_registry.write().upsert(
        mdns_entry,
        "maya._dcc-mcp._tcp.local.".to_string(),
        Duration::from_secs(30),
        now,
    );

    {
        let mut http_registry = gs.http_instance_registry.write();
        http_registry
            .register(
                crate::gateway::http_registration::HttpInstanceRegistrationRequest {
                    instance_id: instance_id.to_string(),
                    dcc_type: "maya".to_string(),
                    mcp_url: "http://remote.example:28765/mcp".to_string(),
                    capabilities_fingerprint: None,
                    adapter_version: Some("2.0.0".to_string()),
                    scene: Some("scene.ma".to_string()),
                    ttl_secs: None,
                },
                now,
            )
            .unwrap();
    }

    let registry = registry.read().await;
    let live = gs.live_instances(&registry);
    assert_eq!(live.len(), 1);
    let row = gs.instance_json(&live[0]);
    assert_eq!(row["source"], "http");
    assert_eq!(row["mcp_url"], "http://remote.example:28765/mcp");
}

/// Regression test for issue #419: when the gateway process is also a
/// DCC instance (e.g. Maya that won the gateway election), its own
/// plain-instance row must be hidden from `live_instances` so the
/// facade does not fan `tools/list` / `tools/call` back into itself.
#[tokio::test]
async fn test_live_instances_excludes_gateway_self_row() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        // The sentinel + the gateway's own DCC row share host/port.
        let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
        sentinel.version = Some(env!("CARGO_PKG_VERSION").into());
        r.register(sentinel).unwrap();

        // Self DCC row — same host/port as the gateway facade.
        let maya_self = ServiceEntry::new("maya", "127.0.0.1", 9765);
        r.register(maya_self).unwrap();

        // A second Maya instance on a different port — must survive.
        let maya_other = ServiceEntry::new("maya", "127.0.0.1", 18812);
        r.register(maya_other).unwrap();
    }

    let gs = test_gateway_state_with_own(registry.clone(), "127.0.0.1", 9765);
    let live = gs.live_instances(&*registry.read().await);
    assert_eq!(
        live.len(),
        1,
        "only the non-self maya row should remain; got {live:#?}"
    );
    assert_eq!(live[0].port, 18812);
}

/// Regression test for issue #419: `localhost` / `::1` / `0.0.0.0` must
/// all normalise to the same address so that a gateway bound on
/// `127.0.0.1` still filters out a self-row advertised as `localhost`
/// (DCC adapters vary in how they populate `ServiceEntry::host`).
#[tokio::test]
async fn test_live_instances_self_row_localhost_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        // Self row advertised as "localhost" — must still be filtered
        // when the gateway is bound to 127.0.0.1.
        let maya_self = ServiceEntry::new("maya", "localhost", 9765);
        r.register(maya_self).unwrap();
    }

    let gs = test_gateway_state_with_own(registry.clone(), "127.0.0.1", 9765);
    let live = gs.live_instances(&*registry.read().await);
    assert!(
        live.is_empty(),
        "self row with localhost alias must be filtered; got {live:#?}"
    );
}

/// Issue #940: `ServiceStatus::Stale` is an immediate routing veto even
/// before the heartbeat crosses `stale_timeout`.
#[tokio::test]
async fn test_live_instances_excludes_status_stale_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        let mut stale = ServiceEntry::new("maya", "127.0.0.1", 18812);
        stale.status = ServiceStatus::Stale;
        r.register(stale).unwrap();

        let photoshop = ServiceEntry::new("photoshop", "127.0.0.1", 18813);
        r.register(photoshop).unwrap();
    }

    let gs = test_gateway_state(registry.clone());
    let live = gs.live_instances(&*registry.read().await);
    assert_eq!(live.len(), 1, "only the non-stale row should remain");
    assert_eq!(live[0].dcc_type, "photoshop");
}

#[tokio::test]
async fn test_live_instances_prefers_sidecar_for_same_dcc_pid() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        let mut in_process = ServiceEntry::new("maya", "127.0.0.1", 18812).with_pid(4242);
        in_process.version = Some("2026".into());
        r.register(in_process).unwrap();

        let mut sidecar = ServiceEntry::new("maya", "127.0.0.1", 28812).with_pid(4242);
        sidecar.adapter_version = Some("0.3.3".into());
        sidecar
            .metadata
            .insert("dcc_mcp_role".into(), "per-dcc-sidecar".into());
        r.register(sidecar).unwrap();
    }

    let gs = test_gateway_state(registry.clone());
    let live = gs.live_instances(&*registry.read().await);

    assert_eq!(live.len(), 1, "sidecar should replace same-pid adapter row");
    assert_eq!(live[0].port, 28812);
    assert_eq!(
        live[0].metadata.get("dcc_mcp_role").map(String::as_str),
        Some("per-dcc-sidecar")
    );
}

#[test]
fn test_entry_json_exposes_lifecycle_metadata_for_admin() {
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 28812).with_pid(4242);
    entry
        .metadata
        .insert("dcc_mcp_role".into(), "per-dcc-sidecar".into());
    entry.metadata.insert("sidecar_pid".into(), "31337".into());
    entry.metadata.insert(
        "restart_command".into(),
        "rez-env dcc_mcp_maya -- maya-sidecar".into(),
    );
    entry.metadata.insert(
        "install_root".into(),
        "G:\\_thm\\rez_local_cache\\ext\\dcc_mcp_maya".into(),
    );
    entry
        .metadata
        .insert("owner".into(), "release-smoke-test".into());
    entry.metadata.insert("session".into(), "test".into());
    entry.metadata.insert(
        "safe_stop_url".into(),
        "http://127.0.0.1:19000/safe-stop".into(),
    );

    let row = entry_to_json(&entry, Duration::from_secs(30), None);

    assert_eq!(row["lifecycle"]["role"], "per-dcc-sidecar");
    assert_eq!(row["lifecycle"]["owner"], "release-smoke-test");
    assert_eq!(row["lifecycle"]["session"], "test");
    assert_eq!(row["lifecycle"]["sidecar_pid"], 31337);
    assert_eq!(row["lifecycle"]["supports_safe_stop"], true);
    assert_eq!(
        row["lifecycle"]["safe_stop_url"],
        "http://127.0.0.1:19000/safe-stop"
    );
    assert_eq!(row["lifecycle"]["restartable"], true);
    assert_eq!(
        row["lifecycle"]["restart_command"],
        "rez-env dcc_mcp_maya -- maya-sidecar"
    );
}

#[tokio::test]
async fn test_instance_json_reports_app_ui_availability_from_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let mut entry = ServiceEntry::new("python", "127.0.0.1", 18812);
    entry.instance_id = uuid::Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
    {
        let r = registry.read().await;
        r.register(entry.clone()).unwrap();
    }

    let gs = test_gateway_state(registry.clone());
    let record = crate::gateway::capability::CapabilityRecord::new(
        crate::gateway::capability::tool_slug("python", &entry.instance_id, "app_ui__snapshot"),
        "app_ui__snapshot".into(),
        "app_ui__snapshot".into(),
        Some("app-ui".into()),
        "Capture app UI snapshot",
        vec!["app-ui".into()],
        "python".into(),
        entry.instance_id,
        true,
        true,
    );
    gs.capability_index.upsert_instance(
        entry.instance_id,
        vec![record],
        crate::gateway::capability::InstanceFingerprint(1),
    );

    let row = gs.instance_json(&entry);

    assert_eq!(row["diagnostics"]["app_ui"]["status"], "available");
    assert_eq!(row["diagnostics"]["app_ui"]["tools"][0], "app_ui__snapshot");
}

#[tokio::test]
async fn test_instance_json_reports_app_ui_disabled_by_policy() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let mut entry = ServiceEntry::new("photoshop", "127.0.0.1", 18812);
    entry
        .metadata
        .insert("app_ui.status".into(), "disabled".into());
    entry.metadata.insert(
        "app_ui.reason".into(),
        "adapter policy denied UI automation".into(),
    );
    {
        let r = registry.read().await;
        r.register(entry.clone()).unwrap();
    }

    let gs = test_gateway_state(registry.clone());
    let row = gs.instance_json(&entry);

    assert_eq!(row["diagnostics"]["app_ui"]["status"], "disabled_by_policy");
    assert_eq!(
        row["diagnostics"]["app_ui"]["reason"],
        "adapter policy denied UI automation"
    );
}

#[tokio::test]
async fn test_live_instances_stale_sidecar_does_not_hide_live_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        let in_process = ServiceEntry::new("maya", "127.0.0.1", 18812).with_pid(4242);
        r.register(in_process).unwrap();

        let mut sidecar = ServiceEntry::new("maya", "127.0.0.1", 28812).with_pid(4242);
        sidecar.last_heartbeat = std::time::SystemTime::now() - Duration::from_secs(600);
        sidecar
            .metadata
            .insert("dcc_mcp_role".into(), "per-dcc-sidecar".into());
        r.register(sidecar).unwrap();
    }

    let gs = test_gateway_state(registry.clone());
    let live = gs.live_instances(&*registry.read().await);

    assert_eq!(
        live.len(),
        1,
        "stale sidecar must not mask a live in-process adapter"
    );
    assert_eq!(live[0].port, 18812);
    assert!(!live[0].metadata.contains_key("dcc_mcp_role"));
}

#[tokio::test]
async fn test_resolve_instance_accepts_short_and_unique_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let instance_id = uuid::Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();

    {
        let r = registry.read().await;
        let mut maya = ServiceEntry::new("maya", "127.0.0.1", 18812);
        maya.instance_id = instance_id;
        r.register(maya).unwrap();
    }

    let gs = test_gateway_state(registry.clone());
    let reg = registry.read().await;
    assert_eq!(
        gs.resolve_instance(&reg, Some("abcdef01"), None)
            .unwrap()
            .instance_id,
        instance_id
    );
    assert_eq!(
        gs.resolve_instance(&reg, Some("abcd"), Some("maya"))
            .unwrap()
            .instance_id,
        instance_id
    );
}

#[tokio::test]
async fn test_resolve_instance_rejects_short_and_ambiguous_prefixes() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        let mut a = ServiceEntry::new("maya", "127.0.0.1", 18812);
        a.instance_id = uuid::Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        r.register(a).unwrap();
        let mut b = ServiceEntry::new("maya", "127.0.0.1", 18813);
        b.instance_id = uuid::Uuid::parse_str("abcdef9923456789abcdef0123456789").unwrap();
        r.register(b).unwrap();
    }

    let gs = test_gateway_state(registry.clone());
    let reg = registry.read().await;
    let too_short = gs.resolve_instance(&reg, Some("ab"), None).unwrap_err();
    assert!(matches!(
        too_short,
        ResolveInstanceError::PrefixTooShort { .. }
    ));
    let ambiguous = gs.resolve_instance(&reg, Some("abcdef"), None).unwrap_err();
    assert!(matches!(
        ambiguous,
        ResolveInstanceError::MultipleMatches { .. }
    ));
}

/// Issue #555: instances with `dcc_type == "unknown"` must be hidden from
/// `live_instances` when `allow_unknown_tools` is `false` (the default).
#[tokio::test]
async fn test_live_instances_hides_unknown_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        let unknown = ServiceEntry::new("unknown", "127.0.0.1", 18812);
        r.register(unknown).unwrap();

        let maya = ServiceEntry::new("maya", "127.0.0.1", 18813);
        r.register(maya).unwrap();
    }

    let gs = test_gateway_state_with_own_and_unknown(registry.clone(), "127.0.0.1", 9765, false);
    let live = gs.live_instances(&*registry.read().await);
    assert_eq!(live.len(), 1, "only the maya row should be returned");
    assert_eq!(live[0].dcc_type, "maya");
    assert!(
        !live
            .iter()
            .any(|e| e.dcc_type.eq_ignore_ascii_case("unknown")),
        "unknown dcc_type must be filtered when allow_unknown_tools is false"
    );
}

/// Issue #555: when `allow_unknown_tools` is `true`, unknown instances
/// survive the filter.
#[tokio::test]
async fn test_live_instances_shows_unknown_when_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        let unknown = ServiceEntry::new("unknown", "127.0.0.1", 18812);
        r.register(unknown).unwrap();

        let maya = ServiceEntry::new("maya", "127.0.0.1", 18813);
        r.register(maya).unwrap();
    }

    let gs = test_gateway_state_with_own_and_unknown(registry.clone(), "127.0.0.1", 9765, true);
    let live = gs.live_instances(&*registry.read().await);
    assert_eq!(live.len(), 2, "both rows should be returned when allowed");
    assert!(
        live.iter()
            .any(|e| e.dcc_type.eq_ignore_ascii_case("unknown")),
        "unknown dcc_type must be present when allow_unknown_tools is true"
    );
}

/// Issue maya#138: `all_instances` keeps stale and `unknown` rows
/// (dropping only the gateway sentinel and the gateway's own
/// self-row) so the operator-facing `list_dcc_instances` tool can
/// surface a complete picture of the registry directory.
#[tokio::test]
async fn test_all_instances_keeps_stale_and_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        // The bookkeeping sentinel — must always be filtered.
        let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
        sentinel.version = Some("0.14.18".into());
        r.register(sentinel).unwrap();

        // The standalone server's "unknown" row — kept by all_instances
        // so operators can see why connect_to_dcc cannot route to it.
        let unknown = ServiceEntry::new("unknown", "127.0.0.1", 18900);
        r.register(unknown).unwrap();

        // A live Maya plugin.
        let maya = ServiceEntry::new("maya", "127.0.0.1", 18812);
        r.register(maya).unwrap();

        // A stale Maya plugin (heartbeat 10 minutes ago).
        let mut stale = ServiceEntry::new("maya", "127.0.0.1", 18813);
        stale.last_heartbeat = std::time::SystemTime::now() - Duration::from_secs(600);
        r.register(stale).unwrap();
    }

    let gs = test_gateway_state_with_own_and_unknown(registry.clone(), "127.0.0.1", 9765, false);
    let all = gs.all_instances(&*registry.read().await);

    assert_eq!(
        all.len(),
        3,
        "expected unknown + live maya + stale maya, got {all:?}"
    );
    assert!(
        !all.iter().any(|e| e.dcc_type == GATEWAY_SENTINEL_DCC_TYPE),
        "gateway sentinel must always be filtered from operator output"
    );
    assert!(
        all.iter().any(|e| e.dcc_type == "unknown"),
        "unknown row must be retained even when allow_unknown_tools is false"
    );
    assert!(
        all.iter().any(|e| e.is_stale(gs.stale_timeout)),
        "stale row must be retained for diagnostics"
    );
}

/// Issue maya#138: `entry_to_json` reports `status: "stale"` once a
/// row has aged past `stale_timeout`, regardless of the original
/// `ServiceStatus`, so callers can branch without a separate field.
#[test]
fn test_entry_to_json_status_stale_for_aged_row() {
    let mut e = ServiceEntry::new("maya", "127.0.0.1", 18812);
    e.last_heartbeat = std::time::SystemTime::now() - Duration::from_secs(600);
    let json = entry_to_json(&e, Duration::from_secs(30), None);
    assert_eq!(json["status"].as_str(), Some("stale"));
    assert_eq!(json["stale"].as_bool(), Some(true));
}

#[test]
fn test_entry_to_json_status_stale_for_marked_row() {
    let mut e = ServiceEntry::new("maya", "127.0.0.1", 18812);
    e.status = ServiceStatus::Stale;
    let json = entry_to_json(&e, Duration::from_secs(30), None);
    assert_eq!(json["status"].as_str(), Some("stale"));
    assert_eq!(json["stale"].as_bool(), Some(true));
    assert_eq!(json["pool"]["available"].as_bool(), Some(false));
}

#[test]
fn test_entry_to_json_includes_pool_state() {
    let mut e = ServiceEntry::new("maya", "127.0.0.1", 18812).with_capacity(2);
    e.acquire_lease(
        "workflow-1",
        Some("job-1".to_string()),
        Some(std::time::SystemTime::now() + Duration::from_secs(60)),
    );

    let json = entry_to_json(&e, Duration::from_secs(30), None);

    assert_eq!(json["status"].as_str(), Some("busy"));
    assert_eq!(json["pool"]["capacity"].as_u64(), Some(2));
    assert_eq!(json["pool"]["lease_owner"].as_str(), Some("workflow-1"));
    assert_eq!(json["pool"]["current_job_id"].as_str(), Some("job-1"));
    assert_eq!(json["pool"]["available"].as_bool(), Some(false));
    assert!(json["pool"]["lease_expires_at"].as_u64().is_some());
}

// ── host_execution summary (issue #1331) ──────────────────────────────

#[test]
fn test_entry_to_json_host_execution_unknown_without_diagnostics() {
    let e = ServiceEntry::new("maya", "127.0.0.1", 18813);
    let json = entry_to_json(&e, Duration::from_secs(30), None);
    assert_eq!(json["host_execution"]["status"].as_str(), Some("unknown"));
    assert_eq!(
        json["host_execution"]["missing_bits"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
}

#[test]
fn test_entry_to_json_host_execution_ready_when_probe_green() {
    use crate::gateway::instance_diagnostics::InstanceDiagnostics;
    use dcc_mcp_skill_rest::ReadinessReport;

    let e = ServiceEntry::new("maya", "127.0.0.1", 18814);
    let diag = InstanceDiagnostics {
        readiness: Some(ReadinessReport {
            process: true,
            dcc: true,
            skill_catalog: true,
            dispatcher: true,
            host_execution_bridge: true,
            main_thread_executor: true,
        }),
        ..Default::default()
    };
    let json = entry_to_json(&e, Duration::from_secs(30), Some(&diag));
    assert_eq!(json["host_execution"]["status"].as_str(), Some("ready"));
}

#[test]
fn test_entry_to_json_host_execution_not_ready_lists_missing_bits() {
    use crate::gateway::instance_diagnostics::InstanceDiagnostics;
    use dcc_mcp_skill_rest::ReadinessReport;

    let e = ServiceEntry::new("maya", "127.0.0.1", 18815);
    let diag = InstanceDiagnostics {
        readiness: Some(ReadinessReport {
            process: true,
            dcc: false,
            skill_catalog: true,
            dispatcher: true,
            host_execution_bridge: false,
            main_thread_executor: true,
        }),
        ..Default::default()
    };
    let json = entry_to_json(&e, Duration::from_secs(30), Some(&diag));
    assert_eq!(json["host_execution"]["status"].as_str(), Some("not_ready"));
    let missing: Vec<&str> = json["host_execution"]["missing_bits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(missing, vec!["dcc", "host_execution_bridge"]);
}

// ── display_id (RFC #998 Addendum B) ───────────────────────────────────

/// `gateway://instances` carries the derived `{dcc}@{version}-{short8}`
/// label so MCP clients can render a single human-readable
/// disambiguation hint instead of stitching three fields together.
/// The full `instance_id` UUID stays the canonical machine handle and
/// is unaffected.
#[test]
fn test_entry_to_json_includes_display_id_with_version() {
    let mut e = ServiceEntry::new("maya", "127.0.0.1", 18812);
    e.version = Some("2026".to_string());
    let json = entry_to_json(&e, Duration::from_secs(30), None);

    let display = json["display_id"]
        .as_str()
        .expect("display_id must be present in entry_to_json output");
    assert!(
        display.starts_with("maya@2026-"),
        "display_id should start with dcc@version-, got {display}"
    );
    // The full UUID is still surfaced verbatim for machine paths.
    assert_eq!(
        json["instance_id"].as_str().unwrap(),
        e.instance_id.to_string()
    );
}

#[test]
fn test_entry_to_json_display_id_falls_back_to_unknown_version() {
    let mut e = ServiceEntry::new("figma", "127.0.0.1", 8765);
    e.version = None;
    let json = entry_to_json(&e, Duration::from_secs(30), None);

    let display = json["display_id"]
        .as_str()
        .expect("display_id must be present even when version is None");
    assert!(
        display.starts_with("figma@unknown-"),
        "display_id should fall back to 'unknown' when version is None, got {display}"
    );
}

// ── Issue #719: read_alive_instances ───────────────────────────────────

/// A row whose PID points at a live process survives the prune; a row
/// whose owning process has exited (simulated by dropping a separate
/// `FileRegistry` handle) is evicted — even if its heartbeat was
/// freshly written. Dead rows also disappear from the on-disk
/// `services.json`, not just from the returned slice.
#[tokio::test]
async fn test_read_alive_instances_prunes_dead_pid() {
    let dir = tempfile::tempdir().unwrap();

    // Reader handle represents the gateway process — keeps the
    // `live` row's sentinel lock held for the duration of the test.
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    let live_id;
    {
        let r = registry.read().await;
        let mut live = ServiceEntry::new("maya", "127.0.0.1", 18812);
        live.pid = Some(std::process::id());
        live_id = live.instance_id;
        r.register(live).unwrap();
    }

    // Separate "writer" handle simulates a crashed DCC process: it
    // registers a row, then its `FileRegistry` is dropped which
    // releases the sentinel lock and leaves a ghost row on disk
    // for the reader handle to find.
    let dead_id = {
        let writer = FileRegistry::new(dir.path()).unwrap();
        let mut dead = ServiceEntry::new("blender", "127.0.0.1", 18813);
        dead.pid = Some(u32::MAX - 1);
        let dead_id = dead.instance_id;
        writer.register(dead).unwrap();
        dead_id
        // `writer` dropped here → its sentinel lock is released.
    };

    // On Windows the filesystem mtime granularity can be ~100ms or coarser.
    // If the reader's flush and the writer's flush land within the same
    // mtime quantum, `reload_if_stale()` will skip the reload because it
    // sees the same mtime it cached. A short sleep ensures the next
    // `read_alive` triggers a fresh reload.
    #[cfg(windows)]
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let gs = test_gateway_state(registry.clone());
    let (alive, evicted) = gs
        .read_alive_instances(&*registry.read().await)
        .expect("read_alive_instances must succeed");

    assert_eq!(evicted, 1, "exactly one dead row must be evicted");
    assert_eq!(alive.len(), 1, "only the live row survives");
    assert_eq!(alive[0].instance_id, live_id);
    assert_ne!(alive[0].instance_id, dead_id);

    // The dead row must also be gone from services.json — not just
    // filtered out of the returned slice.
    let raw = gs.all_instances(&*registry.read().await);
    assert!(
        raw.iter().all(|e| e.instance_id != dead_id),
        "dead row must be purged from the on-disk registry after read_alive_instances",
    );
}

/// Fail-open guard (#227): a row without a `pid` is assumed alive and
/// must survive the prune — older registrations predate the pid field.
#[tokio::test]
async fn test_read_alive_instances_keeps_rows_without_pid() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        // `ServiceEntry::new` defaults pid to the current process; null
        // it out to simulate a legacy registration that predates the
        // pid field.
        let mut legacy = ServiceEntry::new("photoshop", "127.0.0.1", 18814);
        legacy.pid = None;
        r.register(legacy).unwrap();
    }

    let gs = test_gateway_state(registry.clone());
    let (alive, evicted) = gs
        .read_alive_instances(&*registry.read().await)
        .expect("read_alive_instances must succeed");

    assert_eq!(evicted, 0);
    assert_eq!(alive.len(), 1, "pid-less rows must survive (#227 contract)");
    assert_eq!(alive[0].dcc_type, "photoshop");
    assert!(
        alive[0].pid.is_none(),
        "pid must remain null after read_alive"
    );
}

/// Regression guard for maya#138 and #419: the PID-pruned path must
/// still filter out the bookkeeping `__gateway__` sentinel and the
/// gateway's own self-row. Otherwise a gateway that crashed and
/// re-bound would briefly expose its own sentinel to agents.
#[tokio::test]
async fn test_read_alive_instances_filters_sentinel_and_self() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;

        // Sentinel row — carries the current pid (looks alive) but
        // must still be excluded.
        let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
        sentinel.pid = Some(std::process::id());
        r.register(sentinel).unwrap();

        // Gateway's own plain-instance row (same host/port as the
        // facade under test).
        let mut self_row = ServiceEntry::new("maya", "127.0.0.1", 9765);
        self_row.pid = Some(std::process::id());
        r.register(self_row).unwrap();

        // A real, non-self Maya instance on another port — must
        // survive.
        let mut other = ServiceEntry::new("maya", "127.0.0.1", 18815);
        other.pid = Some(std::process::id());
        r.register(other).unwrap();
    }

    let gs = test_gateway_state_with_own(registry.clone(), "127.0.0.1", 9765);
    let (alive, evicted) = gs
        .read_alive_instances(&*registry.read().await)
        .expect("read_alive_instances must succeed");

    assert_eq!(evicted, 0, "no rows were dead; nothing should be evicted");
    assert_eq!(
        alive.len(),
        1,
        "only the non-self non-sentinel maya row should remain; got {alive:#?}",
    );
    assert_eq!(alive[0].port, 18815);
    assert!(
        !alive
            .iter()
            .any(|e| e.dcc_type == GATEWAY_SENTINEL_DCC_TYPE),
        "sentinel must never appear in read_alive_instances output",
    );
}

// ── Sub-state view tests (issue #839) ──────────────────────────────────

/// The discovery view exposes exactly the subset a registry-facing
/// handler needs, and its liveness filter matches
/// [`GatewayState::live_instances`] byte-for-byte.
#[tokio::test]
async fn test_discovery_view_matches_gateway_state() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));

    {
        let r = registry.read().await;
        let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
        sentinel.version = Some(env!("CARGO_PKG_VERSION").into());
        r.register(sentinel).unwrap();
        r.register(ServiceEntry::new("maya", "127.0.0.1", 18812))
            .unwrap();
    }

    let gs = test_gateway_state_with_own(registry.clone(), "127.0.0.1", 9765);
    let via_gs = gs.live_instances(&*registry.read().await);
    let via_view = gs.discovery().live_instances(&*registry.read().await);
    assert_eq!(via_gs.len(), via_view.len());
    assert_eq!(via_gs[0].dcc_type, via_view[0].dcc_type);
    assert_eq!(via_gs[0].port, via_view[0].port);

    // Discovery view holds exactly the agreed fields (SRP).
    let d = gs.discovery();
    assert_eq!(d.stale_timeout, gs.stale_timeout);
    assert_eq!(d.allow_unknown_tools, gs.allow_unknown_tools);
    assert_eq!(d.own_host, gs.own_host.as_str());
    assert_eq!(d.own_port, gs.own_port);
}

/// Each sub-state view exposes the documented responsibility and
/// nothing else — asserted via field-count sanity checks so growth of
/// the god object cannot silently leak back in.
#[tokio::test]
async fn test_sub_state_views_carry_only_their_responsibility() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
    let gs = test_gateway_state(registry);

    // Routing view — fields match the documented dispatch surface.
    let r = gs.routing();
    assert_eq!(r.backend_timeout, gs.backend_timeout);
    assert_eq!(r.async_dispatch_timeout, gs.async_dispatch_timeout);
    assert_eq!(r.wait_terminal_timeout, gs.wait_terminal_timeout);

    // Events view — fields match the documented fan-out surface.
    let ev = gs.events();
    assert!(Arc::ptr_eq(ev.events_tx, &gs.events_tx));
    assert!(Arc::ptr_eq(
        ev.resource_subscriptions,
        &gs.resource_subscriptions
    ));
    assert!(Arc::ptr_eq(ev.capability_index, &gs.capability_index));
    assert!(Arc::ptr_eq(ev.event_log, &gs.event_log));

    // Server view — fields match the identity surface.
    let s = gs.server();
    assert_eq!(s.server_name, gs.server_name);
    assert_eq!(s.server_version, gs.server_version);
    assert!(Arc::ptr_eq(s.protocol_version, &gs.protocol_version));
    assert!(Arc::ptr_eq(s.yield_tx, &gs.yield_tx));
    assert_eq!(s.adapter_version, gs.adapter_version.as_deref());
    assert_eq!(s.adapter_dcc, gs.adapter_dcc.as_deref());
}
