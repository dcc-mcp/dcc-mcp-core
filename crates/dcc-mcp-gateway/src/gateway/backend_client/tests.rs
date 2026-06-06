use super::error::BackendCallError;
use super::http::{post_jsonrpc, uuid_like_id};
use super::urls::{healthz_url_from_mcp_url, rest_base_from_mcp_url};
use super::*;

use dcc_mcp_jsonrpc::McpTool;
use serde_json::{Value, json};
use std::time::Duration;

// ── helper used only in tests ────────────────────────────────────────

fn parse_response_body(body: &str) -> Result<Value, String> {
    let parsed: Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON-RPC response: {e}"))?;
    if let Some(err) = parsed.get("error") {
        let code = err.get("code").and_then(Value::as_i64).unwrap_or(-1);
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(format!("backend error {code}: {msg}"));
    }
    parsed
        .get("result")
        .cloned()
        .ok_or_else(|| "empty JSON-RPC result".to_string())
}

async fn spawn_fake_backend(app: axum::Router) -> (String, tokio::sync::oneshot::Sender<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await
            .ok();
    });
    (format!("http://127.0.0.1:{port}/mcp"), tx)
}

fn rest_backend_router() -> axum::Router {
    use axum::extract::Path;
    axum::Router::new()
            .route(
                "/health",
                axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
            )
            .route("/v1/resources", axum::routing::get(|| async {
                axum::Json(json!({
                    "total": 2,
                    "resources": [
                        {"uri": "scene://current", "name": "Current scene", "mimeType": "application/json"},
                        {"uri": "capture://current_window", "name": "Window capture", "mimeType": "image/png"}
                    ]
                }))
            }))
            .route("/v1/resources/{uri}", axum::routing::get(|Path(uri): Path<String>| async move {
                axum::Json(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "image/png",
                        "blob": "aGVsbG8sIHdvcmxkIQ==",
                    }]
                }))
            }))
}

// ── unit tests ───────────────────────────────────────────────────────

#[test]
fn uuid_like_id_increments_monotonically() {
    let a = uuid_like_id();
    let b = uuid_like_id();
    assert_ne!(a, b);
    assert!(a.starts_with("gw-"));
    assert!(b.starts_with("gw-"));
}

#[test]
fn builds_health_url_from_mcp_url() {
    assert_eq!(
        health_url_from_mcp_url("http://127.0.0.1:64954/mcp"),
        "http://127.0.0.1:64954/health"
    );
    assert_eq!(
        health_url_from_mcp_url("http://127.0.0.1:64954/mcp/"),
        "http://127.0.0.1:64954/health"
    );
}

#[test]
fn builds_healthz_url_from_mcp_url() {
    assert_eq!(
        healthz_url_from_mcp_url("http://127.0.0.1:64954/mcp"),
        "http://127.0.0.1:64954/healthz"
    );
    assert_eq!(
        healthz_url_from_mcp_url("http://127.0.0.1:64954/mcp/"),
        "http://127.0.0.1:64954/healthz"
    );
}

#[test]
fn builds_readyz_url_from_mcp_url() {
    assert_eq!(
        readyz_url_from_mcp_url("http://127.0.0.1:64954/mcp"),
        "http://127.0.0.1:64954/v1/readyz"
    );
    assert_eq!(
        readyz_url_from_mcp_url("http://127.0.0.1:64954/mcp/"),
        "http://127.0.0.1:64954/v1/readyz"
    );
    assert_eq!(
        readyz_url_from_mcp_url("http://127.0.0.1:64954"),
        "http://127.0.0.1:64954/v1/readyz"
    );
}

#[test]
fn probe_outcome_is_ready_and_is_alive() {
    assert!(ProbeOutcome::Ready.is_ready());
    assert!(!ProbeOutcome::Booting.is_ready());
    assert!(!ProbeOutcome::Unreachable.is_ready());

    assert!(ProbeOutcome::Ready.is_alive());
    assert!(ProbeOutcome::Booting.is_alive());
    assert!(!ProbeOutcome::Unreachable.is_alive());
}

#[tokio::test]
async fn skill_rest_metadata_survives_fetch_and_describe() {
    let app = axum::Router::new()
        .route(
            "/v1/search",
            axum::routing::post(|| async {
                axum::Json(json!({
                    "total": 1,
                    "hits": [{
                        "action": "app_ui__snapshot",
                        "summary": "Capture app UI snapshot",
                        "has_schema": true,
                        "loaded": true,
                        "annotations": {
                            "readOnlyHint": true,
                            "destructiveHint": false,
                            "idempotentHint": true
                        },
                        "metadata": {
                            "dcc": {
                                "affinity": "any",
                                "execution": "sync",
                                "timeoutHintSecs": 2,
                                "risk": "read-only",
                                "searchAliases": ["screen capture"],
                                "searchTokens": ["schema:session_id"]
                            }
                        }
                    }, {
                        "action": "loadable_export__save",
                        "skill": "loadable-export",
                        "summary": "Save the current document",
                        "has_schema": true,
                        "loaded": false,
                        "metadata": {
                            "dcc": {
                                "searchAliases": ["write file"],
                                "searchTokens": ["required:destination_path"]
                            }
                        }
                    }]
                }))
            }),
        )
        .route(
            "/v1/describe",
            axum::routing::post(|| async {
                axum::Json(json!({
                    "entry": {"action": "app_ui__snapshot"},
                    "description": "Capture app UI snapshot",
                    "input_schema": {"type": "object", "properties": {"session_id": {"type": "string"}}},
                    "annotations": {"readOnlyHint": true, "idempotentHint": true},
                    "metadata": {
                        "dcc": {
                            "affinity": "any",
                            "execution": "sync",
                            "timeoutHintSecs": 2,
                            "risk": "read-only"
                        },
                        "dcc.next_tools": {
                            "on_success": ["app_ui__inspect"],
                            "on_failure": ["dcc_diagnostics__screenshot"]
                        }
                    }
                }))
            }),
        );
    let (mcp_url, stop) = spawn_fake_backend(app).await;
    let client = reqwest::Client::new();

    let (tools, unloaded) = try_fetch_tools(&client, &mcp_url, Duration::from_secs(2))
        .await
        .expect("search");
    assert_eq!(unloaded.len(), 1);
    assert_eq!(unloaded[0].skill_name, "loadable-export");
    assert!(
        unloaded[0]
            .search_tokens
            .contains(&"alias:write file".to_string())
    );
    assert!(
        unloaded[0]
            .search_tokens
            .contains(&"required:destination_path".to_string())
    );
    assert_eq!(tools[0].name, "app_ui__snapshot");
    assert_eq!(
        tools[0]
            .annotations
            .as_ref()
            .and_then(|ann| ann.read_only_hint),
        Some(true)
    );
    assert_eq!(
        tools[0]
            .meta
            .as_ref()
            .and_then(|meta| meta.get("dcc"))
            .and_then(|dcc| dcc.get("has_schema"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        tools[0]
            .meta
            .as_ref()
            .and_then(|meta| meta.get("dcc"))
            .and_then(|dcc| dcc.get("timeoutHintSecs"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        tools[0]
            .meta
            .as_ref()
            .and_then(|meta| meta.get("dcc"))
            .and_then(|dcc| dcc.get("searchTokens"))
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_str),
        Some("schema:session_id")
    );

    let described = try_describe_tool(
        &client,
        &mcp_url,
        "app_ui__snapshot",
        Duration::from_secs(2),
    )
    .await
    .expect("describe");
    assert_eq!(
        described
            .meta
            .as_ref()
            .and_then(|meta| meta.get("dcc"))
            .and_then(|dcc| dcc.get("risk"))
            .and_then(Value::as_str),
        Some("read-only")
    );
    // next-tools hints authored in tools.yaml must survive the describe
    // round-trip so agents can pre-plan failure recovery (issue #1408).
    let next_tools = described
        .meta
        .as_ref()
        .and_then(|meta| meta.get("dcc.next_tools"))
        .expect("dcc.next_tools forwarded through describe");
    assert_eq!(
        next_tools.get("on_failure").and_then(Value::as_array),
        Some(&vec![Value::String("dcc_diagnostics__screenshot".into())])
    );
    assert_eq!(
        next_tools.get("on_success").and_then(Value::as_array),
        Some(&vec![Value::String("app_ui__inspect".into())])
    );
    let _ = stop.send(());
}

#[test]
fn backend_call_error_display_is_stable() {
    let cases: &[(BackendCallError, &[&str])] = &[
        (
            BackendCallError::Booting {
                mcp_url: "http://127.0.0.1:9/mcp".into(),
            },
            &[
                "http://127.0.0.1:9/mcp",
                "backend not ready",
                "/v1/readyz",
                "host DCC still initialising",
            ],
        ),
        (
            BackendCallError::Unreachable {
                mcp_url: "http://127.0.0.1:9/mcp".into(),
            },
            &[
                "http://127.0.0.1:9/mcp",
                "not a DCC MCP HTTP endpoint",
                "/v1/readyz",
                "/health",
                "/healthz",
            ],
        ),
        (
            BackendCallError::Transport {
                mcp_url: "http://x/mcp".into(),
                reason: "connection refused".into(),
            },
            &["http://x/mcp", "transport error", "connection refused"],
        ),
        (
            BackendCallError::Http {
                mcp_url: "http://x/mcp".into(),
                status: "500 Internal Server Error".into(),
                body: "oops".into(),
            },
            &["http://x/mcp", "HTTP ", "500 Internal Server Error", "oops"],
        ),
        (
            BackendCallError::ReadBody {
                mcp_url: "http://x/mcp".into(),
                reason: "eof".into(),
            },
            &["http://x/mcp", "read body", "eof"],
        ),
        (
            BackendCallError::InvalidJson {
                mcp_url: "http://x/mcp".into(),
                reason: "expected value".into(),
            },
            &[
                "http://x/mcp",
                "invalid JSON-RPC response",
                "expected value",
            ],
        ),
        (
            BackendCallError::Backend {
                mcp_url: "http://x/mcp".into(),
                code: -32601,
                message: "Method not found".into(),
            },
            &[
                "http://x/mcp",
                "backend error",
                "-32601",
                "Method not found",
            ],
        ),
        (
            BackendCallError::EmptyResult {
                mcp_url: "http://x/mcp".into(),
            },
            &["http://x/mcp", "empty JSON-RPC result"],
        ),
    ];

    for (err, needles) in cases {
        let rendered = err.to_string();
        for needle in *needles {
            assert!(
                rendered.contains(needle),
                "variant {err:?} missing {needle:?} in output: {rendered}",
            );
        }
    }
}

#[tokio::test]
async fn post_jsonrpc_forwards_session_header_when_provided() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let saw_header = Arc::new(AtomicBool::new(false));
    let saw_header_clone = saw_header.clone();
    let app = axum::Router::new().route(
        "/mcp",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, _body: axum::body::Bytes| {
                let saw = saw_header_clone.clone();
                async move {
                    if headers.get("mcp-session-id").and_then(|v| v.to_str().ok())
                        == Some("session-abc")
                    {
                        saw.store(true, Ordering::SeqCst);
                    }
                    axum::Json(json!({"jsonrpc":"2.0","id":"x","result":{"ok":true}}))
                }
            },
        ),
    );
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    let body = json!({"jsonrpc":"2.0","id":"x","method":"ping"});
    let result = post_jsonrpc(
        &client,
        &mcp_url,
        body,
        Some("session-abc"),
        Duration::from_secs(2),
    )
    .await
    .expect("post_jsonrpc must succeed against the fake backend");
    assert_eq!(result, json!({"ok": true}));
    assert!(
        saw_header.load(Ordering::SeqCst),
        "backend must observe the Mcp-Session-Id header the caller requested",
    );
    let _ = stop.send(());
}

#[tokio::test]
async fn post_jsonrpc_omits_session_header_when_none() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let had_header = Arc::new(AtomicBool::new(false));
    let had_header_clone = had_header.clone();
    let app = axum::Router::new().route(
        "/mcp",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, _body: axum::body::Bytes| {
                let h = had_header_clone.clone();
                async move {
                    if headers.get("mcp-session-id").is_some() {
                        h.store(true, Ordering::SeqCst);
                    }
                    axum::Json(json!({"jsonrpc":"2.0","id":"x","result":{}}))
                }
            },
        ),
    );
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    let _ = post_jsonrpc(
        &client,
        &mcp_url,
        json!({"jsonrpc":"2.0","id":"x","method":"ping"}),
        None,
        Duration::from_secs(2),
    )
    .await
    .expect("must succeed");
    assert!(
        !had_header.load(Ordering::SeqCst),
        "no session id → no Mcp-Session-Id header leaks to the backend",
    );
    let _ = stop.send(());
}

#[test]
fn parses_success_result() {
    let body = r#"{"jsonrpc":"2.0","id":"gw-1","result":{"tools":[]}}"#;
    let result = parse_response_body(body).unwrap();
    assert_eq!(result, json!({"tools": []}));
}

#[test]
fn parses_backend_error_into_error_string() {
    let body =
        r#"{"jsonrpc":"2.0","id":"gw-1","error":{"code":-32601,"message":"Method not found"}}"#;
    let err = parse_response_body(body).unwrap_err();
    assert!(err.contains("-32601"));
    assert!(err.contains("Method not found"));
}

#[test]
fn treats_missing_result_as_error() {
    let body = r#"{"jsonrpc":"2.0","id":"gw-1"}"#;
    let err = parse_response_body(body).unwrap_err();
    assert!(err.contains("empty"), "got: {err}");
}

#[test]
fn rejects_malformed_json() {
    let body = "not json at all";
    let err = parse_response_body(body).unwrap_err();
    assert!(err.contains("invalid JSON-RPC"), "got: {err}");
}

#[test]
fn extracts_tools_array_from_tools_list_result() {
    let result = json!({
        "tools": [
            {"name": "create_sphere", "description": "make sphere", "inputSchema": {"type": "object"}},
            {"name": "delete_node", "description": "delete", "inputSchema": {"type": "object"}}
        ]
    });
    let tools: Vec<McpTool> = result
        .get("tools")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value::<McpTool>(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "create_sphere");
    assert_eq!(tools[1].name, "delete_node");
}

#[test]
fn handles_tools_list_with_malformed_entries_gracefully() {
    let result = json!({
        "tools": [
            {"name": "good_tool", "description": "ok", "inputSchema": {"type": "object"}},
            {"not_a_tool": true}
        ]
    });
    let tools: Vec<McpTool> = result
        .get("tools")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value::<McpTool>(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "good_tool");
}

// ── #713 integration tests: readiness probe ──────────────────────────

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn probe_readiness_parses_200_green_report() {
    let app = axum::Router::new().route(
        "/v1/readyz",
        axum::routing::get(|| async {
            axum::Json(json!({
                "process": true,
                "dispatcher": true,
                "dcc": true,
            }))
        }),
    );
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    let report = probe_readiness(&client, &mcp_url, Duration::from_secs(2))
        .await
        .expect("readyz should answer");
    assert!(report.is_ready(), "base routing bits green -> is_ready()");
    assert_eq!(
        probe_mcp_readiness(&client, &mcp_url, Duration::from_secs(2)).await,
        ProbeOutcome::Ready
    );
    assert!(probe_mcp_health(&client, &mcp_url, Duration::from_secs(2)).await);
    let _ = stop.send(());
}

#[tokio::test]
async fn probe_readiness_parses_503_red_report_as_booting() {
    let app = axum::Router::new().route(
        "/v1/readyz",
        axum::routing::get(|| async {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(json!({
                    "process": true,
                    "dispatcher": true,
                    "dcc": false,
                })),
            )
        }),
    );
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    let report = probe_readiness(&client, &mcp_url, Duration::from_secs(2))
        .await
        .expect("red readyz still returns a parseable body");
    assert!(!report.is_ready());
    assert!(report.process);
    assert!(!report.dcc);

    let outcome = probe_mcp_readiness(&client, &mcp_url, Duration::from_secs(2)).await;
    assert_eq!(outcome, ProbeOutcome::Booting);
    assert!(outcome.is_alive(), "booting backends stay in the registry");
    assert!(
        !outcome.is_ready(),
        "booting backends must not receive tools/call"
    );
    assert!(!probe_mcp_health(&client, &mcp_url, Duration::from_secs(2)).await);
    let _ = stop.send(());
}

#[tokio::test]
async fn probe_mcp_readiness_falls_back_to_health_when_readyz_missing() {
    let app = axum::Router::new().route(
        "/health",
        axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
    );
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    assert!(
        probe_readiness(&client, &mcp_url, Duration::from_secs(2))
            .await
            .is_none(),
        "no /v1/readyz -> probe_readiness returns None"
    );
    assert_eq!(
        probe_mcp_readiness(&client, &mcp_url, Duration::from_secs(2)).await,
        ProbeOutcome::Ready
    );
    assert!(probe_mcp_health(&client, &mcp_url, Duration::from_secs(2)).await);
    let _ = stop.send(());
}

#[tokio::test]
async fn probe_mcp_readiness_falls_back_to_sidecar_healthz() {
    let app = axum::Router::new().route("/healthz", axum::routing::get(|| async { "ok" }));
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    assert_eq!(
        probe_mcp_readiness(&client, &mcp_url, Duration::from_secs(2)).await,
        ProbeOutcome::Ready
    );
    let _ = stop.send(());
}

#[tokio::test]
async fn probe_mcp_readiness_returns_unreachable_when_nothing_answers() {
    let app = axum::Router::new();
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    assert_eq!(
        probe_mcp_readiness(&client, &mcp_url, Duration::from_secs(2)).await,
        ProbeOutcome::Unreachable
    );
    assert!(!probe_mcp_health(&client, &mcp_url, Duration::from_secs(2)).await);
    let _ = stop.send(());
}

#[tokio::test]
async fn call_backend_refuses_forward_while_backend_is_booting() {
    let hit = Arc::new(AtomicBool::new(false));
    let hit_clone = hit.clone();
    let app = axum::Router::new()
        .route(
            "/v1/readyz",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    axum::Json(json!({
                        "process": true,
                        "dispatcher": false,
                        "dcc": false,
                    })),
                )
            }),
        )
        .route(
            "/mcp",
            axum::routing::post(move || {
                let hit = hit_clone.clone();
                async move {
                    hit.store(true, Ordering::SeqCst);
                    axum::Json(json!({"jsonrpc":"2.0","id":"gw-x","result":{}}))
                }
            }),
        );
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    let err = call_backend(
        &client,
        &mcp_url,
        "tools/list",
        None,
        None,
        Duration::from_secs(2),
    )
    .await
    .expect_err("booting backend must surface an error");
    assert!(
        err.contains("backend not ready") && err.contains("/v1/readyz"),
        "expected booting diagnostic, got: {err}"
    );
    assert!(
        !hit.load(Ordering::SeqCst),
        "call_backend must not post to /mcp while backend is red"
    );
    let _ = stop.send(());
}

// ── #732 / #818 integration tests: REST resource/prompt helpers ──────

#[tokio::test]
async fn try_fetch_resources_returns_backend_resources() {
    let app = rest_backend_router();
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    let resources = try_fetch_resources(&client, &mcp_url, Duration::from_secs(2))
        .await
        .expect("GET /v1/resources must succeed");
    assert_eq!(resources.len(), 2);
    assert_eq!(resources[0]["uri"], json!("scene://current"));
    assert_eq!(resources[1]["mimeType"], json!("image/png"));
    let _ = stop.send(());
}

#[tokio::test]
async fn fetch_resources_returns_empty_on_error() {
    let app = axum::Router::new().route(
        "/health",
        axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
    );
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    let resources = fetch_resources(&client, &mcp_url, Duration::from_secs(2)).await;
    assert!(
        resources.is_empty(),
        "fetch_resources must fail-soft to an empty vector"
    );
    let _ = stop.send(());
}

#[tokio::test]
async fn read_resource_preserves_blob_bytes() {
    const BLOB_B64: &str = "aGVsbG8sIHdvcmxkIQ==";
    let app = rest_backend_router();
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    let result = read_resource(
        &client,
        &mcp_url,
        "capture://current_window",
        Duration::from_secs(2),
    )
    .await
    .expect("GET /v1/resources/{uri} must succeed");
    let content = &result["contents"][0];
    assert_eq!(content["mimeType"], json!("image/png"));
    assert_eq!(content["blob"], json!(BLOB_B64));
    let _ = stop.send(());
}

#[tokio::test]
async fn forward_tools_call_propagates_trace_context_headers() {
    let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<(
        String,
        String,
        String,
        String,
    )>::new()));
    let seen_clone = seen.clone();
    let app = axum::Router::new().route(
        "/v1/call",
        axum::routing::post(move |headers: axum::http::HeaderMap| {
            let seen = seen_clone.clone();
            async move {
                let header = |name| {
                    headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string()
                };
                seen.lock().push((
                    header("x-request-id"),
                    header("x-dcc-mcp-parent-request-id"),
                    header("traceparent"),
                    header("tracestate"),
                ));
                axum::Json(json!({"success": true}))
            }
        }),
    );
    let (mcp_url, stop) = spawn_fake_backend(app).await;
    let trace_context = crate::gateway::admin::trace::TraceContext {
        trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
        request_id: "req-forward".into(),
        span_id: Some("00f067aa0ba902b7".into()),
        parent_span_id: None,
        parent_request_id: Some("req-parent".into()),
        trace_flags: Some("01".into()),
        trace_state: Some("vendor=value".into()),
    };

    forward_tools_call(
        &reqwest::Client::new(),
        &mcp_url,
        ForwardToolsCallRequest {
            tool_name: "maya.render",
            arguments: Some(json!({})),
            meta: None,
            request_id: None,
            trace_context: Some(&trace_context),
            traffic_capture: None,
            timeout: Duration::from_secs(2),
        },
    )
    .await
    .expect("forward should succeed");

    let seen = seen.lock();
    assert_eq!(seen[0].0, "req-forward");
    assert_eq!(seen[0].1, "req-parent");
    assert_eq!(
        seen[0].2,
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
    );
    assert_eq!(seen[0].3, "vendor=value");
    let _ = stop.send(());
}

#[tokio::test]
async fn subscribe_resource_forwards_subscribe_and_unsubscribe_methods() {
    let hits = Arc::new(parking_lot::Mutex::new(
        Vec::<(String, Option<String>)>::new(),
    ));
    let hits_clone = hits.clone();
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        )
        .route(
            "/mcp",
            axum::routing::post(
                move |headers: axum::http::HeaderMap, body: axum::Json<Value>| {
                    let hits = hits_clone.clone();
                    async move {
                        let method = body
                            .get("method")
                            .and_then(|m| m.as_str())
                            .unwrap_or("")
                            .to_owned();
                        let session = headers
                            .get("mcp-session-id")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_owned);
                        hits.lock().push((method, session));
                        axum::Json(json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or(json!("gw-test")),
                            "result": {}
                        }))
                    }
                },
            ),
        );
    let (mcp_url, stop) = spawn_fake_backend(app).await;

    let client = reqwest::Client::new();
    subscribe_resource(
        &client,
        &mcp_url,
        "scene://current",
        true,
        "gw-sub-abc123",
        Duration::from_secs(2),
    )
    .await
    .expect("subscribe must succeed");
    subscribe_resource(
        &client,
        &mcp_url,
        "scene://current",
        false,
        "gw-sub-abc123",
        Duration::from_secs(2),
    )
    .await
    .expect("unsubscribe must succeed");

    let recorded = hits.lock().clone();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].0, "resources/subscribe");
    assert_eq!(recorded[1].0, "resources/unsubscribe");
    assert_eq!(
        recorded[0].1.as_deref(),
        Some("gw-sub-abc123"),
        "Mcp-Session-Id must be forwarded on subscribe",
    );
    assert_eq!(
        recorded[1].1.as_deref(),
        Some("gw-sub-abc123"),
        "Mcp-Session-Id must be forwarded on unsubscribe",
    );
    let _ = stop.send(());
}

#[test]
fn rest_base_from_mcp_url_strips_mcp_suffix() {
    assert_eq!(
        rest_base_from_mcp_url("http://127.0.0.1:64954/mcp"),
        "http://127.0.0.1:64954"
    );
    assert_eq!(
        rest_base_from_mcp_url("http://127.0.0.1:64954/mcp/"),
        "http://127.0.0.1:64954"
    );
    assert_eq!(
        rest_base_from_mcp_url("http://127.0.0.1:64954"),
        "http://127.0.0.1:64954"
    );
}

#[test]
fn percent_encode_uri_encodes_colons_and_slashes() {
    use super::http::percent_encode_uri;
    let encoded = percent_encode_uri("capture://current_window");
    assert!(!encoded.contains(':'), "colon must be encoded");
    assert!(!encoded.contains('/'), "slash must be encoded");
    assert!(encoded.contains('%'), "must have percent-encoded chars");
}
