//! Controlled protocol fixtures; these do not connect to a DCC or native UI runtime.
use std::process::Command;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

#[derive(Clone, Copy)]
enum Case {
    Complete,
    Repeat,
    WrongId,
    WrongVersion,
    RpcError,
    MissingSchema,
    WrongSchemaType,
    MissingSchemaType,
    Duplicate,
    InvalidCursor,
    Timeout,
    InitWrongId,
    InitWrongVersion,
    InitMissingResult,
    InitMissingIdentity,
    InitEmptyProtocol,
    InitUnsupportedProtocol,
    InitLegacyProtocol,
    HttpError,
    MalformedBody,
    InvalidUtf8,
}

#[derive(Clone)]
struct StateData {
    case: Case,
    requests: Arc<Mutex<Vec<Value>>>,
}

struct Fixture {
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

async fn mcp(
    State(state): State<StateData>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let accept = headers.get("accept").unwrap().to_str().unwrap();
    assert!(accept.contains("application/json") && accept.contains("text/event-stream"));
    let id = body["id"].clone();
    match body["method"].as_str().unwrap() {
        "initialize" => {
            let mut response = json!({"jsonrpc":"2.0", "id":id, "result":{
                "protocolVersion":"2025-06-18", "capabilities":{"tools":{}},
                "serverInfo":{"name":"controlled-smoke-fixture", "version":"test"}
            }});
            match state.case {
                Case::InitWrongId => response["id"] = json!("stale-initialize"),
                Case::InitWrongVersion => response["jsonrpc"] = json!("1.0"),
                Case::InitMissingResult => {
                    response.as_object_mut().unwrap().remove("result");
                }
                Case::InitMissingIdentity => {
                    response["result"]
                        .as_object_mut()
                        .unwrap()
                        .remove("serverInfo");
                }
                Case::InitEmptyProtocol => response["result"]["protocolVersion"] = json!(""),
                Case::InitUnsupportedProtocol => {
                    response["result"]["protocolVersion"] = json!("unsupported")
                }
                Case::InitLegacyProtocol => {
                    response["result"]["protocolVersion"] = json!("2025-03-26")
                }
                _ => {}
            }
            Json(response).into_response()
        }
        "tools/list" => {
            let expected_protocol = if matches!(state.case, Case::InitLegacyProtocol) {
                "2025-03-26"
            } else {
                dcc_mcp_jsonrpc::MCP_PROTOCOL_VERSION
            };
            assert_eq!(
                headers
                    .get("Mcp-Protocol-Version")
                    .unwrap()
                    .to_str()
                    .unwrap(),
                expected_protocol
            );
            let page = {
                let mut requests = state.requests.lock().unwrap();
                requests.push(body);
                requests.len()
            };
            if page == 2 {
                match state.case {
                    Case::MalformedBody => {
                        return ([(header::CONTENT_TYPE, "application/json")], "{ not json")
                            .into_response();
                    }
                    Case::InvalidUtf8 => {
                        return ([(header::CONTENT_TYPE, "application/json")], vec![0xff_u8])
                            .into_response();
                    }
                    _ => {}
                }
            }
            let mut response = json!({"jsonrpc":"2.0", "id":id, "result":{
                "tools":[{"name": if page == 1 { "fixture__first" } else { "fixture__second" },
                          "description":"Controlled fixture descriptor",
                          "inputSchema":{"type":"object", "properties":{"label":{"type":"string"}}},
                          "_meta":{"fixture":{"preserve":true}}}]
            }});
            if page == 1 {
                response["result"]["nextCursor"] = json!("3332");
            }
            match state.case {
                Case::Complete => {}
                Case::Repeat => response["result"]["nextCursor"] = json!("3332"),
                Case::WrongId => response["id"] = json!("stale-request"),
                Case::WrongVersion => response["jsonrpc"] = json!("1.0"),
                Case::RpcError => {
                    response = json!({"jsonrpc":"2.0", "id":id,
                    "error":{"code":-32603,"message":"controlled refusal"}})
                }
                Case::MissingSchema => {
                    response["result"]["tools"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("inputSchema");
                }
                Case::WrongSchemaType => {
                    response["result"]["tools"][0]["inputSchema"] = json!({"type":"string"})
                }
                Case::MissingSchemaType => {
                    response["result"]["tools"][0]["inputSchema"] = json!({})
                }
                Case::Duplicate => response["result"]["tools"][0]["name"] = json!("fixture__first"),
                Case::InvalidCursor => response["result"]["nextCursor"] = json!(3332),
                Case::Timeout if page == 2 => {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await
                }
                Case::Timeout => {}
                Case::HttpError | Case::MalformedBody | Case::InvalidUtf8 => {}
                Case::InitLegacyProtocol => {}
                Case::InitWrongId
                | Case::InitWrongVersion
                | Case::InitMissingResult
                | Case::InitMissingIdentity => {
                    panic!("tools/list must not follow an invalid initialize");
                }
                Case::InitEmptyProtocol | Case::InitUnsupportedProtocol => {
                    panic!("unsupported protocol must not list tools")
                }
            }
            let status = if page == 2 && matches!(state.case, Case::HttpError) {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::OK
            };
            (
                status,
                [(header::CONTENT_TYPE, "application/json")],
                serde_json::to_string_pretty(&response).unwrap(),
            )
                .into_response()
        }
        other => panic!("Unexpected protocol method: {other}"),
    }
}

fn fixture(case: Case) -> Fixture {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"ok":true})) }))
        .route("/v1/search", post(|| async { Json(json!({"hits":[]})) }))
        .route("/mcp", post(mcp))
        .with_state(StateData {
            case,
            requests: requests.clone(),
        });
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (stop, stopped) = oneshot::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app)
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await
                    .unwrap();
            });
    });
    Fixture {
        url: format!("http://{address}/mcp"),
        requests,
        stop: Some(stop),
        thread: Some(thread),
    }
}

fn smoke(fixture: &Fixture, max_pages: &str) -> (bool, Value) {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
        .env("DCC_MCP_DISABLE_UPDATE_CHECK", "true")
        .env("DCC_MCP_INSTALL_OFFLINE", "true")
        .env("DCC_MCP_INSTALL_CACHE", temp.path().join("catalog.json"))
        .env("DCC_MCP_REGISTRY_DIR", temp.path().join("registry"))
        .env_remove("DCC_MCP_TIMEOUT_SECS")
        .env_remove("DCC_MCP_DCC_TYPE")
        .env_remove("DCC_MCP_INSTANCE_ID")
        .args([
            "--no-auto-gateway",
            "--gateway",
            "local",
            "--output",
            "json",
            "smoke",
            "--url",
            &fixture.url,
            "--timeout-secs",
            "2",
            "--max-pages",
            max_pages,
        ])
        .output()
        .unwrap();
    let result = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "Invalid CLI JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), result)
}

fn listing(result: &Value) -> &Value {
    result["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "mcp_tools_list")
        .unwrap()
}

#[test]
fn smoke_follows_terminal_cursor_and_preserves_each_formal_page() {
    let fixture = fixture(Case::Complete);
    let (success, result) = smoke(&fixture, "16");
    assert!(success, "{result}");
    let check = listing(&result);
    assert_eq!(check["ok"], true);
    assert_eq!(check["complete"], true);
    assert_eq!(check["tool_count"], 2);
    let pages = check["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0]["request"]["params"], json!({}));
    assert_eq!(pages[1]["request"]["params"], json!({"cursor":"3332"}));
    assert_eq!(pages[0]["response"], check["response"]);
    assert_eq!(pages[0]["response"]["result"]["nextCursor"], "3332");
    assert_eq!(pages[1]["response"]["id"], "smoke-tools-list-2");
    assert_eq!(
        pages[1]["response"]["result"]["tools"][0]["_meta"]["fixture"]["preserve"],
        true
    );
    assert!(pages[1]["response"]["result"].get("nextCursor").is_none());
    for page in pages {
        let body = page["raw_body_utf8"].as_str().unwrap();
        assert!(body.starts_with("{\n"));
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap(),
            page["response"]
        );
        assert_eq!(
            page["raw_body_sha256"],
            hex::encode(Sha256::digest(body.as_bytes()))
        );
        assert_eq!(page["http_status"], 200);
    }
    assert_eq!(
        *fixture.requests.lock().unwrap(),
        pages
            .iter()
            .map(|page| page["request"].clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn smoke_refuses_incomplete_inventory_at_page_bound() {
    let fixture = fixture(Case::Complete);
    let (success, result) = smoke(&fixture, "1");
    assert!(!success);
    let check = listing(&result);
    assert_eq!(check["complete"], false);
    assert_eq!(check["pages"].as_array().unwrap().len(), 1);
    assert!(check["error"].as_str().unwrap().contains("page limit 1"));
    assert_eq!(fixture.requests.lock().unwrap().len(), 1);
}

#[test]
fn smoke_refuses_repeated_cursor_without_retry() {
    let fixture = fixture(Case::Repeat);
    let (success, result) = smoke(&fixture, "16");
    assert!(!success);
    assert!(
        listing(&result)["error"]
            .as_str()
            .unwrap()
            .contains("repeated")
    );
    assert_eq!(fixture.requests.lock().unwrap().len(), 2);
}

#[test]
fn smoke_refuses_protocol_and_descriptor_errors_and_retains_failed_page() {
    for (case, expected, pages) in [
        (Case::WrongId, "transport desync", 1),
        (Case::WrongVersion, "JSON-RPC version", 1),
        (Case::RpcError, "controlled refusal", 1),
        (Case::MissingSchema, "inputSchema", 1),
        (Case::WrongSchemaType, "inputSchema", 1),
        (Case::MissingSchemaType, "inputSchema", 1),
        (Case::Duplicate, "duplicate", 2),
        (Case::InvalidCursor, "nextCursor", 1),
    ] {
        let fixture = fixture(case);
        let (success, result) = smoke(&fixture, "16");
        assert!(!success, "{result}");
        let check = listing(&result);
        assert_eq!(check["complete"], false);
        assert!(
            check["error"].as_str().unwrap().contains(expected),
            "{check}"
        );
        assert_eq!(check["pages"].as_array().unwrap().len(), pages);
        assert!(check["pages"][pages - 1].get("response").is_some());
        assert_eq!(fixture.requests.lock().unwrap().len(), pages);
    }
}

#[test]
fn smoke_retains_first_page_and_failed_request_on_transport_timeout_without_retry() {
    let fixture = fixture(Case::Timeout);
    let (success, result) = smoke(&fixture, "16");
    assert!(!success);
    let check = listing(&result);
    assert_eq!(check["complete"], false);
    assert_eq!(check["pages"].as_array().unwrap().len(), 2);
    assert!(check["pages"][0].get("response").is_some());
    assert_eq!(
        check["pages"][1]["request"]["params"],
        json!({"cursor":"3332"})
    );
    assert!(check["pages"][1].get("response").is_none());
    assert!(check["pages"][1]["error"].as_str().is_some());
    assert_eq!(fixture.requests.lock().unwrap().len(), 2);
}

#[test]
fn smoke_captures_http_and_malformed_body_failures_without_retry() {
    for case in [Case::HttpError, Case::MalformedBody, Case::InvalidUtf8] {
        let fixture = fixture(case);
        let (success, result) = smoke(&fixture, "16");
        assert!(!success);
        let check = listing(&result);
        assert_eq!(check["complete"], false);
        assert_eq!(check["pages"].as_array().unwrap().len(), 2);
        let failed = &check["pages"][1];
        assert_eq!(failed["request"]["params"], json!({"cursor":"3332"}));
        match case {
            Case::HttpError => {
                assert_eq!(failed["http_status"], 500);
                assert!(failed.get("response").is_some());
                assert!(failed["raw_body_utf8"].as_str().unwrap().starts_with("{\n"));
            }
            Case::MalformedBody => assert_eq!(failed["raw_body_utf8"], "{ not json"),
            Case::InvalidUtf8 => {
                assert_eq!(failed["raw_body_bytes"], json!([255]));
                assert!(failed.get("raw_body_utf8").is_none());
            }
            _ => unreachable!(),
        }
        assert_eq!(fixture.requests.lock().unwrap().len(), 2);
    }
}

#[test]
fn smoke_rejects_zero_page_limit_before_protocol_dispatch() {
    let fixture = fixture(Case::Complete);
    let output = Command::new(env!("CARGO_BIN_EXE_dcc-mcp-cli"))
        .args([
            "--no-auto-gateway",
            "smoke",
            "--url",
            &fixture.url,
            "--max-pages",
            "0",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("max-pages"));
    assert!(fixture.requests.lock().unwrap().is_empty());
}

#[test]
fn smoke_refuses_invalid_initialize_before_listing_and_preserves_response() {
    for case in [
        Case::InitWrongId,
        Case::InitWrongVersion,
        Case::InitMissingResult,
        Case::InitMissingIdentity,
        Case::InitEmptyProtocol,
        Case::InitUnsupportedProtocol,
    ] {
        let fixture = fixture(case);
        let (success, result) = smoke(&fixture, "16");
        assert!(!success);
        let checks = result["checks"].as_array().unwrap();
        let initialize = checks
            .iter()
            .find(|check| check["name"] == "mcp_initialize")
            .unwrap();
        assert_eq!(initialize["ok"], false);
        assert!(initialize.get("response").is_some());
        assert_eq!(listing(&result)["complete"], false);
        assert_eq!(listing(&result)["pages"], json!([]));
        assert!(fixture.requests.lock().unwrap().is_empty());
    }
}

#[test]
fn smoke_uses_supported_negotiated_legacy_protocol_for_every_page() {
    let fixture = fixture(Case::InitLegacyProtocol);
    let (success, result) = smoke(&fixture, "16");
    assert!(success, "{result}");
    assert_eq!(listing(&result)["complete"], true);
    assert_eq!(fixture.requests.lock().unwrap().len(), 2);
}
