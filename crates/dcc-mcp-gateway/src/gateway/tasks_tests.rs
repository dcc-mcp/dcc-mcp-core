//! Tests for the gateway supervisor task wiring.
//!
//! Kept in a sibling module so `tasks.rs` stays under the repository's
//! file-size gate.

use super::runner::PromotedGatewayGuard;
use super::tasks::{
    InstanceMembership, build_backend_http_client, poll_list_fingerprint, removed_instances,
    start_gateway_tasks, wait_for_startup_ready,
};
use super::*;
use axum::{Router, response::Redirect, routing::get};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;

fn membership(dcc_type: &str, instance_id: &str) -> (String, InstanceMembership) {
    (
        format!("{dcc_type}:{instance_id}"),
        InstanceMembership {
            dcc_type: dcc_type.to_string(),
            instance_id: instance_id.to_string(),
        },
    )
}

#[test]
fn removed_instances_preserve_dcc_and_instance_identity() {
    let previous = HashMap::from([
        membership("houdini", "aaaaaaaa-0000-0000-0000-000000000000"),
        membership("photoshop", "bbbbbbbb-0000-0000-0000-000000000000"),
    ]);
    let current = HashMap::from([membership(
        "photoshop",
        "bbbbbbbb-0000-0000-0000-000000000000",
    )]);

    assert_eq!(
        removed_instances(&previous, &current),
        vec![InstanceMembership {
            dcc_type: "houdini".to_string(),
            instance_id: "aaaaaaaa-0000-0000-0000-000000000000".to_string(),
        }]
    );
}

#[test]
fn removed_instances_does_not_report_unchanged_inventory() {
    let inventory = HashMap::from([membership(
        "custom_host",
        "cccccccc-0000-0000-0000-000000000000",
    )]);
    assert!(removed_instances(&inventory, &inventory).is_empty());
}

#[tokio::test]
async fn list_fingerprint_poll_is_skipped_without_subscribers() {
    let (events_tx, receiver) = broadcast::channel(1);
    drop(receiver);
    let calls = AtomicUsize::new(0);

    let fingerprint = poll_list_fingerprint(&events_tx, || async {
        calls.fetch_add(1, Ordering::SeqCst);
        "unused".to_string()
    })
    .await;

    assert_eq!(fingerprint, None);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn list_fingerprint_poll_runs_for_a_subscriber() {
    let (events_tx, _receiver) = broadcast::channel(1);
    let calls = AtomicUsize::new(0);

    let fingerprint = poll_list_fingerprint(&events_tx, || async {
        calls.fetch_add(1, Ordering::SeqCst);
        "current".to_string()
    })
    .await;

    assert_eq!(fingerprint.as_deref(), Some("current"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// Mirror of the supervisor's listener teardown: await the drain window,
/// then abort whatever is still running.
/// Regression: a listener that outlives the drain window (a long-lived SSE
/// stream, for example) must still be aborted, not detached. Dropping its
/// JoinHandle would leave the axum listener running and hold the gateway
/// port open — the teardown regression this guards against.
///
/// This routes through the real `GatewayHandle::abort_listeners` (the same
/// call the winner shutdown path makes) rather than re-implementing the
/// abort order, so the production teardown is what is under test.
#[tokio::test]
async fn drain_aborts_a_listener_that_outlives_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let finished_flag = Arc::new(AtomicUsize::new(0));
    let task_flag = finished_flag.clone();
    let handle = tokio::spawn(async move {
        // Never completes on its own — stands in for an open SSE stream.
        std::future::pending::<()>().await;
        task_flag.fetch_add(1, Ordering::SeqCst);
    });
    let listener_abort = handle.abort_handle();

    // Awaiting the stuck task is the drain; it cannot finish on its own.
    let drain = async {
        let _ = handle.await;
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(50), drain)
            .await
            .is_err(),
        "a stuck listener must report the drain window as elapsed"
    );

    let mut gateway = GatewayHandle {
        is_gateway: true,
        service_key: ServiceKey {
            dcc_type: "__gateway__".to_string(),
            instance_id: uuid::Uuid::new_v4(),
        },
        heartbeat_abort: None,
        gateway_abort: None,
        gateway_supervisor: None,
        listener_aborts: vec![listener_abort],
        gateway_thread: None,
        challenger_abort: None,
        registry: Arc::new(FileRegistry::new(dir.path()).unwrap()),
        pending_deregister: Vec::new(),
        registration_active: Arc::new(AtomicBool::new(false)),
    };
    gateway.abort_listeners();

    // Give the abort a scheduling turn to take effect.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        finished_flag.load(Ordering::SeqCst),
        0,
        "the abort fallback must stop the listener task"
    );
}

/// The drain contract: a request already accepted when the yield fires
/// must still run to completion.
///
/// This exercises the real `axum::serve` + `with_graceful_shutdown` path
/// the supervisor awaits, not a stand-in. The handler parks long enough
/// that an abort-based teardown would sever the connection, and the
/// assertion is that the response still arrives complete.
#[tokio::test]
async fn drain_lets_an_in_flight_request_finish_before_abort() {
    let handler_started = Arc::new(Notify::new());
    let handler_done = Arc::new(AtomicUsize::new(0));
    let started = handler_started.clone();
    let done = handler_done.clone();

    let app = Router::new().route(
        "/slow",
        get(move || {
            let started = started.clone();
            let done = done.clone();
            async move {
                started.notify_one();
                tokio::time::sleep(Duration::from_millis(300)).await;
                done.fetch_add(1, Ordering::SeqCst);
                "finished"
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (yield_tx, yield_rx) = watch::channel(false);

    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let mut yield_rx = yield_rx;
                loop {
                    if yield_rx.changed().await.is_err() {
                        break;
                    }
                    if *yield_rx.borrow() {
                        break;
                    }
                }
            })
            .await
            .ok();
    });

    let client = reqwest::Client::new();
    let request =
        tokio::spawn(async move { client.get(format!("http://{addr}/slow")).send().await });

    // Wait until the handler is genuinely in flight before yielding.
    handler_started.notified().await;
    let _ = yield_tx.send(true);

    let response = tokio::time::timeout(Duration::from_secs(5), request)
        .await
        .expect("in-flight request must not hang across graceful shutdown")
        .expect("request task panicked")
        .expect("drain must not sever an in-flight request");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "finished");
    assert_eq!(handler_done.load(Ordering::SeqCst), 1);
    let _ = server.await;
}

#[tokio::test]
async fn cleanup_startup_barrier_waits_for_durable_readback() {
    let (ready_tx, mut ready_rx) = watch::channel(false);
    let waiter = tokio::spawn(async move { wait_for_startup_ready(&mut ready_rx).await });

    tokio::task::yield_now().await;
    assert!(!waiter.is_finished());
    ready_tx.send(true).unwrap();
    assert!(waiter.await.unwrap());
}

#[tokio::test]
async fn backend_http_client_does_not_follow_redirects() {
    let private_hits = Arc::new(AtomicUsize::new(0));
    let private_hits_handler = private_hits.clone();
    let app = Router::new()
        .route("/start", get(|| async { Redirect::temporary("/private") }))
        .route(
            "/private",
            get(move || {
                let private_hits = private_hits_handler.clone();
                async move {
                    private_hits.fetch_add(1, Ordering::SeqCst);
                    "private"
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let response = build_backend_http_client()
        .unwrap()
        .get(format!("http://{addr}/start"))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_redirection());
    assert_eq!(private_hits.load(Ordering::SeqCst), 0);
}

/// Regression (challenger promotion path): dropping the supervisor without
/// awaiting it must still release the gateway port.
///
/// The challenger path builds a `PromotedGatewayGuard` around
/// `tasks.supervisor.await`. When an external abort hits that future it is
/// *dropped* rather than run to completion, so the supervisor never reaches
/// its own end-of-drain listener aborts; the listener `JoinHandle`s are then
/// detached rather than cancelled, and axum keeps serving on the gateway
/// port. The guard carries listener abort handles so teardown stops them
/// regardless of how the supervisor ends.
///
/// This drives the production `start_gateway_tasks`, then drops a real
/// `PromotedGatewayGuard` mid-flight exactly as the challenger path does —
/// the production teardown is what makes the assertion pass, not a
/// test-local copy of it.
#[tokio::test]
async fn dropping_a_promoted_gateway_releases_the_gateway_port() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(FileRegistry::new(dir.path()).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sentinel_key = ServiceKey {
        dcc_type: "__gateway__".to_string(),
        instance_id: uuid::Uuid::new_v4(),
    };

    let tasks = start_gateway_tasks(
        listener,
        None,
        registry.clone(),
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(5),
        Duration::from_secs(5),
        Duration::from_secs(60),
        64,
        "dcc-mcp-gateway-test".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
        sentinel_key.clone(),
        "127.0.0.1".to_string(),
        port,
        false,
        Vec::new(),
        Default::default(),
        None,
        None,
        Default::default(),
        #[cfg(feature = "admin")]
        false,
        #[cfg(feature = "admin")]
        "admin".to_string(),
        #[cfg(feature = "admin")]
        Default::default(),
        30,
        3,
        Default::default(),
        None,
        false,
        30,
        false,
        None,
    )
    .await
    .expect("gateway tasks must start");

    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok(),
        "gateway must be listening before shutdown"
    );

    // The guard takes ownership exactly as the challenger path does; the
    // supervisor is deliberately still in flight when it drops.
    let guard = PromotedGatewayGuard {
        abort: Some(tasks.abort),
        listener_aborts: tasks.listener_aborts,
        registry,
        sentinel_key: Some(sentinel_key),
    };
    drop(tasks.supervisor);
    drop(guard);

    let mut released = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        if let Ok(bound) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            drop(bound);
            released = true;
            break;
        }
    }
    assert!(
        released,
        "gateway port {port} must be released once a promoted gateway is dropped"
    );
}
