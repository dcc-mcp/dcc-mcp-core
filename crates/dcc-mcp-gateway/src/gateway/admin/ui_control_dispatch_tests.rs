use std::sync::Arc;

use axum::Router;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::tests::admin_tests::{
    make_gateway_state, make_service_entry, spawn_sidecar_dispatch_backend,
};

async fn spawn_discovery_dispatch_backend(
    hits: Value,
) -> (u16, oneshot::Sender<()>, Arc<Mutex<Vec<Value>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls_for_rest = calls.clone();
    let app = Router::new()
        .route(
            "/v1/search",
            axum::routing::post(move || {
                let hits = hits.clone();
                async move { axum::Json(json!({ "hits": hits })) }
            }),
        )
        .route(
            "/v1/call",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, axum::Json(req): axum::Json<Value>| {
                    let calls = calls_for_rest.clone();
                    async move {
                        let request_id = headers
                            .get("x-request-id")
                            .and_then(|value| value.to_str().ok())
                            .filter(|value| !value.is_empty())
                            .expect("gateway REST calls must carry an X-Request-ID header")
                            .to_string();
                        calls.lock().push(json!({
                            "request_id": request_id.clone(),
                            "body": req,
                        }));
                        axum::Json(json!({
                            "request_id": request_id,
                            "isError": false,
                            "output": {"success": true, "snapshot_id": "snapshot-1"}
                        }))
                    }
                },
            ),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    (port, tx, calls)
}

#[tokio::test]
async fn gateway_call_routes_ui_control_to_sidecar_discovery_endpoint() {
    let gs = make_gateway_state();
    let (discovery_port, stop_discovery, discovery_calls) =
        spawn_discovery_dispatch_backend(json!([{
            "skill": "core",
            "action": "ui_control__snapshot",
            "summary": "Capture a bounded UI Control snapshot",
            "loaded": true,
            "has_schema": true
        }]))
        .await;
    let (sidecar_port, stop_sidecar, sidecar_calls) = spawn_sidecar_dispatch_backend().await;
    let mut entry = make_service_entry("3dsmax", "127.0.0.1", sidecar_port, None);
    entry.metadata.insert(
        crate::gateway::http_registration::MCP_URL_METADATA_KEY.to_string(),
        format!("http://127.0.0.1:{sidecar_port}/mcp"),
    );
    entry.metadata.insert(
        crate::gateway::http_registration::DISCOVERY_MCP_URL_METADATA_KEY.to_string(),
        format!("http://127.0.0.1:{discovery_port}/mcp"),
    );
    entry.metadata.insert(
        crate::gateway::http_registration::ROLE_METADATA_KEY.to_string(),
        crate::gateway::http_registration::ROLE_PER_DCC_SIDECAR.to_string(),
    );
    let instance_id = entry.instance_id;
    gs.registry.register(entry).unwrap();

    crate::gateway::capability_service::refresh_all_live_backends(
        &gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    let slug =
        crate::gateway::capability::tool_slug("3dsmax", &instance_id, "ui_control__snapshot");

    let result =
        crate::gateway::capability_service::call_service(&gs, &slug, json!({}), None, None, None)
            .await
            .expect("ui-control calls should use the in-process discovery endpoint");
    let _ = stop_discovery.send(());
    let _ = stop_sidecar.send(());

    assert_eq!(result["isError"], false);
    let discovery_calls = discovery_calls.lock();
    assert_eq!(discovery_calls.len(), 1);
    assert!(
        discovery_calls[0]["request_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "gateway REST calls must carry a non-empty request_id"
    );
    assert!(
        sidecar_calls.lock().is_empty(),
        "ui-control calls must not be sent to the sidecar action dispatcher"
    );
}
