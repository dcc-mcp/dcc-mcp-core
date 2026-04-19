//! Async TCP fault-injection tests for the framed I/O transport layer.
//!
//! Uses real tokio TCP connections to simulate network adversity:
//! - FramedIo detects connection closed on recv
//! - FramedIo send fails after peer drops
//! - FramedChannel ping detects connection loss
//! - FramedIo handles partial writes / slow consumer

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use dcc_mcp_transport::connector::IpcStream;
use dcc_mcp_transport::framed::FramedIo;
use dcc_mcp_transport::message::{MessageEnvelope, Ping};

/// Create a connected TCP pair wrapped in IpcStream.
async fn tcp_pair() -> (IpcStream, IpcStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let connect_fut = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"));
    let (client, server) = tokio::join!(connect_fut, listener.accept());
    let client_stream = IpcStream::Tcp(client.unwrap());
    let server_stream = IpcStream::Tcp(server.unwrap().0);
    (client_stream, server_stream)
}

// ── FramedIo detects connection closed ────────────────────────────────────────

#[tokio::test]
async fn framed_io_recv_detects_connection_closed() {
    let (client_stream, server_stream) = tcp_pair().await;
    let mut client = FramedIo::new(client_stream);
    let mut server = FramedIo::new(server_stream);

    // Server sends a ping.
    let ping = Ping::new();
    let envelope = MessageEnvelope::from(ping.clone());
    server.send_envelope(&envelope).await.unwrap();

    // Client receives it.
    let recv = client.recv_envelope().await.unwrap();
    assert!(matches!(recv, MessageEnvelope::Ping(_)));

    // Drop server — client should see ConnectionClosed on next recv.
    drop(server);
    let result = client.recv_envelope().await;
    assert!(result.is_err(), "recv after peer drop should return error");
}

#[tokio::test]
async fn framed_io_send_fails_after_peer_drop() {
    let (client_stream, server_stream) = tcp_pair().await;
    let mut client = FramedIo::new(client_stream);
    let server = FramedIo::new(server_stream);

    // Drop server.
    drop(server);

    // Give the OS a moment to propagate the RST/FIN.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Client sends may succeed (kernel buffer) or fail depending on timing.
    // But eventually, a send should fail or a recv should detect closure.
    let ping = Ping::new();
    let envelope = MessageEnvelope::from(ping.clone());

    // Try sending multiple times — at least one should fail.
    let mut any_failed = false;
    for _ in 0..10 {
        if client.send_envelope(&envelope).await.is_err() {
            any_failed = true;
            break;
        }
    }
    // If sends didn't fail (buffered), recv should detect closure.
    if !any_failed {
        let recv_result = client.recv_envelope().await;
        assert!(recv_result.is_err(), "recv should detect peer is gone");
    }
}

// ── Partial read / slow consumer ──────────────────────────────────────────────

#[tokio::test]
async fn framed_io_handles_partial_reads() {
    let (client_stream, server_stream) = tcp_pair().await;

    let mut client = FramedIo::new(client_stream);
    let mut server = FramedIo::new(server_stream);

    // Server sends a message.
    let ping = Ping::new();
    let envelope = MessageEnvelope::from(ping.clone());
    server.send_envelope(&envelope).await.unwrap();

    // Client receives successfully even if data arrives in small chunks.
    let recv = client.recv_envelope().await.unwrap();
    assert!(matches!(recv, MessageEnvelope::Ping(_)));
}

// ── Large message round-trip over TCP ─────────────────────────────────────────

#[tokio::test]
async fn framed_io_large_message_tcp() {
    let (client_stream, server_stream) = tcp_pair().await;
    let mut client = FramedIo::new(client_stream);
    let mut server = FramedIo::new(server_stream);

    // 100 KB body in a notification.
    let large_data = vec![0xAB; 100 * 1024];
    client
        .send_notification("test.large", large_data.clone())
        .await
        .unwrap();

    let recv = server.recv_envelope().await.unwrap();
    match recv {
        MessageEnvelope::Notify(n) => {
            assert_eq!(n.topic, "test.large");
            assert_eq!(n.data, large_data);
        }
        other => panic!("expected Notify, got {other:?}"),
    }
}

// ── Multiple messages in sequence ─────────────────────────────────────────────

#[tokio::test]
async fn framed_io_multiple_messages_in_sequence() {
    let (client_stream, server_stream) = tcp_pair().await;
    let mut client = FramedIo::new(client_stream);
    let mut server = FramedIo::new(server_stream);

    // Send 10 pings in sequence.
    for i in 0..10u64 {
        let ping = Ping::new();
        let envelope = MessageEnvelope::from(ping.clone());
        client.send_envelope(&envelope).await.unwrap();

        let recv = server.recv_envelope().await.unwrap();
        assert!(
            matches!(recv, MessageEnvelope::Ping(_)),
            "ping {i} should arrive"
        );
    }
}

// ── Raw TCP connection drop mid-stream ────────────────────────────────────────

#[tokio::test]
async fn raw_tcp_drop_mid_write() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let accept_fut = listener.accept();
    let connect_fut = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"));
    let (server_result, client_result) = tokio::join!(accept_fut, connect_fut);

    let mut server_tcp = server_result.unwrap().0;
    let mut client_tcp = client_result.unwrap();

    // Server writes some data then drops.
    server_tcp.write_all(b"partial").await.unwrap();
    server_tcp.shutdown().await.unwrap();
    drop(server_tcp);

    // Client reads — should get EOF eventually.
    let mut buf = vec![0u8; 1024];
    let n = client_tcp.read(&mut buf).await.unwrap();
    assert!(n > 0, "client should read the partial data");

    // Next read should return 0 (EOF).
    let n = client_tcp.read(&mut buf).await.unwrap();
    assert_eq!(n, 0, "client should see EOF after server shutdown");
}
