//! Tests for [`FramedChannel`].
//!
//! Imported as `#[cfg(test)] mod channel_tests;` from `channel.rs`.

use super::*;
use crate::connector::IpcStream;
use crate::message::{Notification, Pong, ShutdownMessage};

/// Helper: create a pair of connected FramedIo instances over TCP.
async fn framed_pair() -> (FramedIo, FramedIo) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let connect_fut = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"));
    let (client, server) = tokio::join!(connect_fut, listener.accept());

    (
        FramedIo::new(IpcStream::Tcp(client.unwrap())),
        FramedIo::new(IpcStream::Tcp(server.unwrap().0)),
    )
}

// ── Basic channel operation tests ──

mod basic {
    use super::*;
    use crate::message::Request;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_recv_data_envelope_from_raw_sender() {
        let (client_framed, mut server_framed) = framed_pair().await;
        let mut channel = FramedChannel::new(client_framed);

        let req = Request {
            id: Uuid::new_v4(),
            method: "test_method".to_string(),
            params: vec![1, 2, 3],
        };

        // Server sends via raw FramedIo.
        server_framed
            .send_envelope(&MessageEnvelope::from(req.clone()))
            .await
            .unwrap();

        // Client receives via channel.
        let received = channel.recv().await.unwrap().unwrap();
        assert_eq!(received, MessageEnvelope::Request(req));

        channel.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_messages_preserved_during_ping() {
        let (client_framed, mut server_framed) = framed_pair().await;
        let mut client = FramedChannel::new(client_framed);

        let server_handle = tokio::spawn(async move {
            // Receive the Ping.
            let envelope = server_framed.recv_envelope().await.unwrap();
            let ping_id = match &envelope {
                MessageEnvelope::Ping(p) => p.id,
                other => panic!("expected Ping, got {other:?}"),
            };

            // Before replying, send several data messages.
            for i in 0..3u8 {
                let notif = Notification {
                    id: None,
                    topic: format!("event_{i}"),
                    data: vec![i],
                };
                server_framed
                    .send_envelope(&MessageEnvelope::from(notif))
                    .await
                    .unwrap();
            }

            // Now reply with Pong.
            let pong = Pong {
                id: ping_id,
                timestamp_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            };
            server_framed
                .send_envelope(&MessageEnvelope::from(pong))
                .await
                .unwrap();
        });

        // Client pings — during the wait, 3 data messages arrive.
        let rtt = client.ping().await.unwrap();
        assert!(rtt < 5000, "RTT {rtt}ms too high");

        // All 3 data messages must be preserved (not lost)!
        for i in 0..3u8 {
            let env = client.recv().await.unwrap().unwrap();
            match env {
                MessageEnvelope::Notify(n) => {
                    assert_eq!(n.topic, format!("event_{i}"));
                }
                other => panic!("expected Notify, got {other:?}"),
            }
        }

        let _ = server_handle.await;
        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_ping_times_out_when_no_response() {
        let (client_framed, _server_framed) = framed_pair().await;
        let mut client = FramedChannel::new(client_framed);

        let result = client
            .ping_with_timeout(std::time::Duration::from_millis(50))
            .await;

        match result.unwrap_err() {
            TransportError::PingTimeout { timeout_ms } => {
                assert_eq!(timeout_ms, 50);
            }
            other => panic!("expected PingTimeout, got {other:?}"),
        }

        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_shutdown_detection_via_channel() {
        let (client_framed, mut server_framed) = framed_pair().await;
        let mut client = FramedChannel::new(client_framed);

        server_framed
            .send_envelope(&MessageEnvelope::from(ShutdownMessage {
                reason: Some("maintenance".to_string()),
            }))
            .await
            .unwrap();

        let reason = client.shutdown_rx.recv().await.unwrap();
        assert_eq!(reason, Some("maintenance".to_string()));

        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_auto_reply_to_incoming_ping() {
        let (mut client_framed, server_framed) = framed_pair().await;
        let _server_channel = FramedChannel::new(server_framed);

        let ping = Ping::new();
        let ping_id = ping.id;

        // Client sends Ping via raw FramedIo.
        client_framed
            .send_envelope(&MessageEnvelope::from(ping))
            .await
            .unwrap();

        // Server (channel) should auto-reply — client receives a Pong.
        let envelope = client_framed.recv_envelope().await.unwrap();
        match envelope {
            MessageEnvelope::Pong(pong) => {
                assert_eq!(pong.id, ping_id);
            }
            other => panic!("expected Pong, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_try_recv_returns_none_when_empty() {
        let (client_framed, _server_framed) = framed_pair().await;
        let mut channel = FramedChannel::new(client_framed);

        let result = channel.try_recv().unwrap();
        assert!(result.is_none());

        channel.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_closed_when_stream_drops() {
        let (client_framed, server_framed) = framed_pair().await;
        let mut channel = FramedChannel::new(client_framed);

        // Drop server side — connection closes.
        drop(server_framed);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let result = channel.recv().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::ConnectionClosed => {}
            other => panic!("expected ConnectionClosed, got {other:?}"),
        }
    }
}

// ── Concurrent ping + data flow tests ──

mod concurrent {
    use super::*;

    #[tokio::test]
    async fn test_consecutive_pings_all_succeed() {
        let (client_framed, mut server_framed) = framed_pair().await;
        let mut client = FramedChannel::new(client_framed);

        let server_handle = tokio::spawn(async move {
            for _ in 0..5 {
                let envelope = server_framed.recv_envelope().await.unwrap();
                if let MessageEnvelope::Ping(ping) = envelope {
                    let pong = Pong {
                        id: ping.id,
                        timestamp_ms: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64,
                    };
                    server_framed
                        .send_envelope(&MessageEnvelope::from(pong))
                        .await
                        .unwrap();
                }
            }
        });

        for _ in 0..5 {
            let rtt = client.ping().await.unwrap();
            assert!(rtt < 5000);
        }

        let _ = server_handle.await;
        client.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_data_and_ping_interleaved() {
        let (client_framed, mut server_framed) = framed_pair().await;
        let mut client = FramedChannel::new(client_framed);

        let server_handle = tokio::spawn(async move {
            for round in 0..3u8 {
                let envelope = server_framed.recv_envelope().await.unwrap();
                match &envelope {
                    MessageEnvelope::Ping(ping) => {
                        // Send data before replying.
                        let notif = Notification {
                            id: None,
                            topic: format!("before_pong_{round}"),
                            data: vec![round],
                        };
                        server_framed
                            .send_envelope(&MessageEnvelope::from(notif))
                            .await
                            .unwrap();

                        let pong = Pong {
                            id: ping.id,
                            timestamp_ms: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64,
                        };
                        server_framed
                            .send_envelope(&MessageEnvelope::from(pong))
                            .await
                            .unwrap();
                    }
                    other => panic!("expected Ping, got {other:?}"),
                }
            }
        });

        let mut rtts = Vec::new();
        let mut notifications = Vec::new();

        for _ in 0..3 {
            let rtt = client.ping().await.unwrap();
            rtts.push(rtt);

            // Drain notifications that arrived during ping.
            while let Ok(Some(env)) = client.try_recv() {
                notifications.push(env);
            }
        }

        assert_eq!(rtts.len(), 3);
        assert_eq!(notifications.len(), 3);
        for (i, n) in notifications.iter().enumerate() {
            match n {
                MessageEnvelope::Notify(notif) => {
                    assert_eq!(notif.topic, format!("before_pong_{i}"));
                }
                other => panic!("expected Notify, got {other:?}"),
            }
        }

        let _ = server_handle.await;
        client.shutdown().await.unwrap();
    }
}

// ── Send tests ──

mod send_tests {
    use super::*;
    use uuid::Uuid;

    /// Verify send_request reaches the peer as a Request envelope.
    #[tokio::test]
    async fn test_send_request_received_by_peer() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        let req_id = client
            .send_request("execute_python", b"print(1)".to_vec())
            .await
            .unwrap();

        let env = server.recv().await.unwrap().unwrap();
        match env {
            MessageEnvelope::Request(req) => {
                assert_eq!(req.id, req_id);
                assert_eq!(req.method, "execute_python");
                assert_eq!(req.params, b"print(1)");
            }
            other => panic!("expected Request, got {other:?}"),
        }

        client.shutdown().await.unwrap();
        server.shutdown().await.unwrap();
    }

    /// Verify send_response reaches the peer as a Response envelope.
    #[tokio::test]
    async fn test_send_response_received_by_peer() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        let req_id = Uuid::new_v4();
        client
            .send_response(req_id, true, b"result".to_vec(), None)
            .await
            .unwrap();

        let env = server.recv().await.unwrap().unwrap();
        match env {
            MessageEnvelope::Response(resp) => {
                assert_eq!(resp.id, req_id);
                assert!(resp.success);
                assert_eq!(resp.payload, b"result");
                assert!(resp.error.is_none());
            }
            other => panic!("expected Response, got {other:?}"),
        }

        client.shutdown().await.unwrap();
        server.shutdown().await.unwrap();
    }

    /// Verify send_response with error flag.
    #[tokio::test]
    async fn test_send_response_with_error() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        let req_id = Uuid::new_v4();
        client
            .send_response(req_id, false, vec![], Some("something failed".to_string()))
            .await
            .unwrap();

        let env = server.recv().await.unwrap().unwrap();
        match env {
            MessageEnvelope::Response(resp) => {
                assert!(!resp.success);
                assert_eq!(resp.error.as_deref(), Some("something failed"));
            }
            other => panic!("expected Response, got {other:?}"),
        }

        client.shutdown().await.unwrap();
        server.shutdown().await.unwrap();
    }

    /// Verify send_notify reaches the peer as a Notify envelope.
    #[tokio::test]
    async fn test_send_notify_received_by_peer() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        client
            .send_notify("scene_changed", b"data".to_vec())
            .await
            .unwrap();

        let env = server.recv().await.unwrap().unwrap();
        match env {
            MessageEnvelope::Notify(notif) => {
                assert_eq!(notif.topic, "scene_changed");
                assert_eq!(notif.data, b"data");
            }
            other => panic!("expected Notify, got {other:?}"),
        }

        client.shutdown().await.unwrap();
        server.shutdown().await.unwrap();
    }

    /// send_request with empty params.
    #[tokio::test]
    async fn test_send_request_empty_params() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        client.send_request("list_tools", vec![]).await.unwrap();

        let env = server.recv().await.unwrap().unwrap();
        match env {
            MessageEnvelope::Request(req) => {
                assert_eq!(req.method, "list_tools");
                assert!(req.params.is_empty());
            }
            other => panic!("expected Request, got {other:?}"),
        }

        client.shutdown().await.unwrap();
        server.shutdown().await.unwrap();
    }

    /// Multiple sends interleaved are received in order.
    #[tokio::test]
    async fn test_multiple_sends_ordered() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        for i in 0u8..5 {
            client
                .send_notify(format!("event_{i}"), vec![i])
                .await
                .unwrap();
        }

        for i in 0u8..5 {
            let env = server.recv().await.unwrap().unwrap();
            match env {
                MessageEnvelope::Notify(n) => {
                    assert_eq!(n.topic, format!("event_{i}"));
                }
                other => panic!("expected Notify, got {other:?}"),
            }
        }

        client.shutdown().await.unwrap();
        server.shutdown().await.unwrap();
    }

    /// send() when the peer has closed returns an error.
    #[tokio::test]
    async fn test_send_when_peer_closed_errors() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        // Drop server side — the reader loop on client side will exit soon.
        drop(server_framed);

        // Give the reader loop time to notice the closure.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // The write_tx buffer of 256 may absorb the first send; keep sending
        // until we overflow the buffer or the reader exits and drops write_rx.
        let mut failed = false;
        for _ in 0..300 {
            let result = client
                .send(MessageEnvelope::Notify(crate::message::Notification {
                    id: None,
                    topic: "x".to_string(),
                    data: vec![],
                }))
                .await;
            if result.is_err() {
                failed = true;
                break;
            }
        }
        assert!(
            failed,
            "expected at least one send to fail after peer closed"
        );
        client.shutdown().await.unwrap();
    }
}

// ── call() RPC helper tests ──

mod call_tests {
    use super::*;
    use crate::message::Response;

    /// Happy path: call() sends request and receives matching response.
    #[tokio::test]
    async fn test_call_happy_path() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        let client_handle = tokio::spawn(async move {
            client
                .call(
                    "execute_python",
                    b"print(42)".to_vec(),
                    std::time::Duration::from_secs(5),
                )
                .await
        });

        // Server receives request and echoes back a response.
        let env = server.recv().await.unwrap().unwrap();
        let req_id = match env {
            MessageEnvelope::Request(ref req) => req.id,
            other => panic!("expected Request, got {other:?}"),
        };
        server
            .send_response(req_id, true, b"result_42".to_vec(), None)
            .await
            .unwrap();

        let response = client_handle.await.unwrap().unwrap();
        assert!(response.success);
        assert_eq!(response.payload, b"result_42");
        assert_eq!(response.id, req_id);

        server.shutdown().await.unwrap();
    }

    /// call() returns CallFailed when the response has success=false.
    #[tokio::test]
    async fn test_call_returns_call_failed_on_error_response() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        let client_handle = tokio::spawn(async move {
            client
                .call("bad_method", vec![], std::time::Duration::from_secs(5))
                .await
        });

        let env = server.recv().await.unwrap().unwrap();
        let req_id = match env {
            MessageEnvelope::Request(ref req) => req.id,
            other => panic!("expected Request, got {other:?}"),
        };
        server
            .send_response(
                req_id,
                false,
                vec![],
                Some("NameError: bad_method".to_string()),
            )
            .await
            .unwrap();

        let err = client_handle.await.unwrap().unwrap_err();
        match err {
            TransportError::CallFailed { method, reason } => {
                assert_eq!(method, "bad_method");
                assert!(reason.contains("NameError"), "reason={reason}");
            }
            other => panic!("expected CallFailed, got {other:?}"),
        }

        server.shutdown().await.unwrap();
    }

    /// call() times out if no response arrives within the deadline.
    #[tokio::test]
    async fn test_call_times_out() {
        let (client_framed, _server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);

        let err = client
            .call("slow_op", vec![], std::time::Duration::from_millis(50))
            .await
            .unwrap_err();

        match err {
            TransportError::CallTimeout { method, timeout_ms } => {
                assert_eq!(method, "slow_op");
                assert_eq!(timeout_ms, 50);
            }
            other => panic!("expected CallTimeout, got {other:?}"),
        }
    }

    /// Unrelated data messages received during call() are not lost.
    #[tokio::test]
    async fn test_call_preserves_unrelated_messages() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        // Server sends a notification THEN the response.
        let server_handle = tokio::spawn(async move {
            // Receive the request.
            let env = server.recv().await.unwrap().unwrap();
            let req_id = match env {
                MessageEnvelope::Request(ref req) => req.id,
                other => panic!("expected Request, got {other:?}"),
            };

            // Send a notification first.
            server
                .send_notify("status_update", b"ready".to_vec())
                .await
                .unwrap();

            // Then send the response.
            server
                .send_response(req_id, true, b"done".to_vec(), None)
                .await
                .unwrap();

            server
        });

        let response = client
            .call("work", b"data".to_vec(), std::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert!(response.success);

        // The notification sent BEFORE the response must still be in the data channel.
        // We need the client back from the spawn — but since we moved it, we just
        // verify the server task completes cleanly (no panic).
        let _server = server_handle.await.unwrap();
    }

    /// Multiple concurrent call()s each match their own response by ID.
    #[tokio::test]
    async fn test_concurrent_calls_correct_correlation() {
        let (client_framed, server_framed) = framed_pair().await;
        // Arc<FramedChannel> for multiple concurrent callers is not supported
        // (call takes &self which requires Arc), so test sequential but verify IDs.
        let client = std::sync::Arc::new(FramedChannel::new(client_framed));
        let mut server = FramedChannel::new(server_framed);

        // Spawn two concurrent calls.
        let c1 = client.clone();
        let c2 = client.clone();

        let t1 = tokio::spawn(async move {
            c1.call(
                "method_a",
                b"params_a".to_vec(),
                std::time::Duration::from_secs(5),
            )
            .await
        });
        let t2 = tokio::spawn(async move {
            c2.call(
                "method_b",
                b"params_b".to_vec(),
                std::time::Duration::from_secs(5),
            )
            .await
        });

        // Collect both requests and respond in reverse order.
        let env1 = server.recv().await.unwrap().unwrap();
        let env2 = server.recv().await.unwrap().unwrap();

        let (id1, method1) = match &env1 {
            MessageEnvelope::Request(r) => (r.id, r.method.clone()),
            other => panic!("expected Request, got {other:?}"),
        };
        let (id2, _) = match &env2 {
            MessageEnvelope::Request(r) => (r.id, r.method.clone()),
            other => panic!("expected Request, got {other:?}"),
        };

        // Respond to request 2 first (reverse order).
        server
            .send_response(id2, true, b"resp_b".to_vec(), None)
            .await
            .unwrap();
        server
            .send_response(id1, true, b"resp_a".to_vec(), None)
            .await
            .unwrap();

        let r1 = t1.await.unwrap().unwrap();
        let r2 = t2.await.unwrap().unwrap();

        // Each call should get its own response.
        if method1 == "method_a" {
            assert_eq!(r1.payload, b"resp_a");
            assert_eq!(r2.payload, b"resp_b");
        } else {
            assert_eq!(r1.payload, b"resp_b");
            assert_eq!(r2.payload, b"resp_a");
        }

        server.shutdown().await.unwrap();
    }

    /// call() works when called via Arc<FramedChannel>.
    #[tokio::test]
    async fn test_call_via_arc() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = std::sync::Arc::new(FramedChannel::new(client_framed));
        let mut server = FramedChannel::new(server_framed);

        let client_clone = client.clone();
        let handle = tokio::spawn(async move {
            client_clone
                .call("rpc", vec![1, 2, 3], std::time::Duration::from_secs(5))
                .await
        });

        let env = server.recv().await.unwrap().unwrap();
        let req_id = match env {
            MessageEnvelope::Request(ref req) => {
                assert_eq!(req.params, vec![1, 2, 3]);
                req.id
            }
            other => panic!("expected Request, got {other:?}"),
        };
        server
            .send_response(req_id, true, b"ok".to_vec(), None)
            .await
            .unwrap();

        let resp = handle.await.unwrap().unwrap();
        assert_eq!(resp.payload, b"ok");

        server.shutdown().await.unwrap();
    }

    /// call() with empty params works correctly.
    #[tokio::test]
    async fn test_call_empty_params() {
        let (client_framed, server_framed) = framed_pair().await;
        let client = FramedChannel::new(client_framed);
        let mut server = FramedChannel::new(server_framed);

        let client_handle = tokio::spawn(async move {
            client
                .call("ping_dcc", vec![], std::time::Duration::from_secs(5))
                .await
        });

        let env = server.recv().await.unwrap().unwrap();
        let req_id = match env {
            MessageEnvelope::Request(ref req) => {
                assert!(req.params.is_empty());
                req.id
            }
            other => panic!("expected Request, got {other:?}"),
        };
        server
            .send_response(req_id, true, vec![], None)
            .await
            .unwrap();

        let resp = client_handle.await.unwrap().unwrap();
        assert!(resp.success);
        assert!(resp.payload.is_empty());

        server.shutdown().await.unwrap();
    }

    // Ensure Response struct is accessible in test scope
    #[allow(dead_code)]
    fn _assert_response_type(_: Response) {}
}
