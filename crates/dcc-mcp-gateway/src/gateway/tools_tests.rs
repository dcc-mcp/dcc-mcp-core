use super::tools::*;
use crate::gateway::capability::InstanceFingerprint;
use crate::gateway::capability_service;
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast, watch};
use uuid::Uuid;

fn test_gateway_state() -> GatewayState {
    let dir = tempfile::tempdir().unwrap();
    let (yield_tx, _) = watch::channel(false);
    let (events_tx, _) = broadcast::channel::<String>(8);
    GatewayState {
        registry: Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap())),
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
        server_name: "test".into(),
        server_version: env!("CARGO_PKG_VERSION").into(),
        own_host: "127.0.0.1".into(),
        own_port: 0,
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
        event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
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

fn annotations_by_tool() -> Map<String, Value> {
    gateway_tool_defs()
        .as_array()
        .expect("gateway_tool_defs returns an array")
        .iter()
        .map(|tool| {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .expect("gateway tool has a name")
                .to_string();
            let annotations = tool
                .get("annotations")
                .cloned()
                .expect("gateway tool has annotations");
            (name, annotations)
        })
        .collect()
}

#[test]
fn gateway_tool_defs_advertise_canonical_workflow_tools_only() {
    let defs = gateway_tool_defs();
    let names: Vec<&str> = defs
        .as_array()
        .expect("gateway_tool_defs returns an array")
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();

    assert_eq!(names, ["search", "describe", "load_skill", "call"]);
}

#[test]
fn gateway_tool_defs_all_have_annotations() {
    let annotations = annotations_by_tool();
    assert_eq!(annotations.len(), 4);

    for (name, value) in annotations {
        let hints = value
            .as_object()
            .unwrap_or_else(|| panic!("{name} annotations must be an object"));
        assert!(
            [
                "readOnlyHint",
                "destructiveHint",
                "idempotentHint",
                "openWorldHint"
            ]
            .iter()
            .any(|key| hints.contains_key(*key)),
            "{name} annotations must include at least one MCP ToolAnnotations hint"
        );
    }
}

#[test]
fn gateway_call_schema_keeps_compatibility_shape() {
    let defs = gateway_tool_defs();
    let call = defs
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "call")
        .expect("call tool advertised");

    assert_eq!(call["inputSchema"]["type"], "object");
    assert!(call["inputSchema"].get("anyOf").is_none());
    assert!(call["inputSchema"].get("oneOf").is_none());
    assert!(call["inputSchema"].get("allOf").is_none());
    assert!(call["inputSchema"].get("not").is_none());
    assert!(call["inputSchema"].get("required").is_none());
    assert!(call["inputSchema"]["properties"]["tool_slug"].is_object());
    assert_eq!(call["inputSchema"]["properties"]["calls"]["maxItems"], 25);
    assert_eq!(call["annotations"]["destructiveHint"], true);
}

#[test]
fn gateway_tool_defs_use_expected_annotations() {
    let annotations = annotations_by_tool();

    assert_eq!(
        annotations.get("search"),
        Some(&json!({"readOnlyHint": true, "openWorldHint": true}))
    );
    assert_eq!(
        annotations.get("describe"),
        Some(&json!({"readOnlyHint": true, "openWorldHint": true}))
    );
}

#[test]
fn describe_refresh_is_conditional_on_generation_and_index_hit() {
    let gs = test_gateway_state();
    let instance_id = Uuid::from_u128(0x1234);
    let record = crate::gateway::capability::CapabilityRecord::new(
        crate::gateway::capability::tool_slug("maya", &instance_id, "maya_scene__list_objects"),
        "maya_scene__list_objects".to_string(),
        "maya_scene__list_objects".to_string(),
        Some("maya-scene".into()),
        "List scene objects",
        vec!["scene".into()],
        "maya".into(),
        instance_id,
        false,
        true,
        None,
    );
    let fingerprint = InstanceFingerprint(1);
    gs.capability_index
        .upsert_instance(instance_id, vec![record.clone()], fingerprint);

    let current_generation = capability_service::index_generation(&gs.capability_index);
    let args = json!({
        "tool_slug": record.tool_slug,
        "meta": {"index_generation": current_generation}
    });
    assert!(!describe_needs_refresh(&gs, &record.tool_slug, &args, None));

    let stale_args = json!({
        "tool_slug": record.tool_slug,
        "meta": {"index_generation": "stale"}
    });
    assert!(describe_needs_refresh(
        &gs,
        &record.tool_slug,
        &stale_args,
        None
    ));

    assert!(describe_needs_refresh(
        &gs,
        "maya.abcdef01.__missing__",
        &json!({}),
        None
    ));
}

#[test]
fn calls_refresh_only_after_route_index_errors() {
    use crate::gateway::capability_service::ServiceError;

    assert!(call_error_needs_refresh(&ServiceError::new(
        "unknown-slug",
        "missing capability",
    )));
    assert!(call_error_needs_refresh(&ServiceError::new(
        "instance-offline",
        "live instance not indexed yet",
    )));
    assert!(!call_error_needs_refresh(&ServiceError::new(
        "backend-error",
        "tool execution failed",
    )));
    assert!(!call_error_needs_refresh(&ServiceError::new(
        "policy-denied",
        "request rejected",
    )));
}
