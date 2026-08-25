use super::*;
use axum::body::Body;
use http::{Method, Request, Response, StatusCode};
use parking_lot::Mutex;
use std::{
    io::{self, Write},
    sync::Arc,
};
use tower::ServiceExt;
use tower_http::classify::{ClassifiedResponse, ClassifyResponse, MakeClassifier};
use tracing_subscriber::fmt::MakeWriter;

#[cfg(feature = "auto-gateway")]
fn unused_local_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn instance_id_is_none_when_gateway_registration_is_disabled() {
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = 0;

    let handle = McpHttpServer::new(Arc::new(ToolRegistry::new()), config)
        .start()
        .await
        .unwrap();

    assert_eq!(handle.instance_id, None);
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn context_endpoint_reads_live_instance_metadata() {
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = 0;
    config.instance.dcc_type = Some("unreal".to_string());

    let server = McpHttpServer::new(Arc::new(ToolRegistry::new()), config);
    server.update_live_scene(
        Some("/Game/Maps/LiveMap".to_string()),
        Some("5.8".to_string()),
        None,
        None,
    );
    let handle = server.start().await.unwrap();
    let context: serde_json::Value =
        reqwest::get(format!("http://127.0.0.1:{}/v1/context", handle.port))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

    assert_eq!(context["dcc"], "unreal");
    assert_eq!(context["scene"], "/Game/Maps/LiveMap");
    assert_eq!(context["version"], "5.8");
    handle.shutdown().await;
}

#[cfg(feature = "auto-gateway")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn instance_id_matches_the_registered_service_key() {
    use dcc_mcp_transport::discovery::file_registry::FileRegistry;

    let registry_dir = tempfile::tempdir().unwrap();
    let mut config = McpHttpConfig::default();
    config.server.port = 0;
    config.gateway.gateway_port = unused_local_port();
    config.gateway.registry_dir = Some(registry_dir.path().to_path_buf());
    config.gateway.heartbeat_secs = 0;
    config.gateway.admin_enabled = false;
    config.instance.dcc_type = Some("maya".to_string());

    let handle = McpHttpServer::new(Arc::new(ToolRegistry::new()), config)
        .start()
        .await
        .unwrap();
    let instance_id = handle
        .instance_id
        .expect("gateway registration should publish an instance UUID");

    let registry = FileRegistry::new(registry_dir.path()).unwrap();
    let entries = registry.list_instances("maya");
    assert_eq!(entries.len(), 1);
    assert_eq!(instance_id, entries[0].instance_id);

    handle.shutdown().await;
}

fn classify_http_response(method: Method, path: &str, status: StatusCode) -> bool {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(())
        .unwrap();
    let response = Response::builder().status(status).body(()).unwrap();
    let classifier = HttpTraceClassifier.make_classifier(&request);

    matches!(
        classifier.classify_response(&response),
        ClassifiedResponse::Ready(Ok(()))
    )
}

#[test]
fn readyz_not_ready_response_is_a_routine_probe_result() {
    assert!(classify_http_response(
        Method::GET,
        "/v1/readyz",
        StatusCode::SERVICE_UNAVAILABLE
    ));
}

#[test]
fn only_exact_get_readyz_without_query_is_a_routine_probe() {
    for (method, uri) in [
        (Method::POST, "/v1/readyz"),
        (Method::GET, "/v1/readyz?token=private-query-value"),
        (Method::GET, "/v1/%72eadyz"),
        (Method::GET, "/v1/readyz/"),
        (Method::GET, "/v1/call"),
    ] {
        assert!(
            !classify_http_response(method.clone(), uri, StatusCode::SERVICE_UNAVAILABLE),
            "{method} {uri} must keep the standard server-error classification"
        );
    }

    assert!(!classify_http_response(
        Method::GET,
        "/v1/readyz",
        StatusCode::INTERNAL_SERVER_ERROR,
    ));
}

#[derive(Clone, Default)]
struct TraceBuffer(Arc<Mutex<Vec<u8>>>);

struct TraceBufferWriter(TraceBuffer);

impl Write for TraceBufferWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for TraceBuffer {
    type Writer = TraceBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        TraceBufferWriter(self.clone())
    }
}

impl TraceBuffer {
    fn output(&self) -> String {
        String::from_utf8(self.0.lock().clone()).unwrap()
    }
}

async fn capture_http_trace(method: Method, uri: &str, status: StatusCode) -> String {
    let output = TraceBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(output.clone())
        .finish();
    let dispatch = tracing::Dispatch::new(subscriber);
    let _default = tracing::dispatcher::set_default(&dispatch);

    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", "Bearer private-header-value")
        .body(Body::empty())
        .unwrap();
    let service = Router::new()
        .fallback(move || async move { status })
        .layer(http_trace_layer());

    service.oneshot(request).await.unwrap();
    output.output()
}

#[tokio::test(flavor = "current_thread")]
async fn real_trace_layer_emits_errors_only_for_non_routine_failures() {
    let routine =
        capture_http_trace(Method::GET, "/v1/readyz", StatusCode::SERVICE_UNAVAILABLE).await;
    assert!(!routine.contains("ERROR"), "{routine}");

    for (method, uri, status) in [
        (Method::GET, "/v1/readyz", StatusCode::INTERNAL_SERVER_ERROR),
        (Method::GET, "/v1/call", StatusCode::SERVICE_UNAVAILABLE),
        (Method::GET, "/v1/call", StatusCode::INTERNAL_SERVER_ERROR),
        (Method::POST, "/v1/readyz", StatusCode::SERVICE_UNAVAILABLE),
        (
            Method::GET,
            "/v1/readyz?token=private-query-value",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
        let trace = capture_http_trace(method.clone(), uri, status).await;
        assert!(
            trace.contains("ERROR"),
            "missing ERROR for {method} {uri}: {trace}"
        );
    }

    for status in [StatusCode::OK, StatusCode::BAD_REQUEST] {
        let trace = capture_http_trace(Method::GET, "/v1/readyz", status).await;
        assert!(!trace.contains("ERROR"), "{trace}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn trace_events_do_not_expose_request_secrets() {
    let trace = capture_http_trace(
        Method::GET,
        "/v1/readyz?token=private-query-value",
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;

    assert!(!trace.contains("private-header-value"), "{trace}");
    assert!(!trace.contains("private-query-value"), "{trace}");
}
