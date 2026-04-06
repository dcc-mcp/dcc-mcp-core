use super::*;
use crate::message::{Request, Response};
use uuid::Uuid;

/// Helper: create a pair of connected FramedIo instances over TCP.
async fn framed_pair() -> (FramedIo, FramedIo) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let connect_fut = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"));
    let (client, server) = tokio::join!(connect_fut, listener.accept());

    let client_stream = IpcStream::Tcp(client.unwrap());
    let server_stream = IpcStream::Tcp(server.unwrap().0);

    (FramedIo::new(client_stream), FramedIo::new(server_stream))
}

// ── Construction tests ──

mod construction {
    use super::*;

    #[tokio::test]
    async fn test_new() {
        let (client, _server) = framed_pair().await;
        assert_eq!(client.transport_name(), "tcp");
    }

    #[tokio::test]
    async fn test_with_capacity() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let connect_fut = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"));
        let (client, _) = tokio::join!(connect_fut, listener.accept());

        let framed = FramedIo::with_capacity(IpcStream::Tcp(client.unwrap()), 65536);
        assert_eq!(framed.read_buf.capacity(), 65536);
    }

    #[tokio::test]
    async fn test_into_inner() {
        let (client, _server) = framed_pair().await;
        let stream = client.into_inner();
        assert_eq!(stream.transport_name(), "tcp");
    }

    #[tokio::test]
    async fn test_stream_ref() {
        let (client, _server) = framed_pair().await;
        assert_eq!(client.stream().transport_name(), "tcp");
    }
}

// ── Send/Recv roundtrip tests ──

mod roundtrip {
    use super::*;

    #[tokio::test]
    async fn test_request_roundtrip() {
        let (mut client, mut server) = framed_pair().await;

        let req = Request {
            id: Uuid::new_v4(),
            method: "execute_python".to_string(),
            params: b"print('hello')".to_vec(),
        };

        let send_handle = tokio::spawn(async move {
            let bytes = client.send(&req).await.unwrap();
            (client, req, bytes)
        });

        let recv_handle = tokio::spawn(async move {
            let received: Request = server.recv().await.unwrap();
            (server, received)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let (_client, original, bytes_sent) = send_result.unwrap();
        let (_server, received) = recv_result.unwrap();

        assert_eq!(original.id, received.id);
        assert_eq!(original.method, received.method);
        assert_eq!(original.params, received.params);
        assert!(bytes_sent > 4);
    }

    #[tokio::test]
    async fn test_response_roundtrip() {
        let (mut client, mut server) = framed_pair().await;

        let resp = Response {
            id: Uuid::new_v4(),
            success: true,
            payload: b"result data".to_vec(),
            error: None,
        };

        let send_handle = tokio::spawn(async move {
            server.send(&resp).await.unwrap();
            (server, resp)
        });

        let recv_handle = tokio::spawn(async move {
            let received: Response = client.recv().await.unwrap();
            (client, received)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let (_server, original) = send_result.unwrap();
        let (_client, received) = recv_result.unwrap();

        assert_eq!(original.id, received.id);
        assert_eq!(original.success, received.success);
        assert_eq!(original.payload, received.payload);
        assert!(received.error.is_none());
    }

    #[tokio::test]
    async fn test_multiple_messages() {
        let (mut client, mut server) = framed_pair().await;
        let count = 10;

        let send_handle = tokio::spawn(async move {
            for i in 0..count {
                let req = Request {
                    id: Uuid::new_v4(),
                    method: format!("method_{i}"),
                    params: vec![i as u8; i + 1],
                };
                client.send(&req).await.unwrap();
            }
            client
        });

        let recv_handle = tokio::spawn(async move {
            let mut received = Vec::new();
            for _ in 0..count {
                let req: Request = server.recv().await.unwrap();
                received.push(req);
            }
            (server, received)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let _client = send_result.unwrap();
        let (_server, received) = recv_result.unwrap();

        assert_eq!(received.len(), count);
        for (i, req) in received.iter().enumerate() {
            assert_eq!(req.method, format!("method_{i}"));
            assert_eq!(req.params.len(), i + 1);
        }
    }

    #[tokio::test]
    async fn test_request_response_pattern() {
        let (mut client, mut server) = framed_pair().await;

        let req = Request {
            id: Uuid::new_v4(),
            method: "ping".to_string(),
            params: vec![],
        };
        let req_id = req.id;

        let server_handle = tokio::spawn(async move {
            let received: Request = server.recv().await.unwrap();
            let resp = Response {
                id: received.id,
                success: true,
                payload: b"pong".to_vec(),
                error: None,
            };
            server.send(&resp).await.unwrap();
            server
        });

        let client_handle = tokio::spawn(async move {
            client.send(&req).await.unwrap();
            let resp: Response = client.recv().await.unwrap();
            (client, resp)
        });

        let (server_result, client_result) = tokio::join!(server_handle, client_handle);
        let _server = server_result.unwrap();
        let (_client, resp) = client_result.unwrap();

        assert_eq!(resp.id, req_id);
        assert!(resp.success);
        assert_eq!(resp.payload, b"pong");
    }

    #[tokio::test]
    async fn test_convenience_request_method() {
        let (mut client, mut server) = framed_pair().await;

        let req = Request {
            id: Uuid::new_v4(),
            method: "test".to_string(),
            params: vec![1, 2, 3],
        };
        let req_id = req.id;

        let server_handle = tokio::spawn(async move {
            let received: Request = server.recv().await.unwrap();
            let resp = Response {
                id: received.id,
                success: true,
                payload: vec![4, 5, 6],
                error: None,
            };
            server.send(&resp).await.unwrap();
            server
        });

        let resp: Response = client.request(&req).await.unwrap();
        let _server = server_handle.await.unwrap();

        assert_eq!(resp.id, req_id);
        assert!(resp.success);
        assert_eq!(resp.payload, vec![4, 5, 6]);
    }

    #[tokio::test]
    async fn test_large_payload() {
        let (mut client, mut server) = framed_pair().await;

        let big_data = vec![0xABu8; 1024 * 1024];
        let req = Request {
            id: Uuid::new_v4(),
            method: "large".to_string(),
            params: big_data.clone(),
        };

        let send_handle = tokio::spawn(async move {
            client.send(&req).await.unwrap();
            client
        });

        let recv_handle = tokio::spawn(async move {
            let received: Request = server.recv().await.unwrap();
            (server, received)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let _client = send_result.unwrap();
        let (_server, received) = recv_result.unwrap();

        assert_eq!(received.params.len(), 1024 * 1024);
        assert_eq!(received.params, big_data);
    }
}

// ── Error path tests ──

mod error_paths {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_recv_connection_closed() {
        let (mut client, server) = framed_pair().await;
        drop(server);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let result: TransportResult<Request> = client.recv().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::ConnectionClosed => {}
            TransportError::IpcConnectionFailed { .. } => {}
            other => panic!("expected ConnectionClosed or IpcConnectionFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_send_to_closed_connection() {
        let (mut client, server) = framed_pair().await;
        drop(server);
        // Give the OS time to propagate the TCP RST/FIN.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let req = Request {
            id: Uuid::new_v4(),
            method: "test".to_string(),
            params: vec![],
        };

        // Send repeatedly until the broken-pipe / connection-reset error
        // surfaces. TCP send-buffers may absorb a few writes before the
        // kernel reports the peer closure, so we retry generously and also
        // interleave a short sleep to let the RST propagate.
        let mut failed = false;
        for _ in 0..30 {
            if client.send(&req).await.is_err() {
                failed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(failed, "expected send to fail after peer close");
    }

    #[tokio::test]
    async fn test_recv_corrupted_length() {
        let oversized = (MAX_FRAME_SIZE + 1).to_be_bytes();

        let (raw_client, mut raw_server) = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let connect_fut = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"));
            let (c, s) = tokio::join!(connect_fut, listener.accept());
            (c.unwrap(), s.unwrap().0)
        };

        use tokio::io::AsyncWriteExt;
        raw_server.write_all(&oversized).await.unwrap();
        raw_server.flush().await.unwrap();

        let mut framed = FramedIo::new(IpcStream::Tcp(raw_client));
        let result: TransportResult<Request> = framed.recv().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::FrameTooLarge { size, max_size } => {
                assert_eq!(size, MAX_FRAME_SIZE as usize + 1);
                assert_eq!(max_size, MAX_FRAME_SIZE as usize);
            }
            other => panic!("expected FrameTooLarge, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_recv_truncated_payload() {
        let (raw_client, mut raw_server) = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let connect_fut = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"));
            let (c, s) = tokio::join!(connect_fut, listener.accept());
            (c.unwrap(), s.unwrap().0)
        };

        use tokio::io::AsyncWriteExt;
        let len_bytes = 100u32.to_be_bytes();
        raw_server.write_all(&len_bytes).await.unwrap();
        raw_server.write_all(&[0u8; 10]).await.unwrap();
        raw_server.flush().await.unwrap();
        drop(raw_server);

        let mut framed = FramedIo::new(IpcStream::Tcp(raw_client));
        let result: TransportResult<Request> = framed.recv().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recv_invalid_msgpack() {
        let (raw_client, mut raw_server) = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let connect_fut = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"));
            let (c, s) = tokio::join!(connect_fut, listener.accept());
            (c.unwrap(), s.unwrap().0)
        };

        use tokio::io::AsyncWriteExt;
        let garbage = b"this is not valid msgpack data!!";
        let len_bytes = (garbage.len() as u32).to_be_bytes();
        raw_server.write_all(&len_bytes).await.unwrap();
        raw_server.write_all(garbage).await.unwrap();
        raw_server.flush().await.unwrap();

        let mut framed = FramedIo::new(IpcStream::Tcp(raw_client));
        let result: TransportResult<Request> = framed.recv().await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::Serialization(_) => {}
            other => panic!("expected Serialization error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_empty_payload() {
        let (mut client, mut server) = framed_pair().await;

        let req = Request {
            id: Uuid::new_v4(),
            method: String::new(),
            params: vec![],
        };

        let send_handle = tokio::spawn(async move {
            client.send(&req).await.unwrap();
            client
        });

        let recv_handle = tokio::spawn(async move {
            let received: Request = server.recv().await.unwrap();
            (server, received)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let _client = send_result.unwrap();
        let (_server, received) = recv_result.unwrap();

        assert!(received.method.is_empty());
        assert!(received.params.is_empty());
    }
}

// ── Envelope API tests ──

mod envelope_api {
    use super::*;
    use crate::message::{MessageEnvelope, Pong, Request, Response};

    #[tokio::test]
    async fn test_send_recv_envelope_request() {
        let (mut client, mut server) = framed_pair().await;

        let req = Request {
            id: Uuid::new_v4(),
            method: "test_method".to_string(),
            params: vec![1, 2, 3],
        };
        let envelope = MessageEnvelope::from(req.clone());

        let send_handle = tokio::spawn(async move {
            client.send_envelope(&envelope).await.unwrap();
            client
        });

        let recv_handle = tokio::spawn(async move {
            let received = server.recv_envelope().await.unwrap();
            (server, received)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let _client = send_result.unwrap();
        let (_server, received) = recv_result.unwrap();
        assert_eq!(received, MessageEnvelope::Request(req));
    }

    #[tokio::test]
    async fn test_send_recv_envelope_response() {
        let (mut client, mut server) = framed_pair().await;

        let resp = Response {
            id: Uuid::new_v4(),
            success: true,
            payload: vec![4, 5, 6],
            error: None,
        };
        let envelope = MessageEnvelope::from(resp.clone());

        let send_handle = tokio::spawn(async move {
            server.send_envelope(&envelope).await.unwrap();
            server
        });

        let recv_handle = tokio::spawn(async move {
            let received = client.recv_envelope().await.unwrap();
            (client, received)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let _server = send_result.unwrap();
        let (_client, received) = recv_result.unwrap();
        assert_eq!(received, MessageEnvelope::Response(resp));
    }

    #[tokio::test]
    async fn test_ping_pong_roundtrip() {
        let (mut client, mut server) = framed_pair().await;

        let server_handle = tokio::spawn(async move {
            let envelope = server.recv_envelope().await.unwrap();
            if let MessageEnvelope::Ping(ping) = envelope {
                let pong = Pong::from_ping(&ping);
                server
                    .send_envelope(&MessageEnvelope::from(pong))
                    .await
                    .unwrap();
            } else {
                panic!("expected Ping, got: {envelope:?}");
            }
            server
        });

        let rtt = client.ping().await.unwrap();
        let _server = server_handle.await.unwrap();

        assert!(rtt < 5000, "RTT {rtt}ms seems too high for local loopback");
    }

    #[tokio::test]
    async fn test_ping_skips_non_pong_messages() {
        let (mut client, mut server) = framed_pair().await;

        let server_handle = tokio::spawn(async move {
            let envelope = server.recv_envelope().await.unwrap();
            if let MessageEnvelope::Ping(ping) = envelope {
                server
                    .send_notification("distraction", vec![])
                    .await
                    .unwrap();
                let pong = Pong::from_ping(&ping);
                server
                    .send_envelope(&MessageEnvelope::from(pong))
                    .await
                    .unwrap();
            }
            server
        });

        let rtt = client.ping().await.unwrap();
        let _server = server_handle.await.unwrap();
        assert!(rtt < 5000);
    }

    #[tokio::test]
    async fn test_ping_returns_error_on_shutdown() {
        let (mut client, mut server) = framed_pair().await;

        let server_handle = tokio::spawn(async move {
            let _envelope = server.recv_envelope().await.unwrap();
            server
                .send_shutdown(Some("going away".to_string()))
                .await
                .unwrap();
            server
        });

        let result = client.ping().await;
        let _server = server_handle.await.unwrap();

        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::ConnectionClosed => {}
            other => panic!("expected ConnectionClosed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_send_notification() {
        let (mut client, mut server) = framed_pair().await;

        let send_handle = tokio::spawn(async move {
            client
                .send_notification("scene_changed", b"frame 42".to_vec())
                .await
                .unwrap();
            client
        });

        let recv_handle = tokio::spawn(async move {
            let envelope = server.recv_envelope().await.unwrap();
            (server, envelope)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let _client = send_result.unwrap();
        let (_server, envelope) = recv_result.unwrap();

        match envelope {
            MessageEnvelope::Notify(notif) => {
                assert_eq!(notif.topic, "scene_changed");
                assert_eq!(notif.data, b"frame 42");
                assert!(notif.id.is_none());
            }
            other => panic!("expected Notify, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_send_shutdown_with_reason() {
        let (mut client, mut server) = framed_pair().await;

        let send_handle = tokio::spawn(async move {
            client
                .send_shutdown(Some("maintenance".to_string()))
                .await
                .unwrap();
            client
        });

        let recv_handle = tokio::spawn(async move {
            let envelope = server.recv_envelope().await.unwrap();
            (server, envelope)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let _client = send_result.unwrap();
        let (_server, envelope) = recv_result.unwrap();

        match envelope {
            MessageEnvelope::Shutdown(msg) => {
                assert_eq!(msg.reason.as_deref(), Some("maintenance"));
            }
            other => panic!("expected Shutdown, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_send_shutdown_without_reason() {
        let (mut client, mut server) = framed_pair().await;

        let send_handle = tokio::spawn(async move {
            client.send_shutdown(None).await.unwrap();
            client
        });

        let recv_handle = tokio::spawn(async move {
            let envelope = server.recv_envelope().await.unwrap();
            (server, envelope)
        });

        let (send_result, recv_result) = tokio::join!(send_handle, recv_handle);
        let _client = send_result.unwrap();
        let (_server, envelope) = recv_result.unwrap();

        match envelope {
            MessageEnvelope::Shutdown(msg) => {
                assert!(msg.reason.is_none());
            }
            other => panic!("expected Shutdown, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_mixed_envelope_conversation() {
        let (mut client, mut server) = framed_pair().await;

        let server_handle = tokio::spawn(async move {
            // 1. Receive request.
            let envelope = server.recv_envelope().await.unwrap();
            let req_id = match &envelope {
                MessageEnvelope::Request(r) => r.id,
                other => panic!("expected Request, got: {other:?}"),
            };

            // 2. Send response.
            let resp = Response {
                id: req_id,
                success: true,
                payload: b"ok".to_vec(),
                error: None,
            };
            server
                .send_envelope(&MessageEnvelope::from(resp))
                .await
                .unwrap();

            // 3. Receive notification from client.
            let envelope = server.recv_envelope().await.unwrap();
            assert!(matches!(envelope, MessageEnvelope::Notify(_)));

            // 4. Send shutdown.
            server.send_shutdown(None).await.unwrap();
            server
        });

        // Client side.
        let req = Request {
            id: Uuid::new_v4(),
            method: "test".to_string(),
            params: vec![],
        };
        let req_id = req.id;

        // 1. Send request.
        client
            .send_envelope(&MessageEnvelope::from(req))
            .await
            .unwrap();

        // 2. Receive response.
        let envelope = client.recv_envelope().await.unwrap();
        match &envelope {
            MessageEnvelope::Response(r) => {
                assert_eq!(r.id, req_id);
                assert!(r.success);
            }
            other => panic!("expected Response, got: {other:?}"),
        }

        // 3. Send notification.
        client.send_notification("done", vec![]).await.unwrap();

        // 4. Receive shutdown.
        let envelope = client.recv_envelope().await.unwrap();
        assert!(matches!(envelope, MessageEnvelope::Shutdown(_)));

        let _server = server_handle.await.unwrap();
    }

    // ── Ping timeout tests ──

    mod ping_timeout {
        use super::*;

        #[tokio::test]
        async fn test_ping_times_out_when_no_pong() {
            let (mut client, _server) = framed_pair().await;
            // Server never responds — ping should time out.

            let result = client
                .ping_with_timeout(std::time::Duration::from_millis(50))
                .await;

            match result.unwrap_err() {
                TransportError::PingTimeout { timeout_ms } => {
                    assert_eq!(timeout_ms, 50);
                }
                other => panic!("expected PingTimeout, got: {other:?}"),
            }
        }

        #[tokio::test]
        async fn test_ping_default_timeout_succeeds_on_responsive_peer() {
            let (mut client, mut server) = framed_pair().await;

            let server_handle = tokio::spawn(async move {
                let envelope = server.recv_envelope().await.unwrap();
                if let MessageEnvelope::Ping(ping) = envelope {
                    let pong = Pong::from_ping(&ping);
                    server
                        .send_envelope(&MessageEnvelope::from(pong))
                        .await
                        .unwrap();
                }
                server
            });

            // Default ping() uses a 5s timeout — responsive peer should reply well within.
            let rtt = client.ping().await.unwrap();
            let _server = server_handle.await.unwrap();

            assert!(rtt < 5000, "RTT {rtt}ms seems too high for local loopback");
        }

        #[tokio::test]
        async fn test_ping_custom_short_timeout_succeeds_quickly() {
            let (mut client, mut server) = framed_pair().await;

            let server_handle = tokio::spawn(async move {
                let envelope = server.recv_envelope().await.unwrap();
                if let MessageEnvelope::Ping(ping) = envelope {
                    let pong = Pong::from_ping(&ping);
                    server
                        .send_envelope(&MessageEnvelope::from(pong))
                        .await
                        .unwrap();
                }
                server
            });

            // Very short timeout — but peer responds immediately, so it should succeed.
            let rtt = client
                .ping_with_timeout(std::time::Duration::from_secs(10))
                .await
                .unwrap();
            let _server = server_handle.await.unwrap();

            assert!(rtt < 10000);
        }

        #[tokio::test]
        async fn test_ping_timeout_error_message_format() {
            let (_client, _server) = framed_pair().await;

            let err = TransportError::PingTimeout { timeout_ms: 1234 };
            let msg = format!("{err}");
            assert!(msg.contains("ping timed out"));
            assert!(msg.contains("1234"));
        }
    }
}
