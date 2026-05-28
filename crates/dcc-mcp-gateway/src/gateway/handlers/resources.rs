use dcc_mcp_gateway_core::resource_uri::decode_resource_uri;

use super::*;
use crate::gateway::http_registration::entry_mcp_url;

/// URI for the gateway's own contention event log (issue #766).
pub(crate) const GATEWAY_EVENTS_URI: &str = "resources://gateway/events";

pub(super) async fn handle_resources_list(gs: &GatewayState, id: Value) -> Value {
    let result = aggregator::aggregate_resources_list(gs).await;
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub(super) async fn handle_resources_read(
    gs: &GatewayState,
    id: Value,
    req: &super::mcp_impl::JsonRpcRequest,
) -> Value {
    let uri = req
        .params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();

    // ── Gateway-internal event log (issue #766) ──────────────────────────
    if uri == GATEWAY_EVENTS_URI {
        let jsonl = gs.event_log.as_jsonl();
        return json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "contents": [{
                    "uri":      GATEWAY_EVENTS_URI,
                    "mimeType": "application/x-ndjson",
                    "text":     jsonl
                }]
            }
        });
    }

    // ── Gateway-native resources (issues #813 phases 1+2) ────────────────
    if let Some(req) = crate::gateway::native_resources::Request::parse(&uri) {
        let tool_count = crate::gateway::tools::gateway_tool_defs()
            .as_array()
            .map_or(0, Vec::len);
        return match req.build_payload(gs, tool_count).await {
            Ok(payload) => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "contents": [{
                        "uri":      uri,
                        "mimeType": "application/json",
                        "text":     serde_json::to_string_pretty(&payload).unwrap_or_default()
                    }]
                }
            }),
            Err(err) => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32002, "message": err}
            }),
        };
    }

    if let Some((id8, backend_uri)) = decode_resource_uri(&uri) {
        let owning = aggregator::find_instance_by_prefix(gs, &id8).await;
        return match owning {
            Some(entry) => {
                let url = entry_mcp_url(&entry);
                match crate::gateway::backend_client::read_resource(
                    &gs.http_client,
                    &url,
                    &backend_uri,
                    gs.backend_timeout,
                )
                .await
                {
                    Ok(mut result) => {
                        rewrite_content_uris(&mut result, &backend_uri, &uri);
                        json!({"jsonrpc": "2.0", "id": id, "result": result})
                    }
                    Err(e) => json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": {"code": -32002, "message": format!("Backend resources/read failed: {e}")}
                    }),
                }
            }
            None => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32002, "message": format!("Resource not found: {uri} (no live instance matches prefix '{id8}')")}
            }),
        };
    }

    let parts: Vec<&str> = uri.trim_start_matches("dcc://").splitn(2, '/').collect();
    let registry = gs.registry.read().await;
    let found = gs.live_instances(&registry).into_iter().find(|entry| {
        parts.len() == 2
            && entry.dcc_type == parts[0]
            && entry.instance_id.to_string().starts_with(parts[1])
    });

    match found {
        Some(entry) => {
            let detail = gs.instance_json(&entry);
            json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "contents": [{
                        "uri":      uri,
                        "mimeType": "application/json",
                        "text":     serde_json::to_string_pretty(&detail).unwrap_or_default()
                    }]
                }
            })
        }
        None => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32002, "message": format!("Resource not found: {uri}")}
        }),
    }
}

fn rewrite_content_uris(result: &mut Value, backend_uri: &str, client_uri: &str) {
    let Some(contents) = result.get_mut("contents").and_then(Value::as_array_mut) else {
        return;
    };
    for entry in contents {
        if let Some(obj) = entry.as_object_mut()
            && obj
                .get("uri")
                .and_then(Value::as_str)
                .is_some_and(|u| u == backend_uri)
        {
            obj.insert("uri".to_string(), Value::String(client_uri.to_string()));
        }
    }
}

pub(super) async fn handle_resource_subscription(
    gs: &GatewayState,
    id: Value,
    req: &super::mcp_impl::JsonRpcRequest,
    session_id: &str,
    subscribe: bool,
) -> Value {
    let uri = req
        .params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();

    {
        let mut subscriptions = gs.resource_subscriptions.write().await;
        if subscribe {
            subscriptions
                .entry(session_id.to_owned())
                .or_default()
                .insert(uri.clone());
        } else if let Some(set) = subscriptions.get_mut(session_id) {
            set.remove(&uri);
        }
    }

    if let Some((id8, backend_uri)) = decode_resource_uri(&uri) {
        let owning = aggregator::find_instance_by_prefix(gs, &id8).await;
        return match owning {
            Some(entry) => {
                let backend_url = entry_mcp_url(&entry);
                if subscribe {
                    gs.subscriber.bind_resource_subscription(
                        &backend_url,
                        &backend_uri,
                        session_id,
                        &uri,
                    );
                    gs.subscriber.ensure_subscribed(&backend_url);
                } else {
                    gs.subscriber.unbind_resource_subscription(
                        &backend_url,
                        &backend_uri,
                        session_id,
                        &uri,
                    );
                }

                let Some(backend_session_id) = gs
                    .subscriber
                    .wait_for_backend_session_id(&backend_url, std::time::Duration::from_secs(3))
                    .await
                else {
                    if subscribe {
                        gs.subscriber.unbind_resource_subscription(
                            &backend_url,
                            &backend_uri,
                            session_id,
                            &uri,
                        );
                    }
                    return json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": {"code": -32002, "message": format!("Backend {backend_url} SSE subscriber not yet ready; retry")}
                    });
                };

                match crate::gateway::backend_client::subscribe_resource(
                    &gs.http_client,
                    &backend_url,
                    &backend_uri,
                    subscribe,
                    &backend_session_id,
                    gs.backend_timeout,
                )
                .await
                {
                    Ok(_) => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                    Err(e) => {
                        if subscribe {
                            gs.subscriber.unbind_resource_subscription(
                                &backend_url,
                                &backend_uri,
                                session_id,
                                &uri,
                            );
                        }
                        json!({
                            "jsonrpc": "2.0", "id": id,
                            "error": {"code": -32002, "message": format!("Backend resources/{}: {e}", if subscribe { "subscribe" } else { "unsubscribe" })}
                        })
                    }
                }
            }
            None => json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32002, "message": format!("Resource not found: {uri} (no live instance matches prefix '{id8}')")}
            }),
        };
    }

    json!({"jsonrpc":"2.0","id":id,"result":{}})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event_log::{ContendEvent, EventKind};
    use crate::gateway::handlers::mcp_impl::JsonRpcRequest;
    use crate::gateway::state::GatewayState;
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;
    use dcc_mcp_transport::discovery::types::ServiceEntry;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::{RwLock, broadcast, watch};

    fn test_gs_with_events(events: Vec<ContendEvent>) -> GatewayState {
        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(FileRegistry::new(dir.path()).unwrap()));
        let (yield_tx, _) = watch::channel(false);
        let (events_tx, _) = broadcast::channel::<String>(8);
        let log = Arc::new(crate::gateway::event_log::EventLog::new());
        for e in events {
            log.push(e);
        }
        GatewayState {
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
            stale_timeout: std::time::Duration::from_secs(30),
            backend_timeout: std::time::Duration::from_secs(10),
            async_dispatch_timeout: std::time::Duration::from_secs(60),
            wait_terminal_timeout: std::time::Duration::from_secs(600),
            server_name: "test".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
            own_host: "127.0.0.1".into(),
            own_port: 9765,
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
            allow_unknown_tools: false,
            policy: Arc::new(crate::gateway::GatewayPolicy::default()),
            adapter_version: None,
            adapter_dcc: None,
            capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
            event_log: log,
            middleware_chain: std::sync::Arc::new(
                crate::gateway::middleware::MiddlewareChain::new(),
            ),
            instance_diagnostics: Arc::new(
                crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
            ),
            traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
            search_telemetry: Arc::new(
                crate::gateway::search_telemetry::SearchTelemetryStore::new(),
            ),
            debug_routes_enabled: false,
            #[cfg(feature = "prometheus")]
            gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
        }
    }

    fn make_read_req(uri: &str) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(1)),
            method: "resources/read".into(),
            params: Some(json!({"uri": uri})),
        }
    }

    /// Issue #766: `resources://gateway/events` must return JSONL text content
    /// containing every event pushed to the ring buffer.
    #[tokio::test]
    async fn gateway_events_resource_returns_jsonl() {
        let events = vec![
            ContendEvent::new(EventKind::ElectionWon, "gateway", "abcd1234", None),
            ContendEvent::new(
                EventKind::ProbeBooting,
                "maya",
                "ef012345",
                Some("still starting".into()),
            ),
        ];
        let gs = test_gs_with_events(events);

        let req = make_read_req(GATEWAY_EVENTS_URI);
        let resp = handle_resources_read(&gs, json!(1), &req).await;

        let text = resp["result"]["contents"][0]["text"]
            .as_str()
            .expect("response must contain text content");
        let mime = resp["result"]["contents"][0]["mimeType"]
            .as_str()
            .expect("response must contain mimeType");

        assert_eq!(mime, "application/x-ndjson");

        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "expected 2 JSONL lines, got: {text:?}");

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["event"], "election_won");
        assert_eq!(first["dcc_type"], "gateway");

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["event"], "probe_booting");
        assert_eq!(second["reason"], "still starting");
    }

    /// Issue #766: empty event log returns an empty string (not an error).
    #[tokio::test]
    async fn gateway_events_resource_empty_log() {
        let gs = test_gs_with_events(vec![]);
        let req = make_read_req(GATEWAY_EVENTS_URI);
        let resp = handle_resources_read(&gs, json!(1), &req).await;

        assert!(
            resp.get("error").is_none(),
            "empty log must not return an error; got {resp}"
        );
        let text = resp["result"]["contents"][0]["text"]
            .as_str()
            .expect("must return text content");
        assert!(text.is_empty(), "empty log text must be empty string");
    }

    #[tokio::test]
    async fn gateway_instances_resource_includes_lifecycle_hints() {
        let gs = test_gs_with_events(Vec::new());
        {
            let registry = gs.registry.write().await;
            let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
            entry.version = Some("2026".into());
            entry.adapter_version = Some("1.2.0".into());
            entry
                .metadata
                .insert("dcc_mcp_role".into(), "per-dcc-sidecar".into());
            entry.metadata.insert("sidecar_pid".into(), "31337".into());
            entry.metadata.insert(
                "restart_command".into(),
                "rez-env dcc_mcp_maya -- maya-sidecar".into(),
            );
            registry.register(entry).unwrap();
        }

        let req = make_read_req("gateway://instances");
        let resp = handle_resources_read(&gs, json!(1), &req).await;
        let text = resp["result"]["contents"][0]["text"]
            .as_str()
            .expect("response must contain text content");
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();
        let row = &payload["instances"][0];

        assert_eq!(payload["by_source"]["file"], 1);
        assert_eq!(payload["by_source"]["http"], 0);
        assert_eq!(payload["by_source"]["mdns"], 0);
        assert_eq!(payload["by_source"]["relay"], 0);
        assert_eq!(row["source"], "file");
        assert_eq!(row["instance_short"].as_str().unwrap().len(), 8);
        assert!(row["source_meta"].as_object().unwrap().is_empty());
        assert_eq!(row["version"], "2026");
        assert_eq!(row["adapter_version"], "1.2.0");
        assert_eq!(row["lifecycle"]["role"], "per-dcc-sidecar");
        assert_eq!(row["lifecycle"]["sidecar_pid"], 31337);
        assert_eq!(row["lifecycle"]["supports_safe_stop"], true);
        assert_eq!(row["lifecycle"]["restartable"], true);
        assert_eq!(
            row["lifecycle"]["restart_command"],
            "rez-env dcc_mcp_maya -- maya-sidecar"
        );
    }

    #[test]
    fn rewrites_matching_content_uris_only() {
        let mut result = json!({
            "contents": [
                {"uri": "file://scene.ma", "text": "a"},
                {"uri": "file://other.ma", "text": "b"},
                {"text": "c"}
            ]
        });

        rewrite_content_uris(&mut result, "file://scene.ma", "file://abcd1234/scene.ma");

        assert_eq!(result["contents"][0]["uri"], "file://abcd1234/scene.ma");
        assert_eq!(result["contents"][1]["uri"], "file://other.ma");
        assert!(result["contents"][2].get("uri").is_none());
    }
}
