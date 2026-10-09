//! Tests for the gateway supervisor task wiring.
//!
//! Kept in a sibling module so `tasks.rs` stays under the repository's
//! file-size gate.

use super::*;
use axum::{Router, response::Redirect, routing::get};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, response::Redirect, routing::get};
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    /// The drain contract: a request already accepted when the yield fires
    /// must still run to completion.
    ///
    /// This exercises the real `axum::serve` + `with_graceful_shutdown` path
    /// the supervisor awaits, not a stand-in. The handler parks long enough
    /// that an abort-based teardown would sever the connection, and the
    /// assertion is that the response still arrives complete.
    #[tokio::test]
    async fn in_flight_request_completes_during_graceful_shutdown() {
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
}
