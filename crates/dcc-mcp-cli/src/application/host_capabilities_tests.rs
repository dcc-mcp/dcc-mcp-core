use axum::extract::{Json, OriginalUri};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::application::capabilities_result::backend_summary;
use crate::application::client::TransportMode;
use crate::application::gateway_profile::GatewayTarget;
use crate::application::local_control::call_result_succeeded;
use crate::domain::rest::Endpoint;
use crate::presentation::cli::capabilities_output::render_human;

fn summary() -> Value {
    json!({
        "slug": "dcc_host_capabilities",
        "output": {
            "success": true,
            "message": "houdini host capabilities",
            "context": {
                "host_version": "20.5",
                "adapter_version": "0.3.1",
                "probed_at": "2026-11-20T00:00:00Z",
                "available": [{
                    "key": "render.lpe",
                    "supported": true,
                    "probe_result": "PASS",
                    "probed_at": "2026-11-20T00:00:00Z"
                }],
                "declared": [],
                "unavailable": [],
                "not_run": [],
                "totals": {
                    "available": 1,
                    "declared": 0,
                    "unavailable": 0,
                    "not_run": 0
                }
            }
        }
    })
}

fn correlated_json(headers: &HeaderMap, value: Value) -> Response {
    let request_id = headers
        .get("x-request-id")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("test-request"));
    let mut response = Json(value).into_response();
    response.headers_mut().insert("x-request-id", request_id);
    response
}

fn inventory() -> Value {
    json!({
        "instances": [
            {
                "dcc_type": "houdini",
                "instance_id": "abc12345-0000-0000-0000-000000000000",
                "instance_short": "abc12345"
            },
            {
                "dcc_type": "photoshop",
                "instance_id": "def67890-0000-0000-0000-000000000000",
                "instance_short": "def67890"
            }
        ]
    })
}

#[tokio::test]
async fn repeated_keys_and_force_refresh_reach_the_direct_gateway_call() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    let app = axum::Router::new()
        .route("/v1/instances", get(|| async { Json(inventory()) }))
        .route("/admin/api/health", get(|| async { Json(json!({})) }))
        .route(
            "/v1/dcc/{dcc_type}/instances/{instance_id}/call",
            post(
                move |headers: HeaderMap,
                      OriginalUri(uri): OriginalUri,
                      Json(body): Json<Value>| async move {
                    let path = uri
                        .path()
                        .trim_start_matches('/')
                        .split('/')
                        .collect::<Vec<_>>();
                    let mut seen = seen.lock().unwrap();
                    seen.push(json!({
                        "dcc_type": path[2],
                        "instance_id": path[4],
                        "body": body
                    }));
                    correlated_json(&headers, summary())
                },
            ),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        None,
        true,
    );

    let result = CapabilitiesCommand::new(
        Some("houdini".to_string()),
        Some("abc12345".to_string()),
        vec![
            "render.lpe, render.cryptomatte".to_string(),
            "render.lpe".to_string(),
        ],
        true,
    )
    .run(&control, Duration::from_secs(2))
    .await
    .unwrap();

    let seen = calls.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0]["dcc_type"], "houdini");
    assert_eq!(
        seen[0]["instance_id"],
        "abc12345-0000-0000-0000-000000000000"
    );
    assert_eq!(seen[0]["body"]["backend_tool"], "dcc_host_capabilities");
    assert_eq!(seen[0]["body"]["arguments"]["force_refresh"], true);
    assert_eq!(
        seen[0]["body"]["arguments"]["keys"][0],
        "render.cryptomatte"
    );
    assert_eq!(seen[0]["body"]["arguments"]["keys"][1], "render.lpe");
    assert_eq!(result["dcc_type"], "houdini");
    assert_eq!(result["instance_short"], "abc12345");
    assert_eq!(result["control_route"], "gateway");

    let backend_ok = call_result_succeeded(&result);
    let has_summary = backend_summary(&result).is_some_and(Value::is_object);
    assert!(backend_ok);
    assert!(has_summary);

    let rendered = render_human(&result);
    assert!(rendered.contains("host=20.5 adapter=0.3.1"));
    assert!(rendered.contains("AVAILABLE (1)"));
    assert!(rendered.contains("render.lpe [PASS]"));
    server.abort();
}

#[tokio::test]
async fn unique_instance_id_is_resolved_without_caller_supplying_one() {
    let app = axum::Router::new()
        .route("/v1/instances", get(|| async { Json(inventory()) }))
        .route("/admin/api/health", get(|| async { Json(json!({})) }))
        .route(
            "/v1/dcc/{dcc_type}/instances/{instance_id}/call",
            post(
                |headers: HeaderMap, OriginalUri(uri): OriginalUri| async move {
                    let path = uri
                        .path()
                        .trim_start_matches('/')
                        .split('/')
                        .collect::<Vec<_>>();
                    correlated_json(
                        &headers,
                        json!({
                            "dcc_type": path[2],
                            "instance_id": path[4],
                            "context": {}
                        }),
                    )
                },
            ),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let control = DccControlPlane::new(
        GatewayTarget::Local,
        Endpoint::new(format!("http://{addr}")),
        None,
        true,
    );

    let result = CapabilitiesCommand::new(Some("photoshop".to_string()), None, Vec::new(), false)
        .run(&control, Duration::from_secs(2))
        .await
        .unwrap();

    assert_eq!(result["dcc_type"], "photoshop");
    assert_eq!(
        result["instance_id"],
        "def67890-0000-0000-0000-000000000000"
    );
    server.abort();
}

#[test]
fn missing_instance_reports_the_requested_filters() {
    let error = resolve_target(&inventory(), Some("nuke"), None).unwrap_err();

    assert_eq!(
        error.to_string(),
        "no DCC instance matched --dcc-type=nuke --instance-id=*"
    );
}

#[test]
fn ambiguous_instances_list_full_identity_candidates() {
    let payload = json!({
        "instances": [
            {
                "dcc_type": "maya",
                "instance_id": "abc10000-0000-0000-0000-000000000000",
                "instance_short": "abc10000"
            },
            {
                "dcc_type": "maya",
                "instance_id": "abc20000-0000-0000-0000-000000000000",
                "instance_short": "abc20000"
            }
        ]
    });

    let error = resolve_target(&payload, Some("maya"), None).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("multiple DCC instances matched capabilities"));
    assert!(message.contains("instance_short=abc10000"));
    assert!(message.contains("instance_id=abc20000-0000-0000-0000-000000000000"));
}

#[test]
fn normalization_splits_commas_and_deduplicates_keys() {
    assert_eq!(
        normalize_keys(vec![
            " render.lpe, render.cryptomatte ".to_string(),
            "render.lpe".to_string(),
            ",".to_string(),
        ]),
        vec!["render.cryptomatte".to_string(), "render.lpe".to_string()]
    );
}

#[test]
fn mcp_transport_keeps_the_existing_direct_call_guard() {
    let control = DccControlPlane::new(
        GatewayTarget::Remote {
            name: "pcA".to_string(),
            endpoint: Endpoint::new("http://127.0.0.1:9765"),
        },
        Endpoint::new("http://127.0.0.1:9765"),
        None,
        false,
    )
    .with_transport(TransportMode::Mcp);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = runtime.block_on(async {
        control
            .call(
                HOST_CAPABILITIES_TOOL.to_string(),
                Some("nuke".to_string()),
                Some("abc12345-0000-0000-0000-000000000000".to_string()),
                json!({}),
                None,
                Duration::from_secs(1),
            )
            .await
            .unwrap_err()
    });

    assert!(
        error
            .to_string()
            .contains("direct backend calls need REST routing")
    );
}
