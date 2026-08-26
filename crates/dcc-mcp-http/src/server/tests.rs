use super::*;
use axum::body::Body;
use http::{Method, Request, Response, StatusCode};
use parking_lot::Mutex;
use std::{
    io::{self, Write},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use tower::ServiceExt;
use tower_http::classify::{ClassifiedResponse, ClassifyResponse, MakeClassifier};
use tracing_subscriber::fmt::MakeWriter;

#[test]
fn health_payload_surfaces_job_persistence_state() {
    let payload = health_payload(&crate::job::JobManager::new());

    assert_eq!(payload["ok"], true);
    assert_eq!(payload["job_persistence"]["state"], "not_configured");
    assert_eq!(payload["job_persistence"]["consecutive_failures"], 0);
    assert!(payload["job_persistence"]["last_error_kind"].is_null());
}

struct HttpBlockingStorage {
    entered: mpsc::SyncSender<()>,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
}

impl std::fmt::Debug for HttpBlockingStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpBlockingStorage")
            .finish_non_exhaustive()
    }
}

struct HttpCleanupBlockingStorage {
    entered: mpsc::SyncSender<()>,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
}

impl std::fmt::Debug for HttpCleanupBlockingStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpCleanupBlockingStorage")
            .finish_non_exhaustive()
    }
}

impl crate::JobStorage for HttpCleanupBlockingStorage {
    fn put(&self, _job: &crate::Job) -> Result<(), crate::JobStorageError> {
        Ok(())
    }

    fn get(&self, _job_id: &str) -> Result<Option<crate::Job>, crate::JobStorageError> {
        Ok(None)
    }

    fn list(&self, _filter: crate::JobFilter) -> Result<Vec<crate::Job>, crate::JobStorageError> {
        Ok(Vec::new())
    }

    fn update_status(
        &self,
        _job_id: &str,
        _status: crate::JobStatus,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), crate::JobStorageError> {
        Ok(())
    }

    fn delete_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, crate::JobStorageError> {
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        Ok(1)
    }
}

impl crate::JobStorage for HttpBlockingStorage {
    fn put(&self, _job: &crate::Job) -> Result<(), crate::JobStorageError> {
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        Ok(())
    }

    fn get(&self, _job_id: &str) -> Result<Option<crate::Job>, crate::JobStorageError> {
        Ok(None)
    }

    fn list(&self, _filter: crate::JobFilter) -> Result<Vec<crate::Job>, crate::JobStorageError> {
        Ok(Vec::new())
    }

    fn update_status(
        &self,
        _job_id: &str,
        _status: crate::JobStatus,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), crate::JobStorageError> {
        Ok(())
    }

    fn delete_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, crate::JobStorageError> {
        Ok(0)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dedicated_health_remains_responsive_while_job_storage_is_blocked() {
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let jobs = Arc::new(crate::JobManager::with_offloaded_storage(Arc::new(
        HttpBlockingStorage {
            entered: entered_tx,
            release: std::sync::Mutex::new(release_rx),
        },
    )));
    let writer_jobs = jobs.clone();
    let health_jobs = jobs.clone();
    let router = Router::new()
        .route(
            "/jobs",
            routing::post(move || {
                let jobs = writer_jobs.clone();
                async move {
                    jobs.create("scene.inspect");
                    StatusCode::NO_CONTENT
                }
            }),
        )
        .route(
            "/health",
            routing::get(move || {
                let jobs = health_jobs.clone();
                async move { Json(health_payload(&jobs)) }
            }),
        );

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let actual_bind = listener.local_addr().unwrap().to_string();
    let port = listener.local_addr().unwrap().port();
    let mut config = McpHttpConfig::default();
    config.server.spawn_mode = crate::ServerSpawnMode::Dedicated;
    config.server.self_probe_timeout_ms = 0;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (join, serve_thread) = spawn_impl::spawn_http_server(
        listener,
        router,
        &config,
        actual_bind,
        port,
        shutdown_tx.clone(),
        shutdown_rx,
    )
    .await
    .unwrap();
    assert!(join.is_none());

    let client = reqwest::Client::new();
    let write_client = client.clone();
    let write_request = tokio::spawn(async move {
        write_client
            .post(format!("http://127.0.0.1:{port}/jobs"))
            .send()
            .await
    });
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(2)))
        .await
        .unwrap()
        .expect("storage write did not start");
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        release_tx.send(()).unwrap();
    });

    let health_started = Instant::now();
    let health_response = tokio::time::timeout(
        Duration::from_millis(100),
        client.get(format!("http://127.0.0.1:{port}/health")).send(),
    )
    .await;
    let health_elapsed = health_started.elapsed();

    let write_response = tokio::time::timeout(Duration::from_secs(2), write_request)
        .await
        .expect("job request did not finish after storage release")
        .unwrap()
        .unwrap();
    assert_eq!(write_response.status(), StatusCode::NO_CONTENT);
    tokio::task::spawn_blocking(move || release.join().unwrap())
        .await
        .unwrap();
    let _ = shutdown_tx.send(true);
    if let Some(thread) = serve_thread {
        tokio::task::spawn_blocking(move || thread.join().unwrap())
            .await
            .unwrap();
    }

    let health_response = health_response.unwrap_or_else(|_| {
        panic!("/health was starved for {health_elapsed:?} by synchronous storage I/O")
    });
    assert_eq!(health_response.unwrap().status(), StatusCode::OK);
    assert!(
        health_elapsed < Duration::from_millis(100),
        "/health took {health_elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dedicated_health_remains_responsive_while_job_cleanup_is_blocked() {
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let jobs = Arc::new(crate::JobManager::with_offloaded_storage(Arc::new(
        HttpCleanupBlockingStorage {
            entered: entered_tx,
            release: std::sync::Mutex::new(release_rx),
        },
    )));
    let old_job = jobs.create("scene.inspect.cleanup");
    let old_job_id = old_job.read().id.clone();
    jobs.start(&old_job_id).unwrap();
    jobs.complete(&old_job_id, serde_json::json!({"ok": true}))
        .unwrap();
    old_job.write().updated_at = chrono::Utc::now() - chrono::Duration::hours(2);

    let cleanup_jobs = jobs.clone();
    let health_jobs = jobs.clone();
    let router = Router::new()
        .route(
            "/cleanup",
            routing::post(move || {
                let jobs = cleanup_jobs.clone();
                async move {
                    Json(serde_json::json!({
                        "removed": jobs.cleanup_older_than_hours(1),
                    }))
                }
            }),
        )
        .route(
            "/health",
            routing::get(move || {
                let jobs = health_jobs.clone();
                async move { Json(health_payload(&jobs)) }
            }),
        );

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let actual_bind = listener.local_addr().unwrap().to_string();
    let port = listener.local_addr().unwrap().port();
    let mut config = McpHttpConfig::default();
    config.server.spawn_mode = crate::ServerSpawnMode::Dedicated;
    config.server.self_probe_timeout_ms = 0;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (join, serve_thread) = spawn_impl::spawn_http_server(
        listener,
        router,
        &config,
        actual_bind,
        port,
        shutdown_tx.clone(),
        shutdown_rx,
    )
    .await
    .unwrap();
    assert!(join.is_none());

    let client = reqwest::Client::new();
    let cleanup_client = client.clone();
    let cleanup_request = tokio::spawn(async move {
        cleanup_client
            .post(format!("http://127.0.0.1:{port}/cleanup"))
            .send()
            .await
    });
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(2)))
        .await
        .unwrap()
        .expect("storage cleanup did not start");
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        release_tx.send(()).unwrap();
    });

    let health_started = Instant::now();
    let health_response = tokio::time::timeout(
        Duration::from_millis(100),
        client.get(format!("http://127.0.0.1:{port}/health")).send(),
    )
    .await;
    let health_elapsed = health_started.elapsed();

    let cleanup_response = tokio::time::timeout(Duration::from_secs(2), cleanup_request)
        .await
        .expect("cleanup request did not finish after storage release")
        .unwrap()
        .unwrap();
    assert_eq!(cleanup_response.status(), StatusCode::OK);
    tokio::task::spawn_blocking(move || release.join().unwrap())
        .await
        .unwrap();
    let _ = shutdown_tx.send(true);
    if let Some(thread) = serve_thread {
        tokio::task::spawn_blocking(move || thread.join().unwrap())
            .await
            .unwrap();
    }

    let health_response = health_response.unwrap_or_else(|_| {
        panic!("/health was starved for {health_elapsed:?} by synchronous cleanup storage I/O")
    });
    assert_eq!(health_response.unwrap().status(), StatusCode::OK);
    assert!(
        health_elapsed < Duration::from_millis(100),
        "/health took {health_elapsed:?}"
    );
}

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
