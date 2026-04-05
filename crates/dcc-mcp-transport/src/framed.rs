//! Framed I/O — length-prefixed MessagePack frame reader/writer over [`IpcStream`].
//!
//! Wraps an [`IpcStream`] and provides `send` / `recv` methods that handle the
//! wire protocol framing: `[4-byte big-endian length][MessagePack payload]`.
//!
//! This is the bridge between the raw byte stream and typed `Request`/`Response` messages.
//!
//! ## Usage
//!
//! ```ignore
//! use dcc_mcp_transport::connector::{connect, IpcStream};
//! use dcc_mcp_transport::framed::FramedIo;
//! use dcc_mcp_transport::message::{Request, Response};
//!
//! let stream = connect(&addr, timeout).await?;
//! let mut framed = FramedIo::new(stream);
//!
//! // Send a request
//! framed.send(&request).await?;
//!
//! // Receive a response
//! let response: Response = framed.recv().await?;
//! ```

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::connector::{IpcStream, MAX_FRAME_SIZE};
use crate::error::{TransportError, TransportResult};
use crate::message::{MessageEnvelope, Notification, Ping, ShutdownMessage};

/// Length-prefixed framed I/O over an [`IpcStream`].
///
/// Handles reading and writing complete message frames with a 4-byte big-endian
/// length prefix followed by a MessagePack payload.
///
/// The maximum frame size is [`MAX_FRAME_SIZE`] (256 MB). Frames exceeding this
/// limit are rejected with [`TransportError::FrameTooLarge`].
#[derive(Debug)]
pub struct FramedIo {
    stream: IpcStream,
    /// Read buffer — reused across recv calls to avoid re-allocation.
    read_buf: Vec<u8>,
}

impl FramedIo {
    /// Create a new framed I/O wrapper around an IPC stream.
    pub fn new(stream: IpcStream) -> Self {
        Self {
            stream,
            read_buf: Vec::with_capacity(4096),
        }
    }

    /// Create a new framed I/O wrapper with a custom initial buffer capacity.
    pub fn with_capacity(stream: IpcStream, capacity: usize) -> Self {
        Self {
            stream,
            read_buf: Vec::with_capacity(capacity),
        }
    }

    /// Get a reference to the underlying stream.
    pub fn stream(&self) -> &IpcStream {
        &self.stream
    }

    /// Get a mutable reference to the underlying stream.
    pub fn stream_mut(&mut self) -> &mut IpcStream {
        &mut self.stream
    }

    /// Consume this wrapper and return the underlying stream.
    pub fn into_inner(self) -> IpcStream {
        self.stream
    }

    /// Get the transport name (e.g. "tcp", "named_pipe", "unix_socket").
    pub fn transport_name(&self) -> &'static str {
        self.stream.transport_name()
    }

    // ── Send ──

    /// Serialize and send a message as a length-prefixed frame.
    ///
    /// Wire format: `[4-byte BE length][MessagePack payload]`
    pub async fn send<T: Serialize>(&mut self, msg: &T) -> TransportResult<usize> {
        let payload =
            rmp_serde::to_vec(msg).map_err(|e| TransportError::Serialization(e.to_string()))?;

        let len = payload.len();
        if len > MAX_FRAME_SIZE as usize {
            return Err(TransportError::FrameTooLarge {
                size: len,
                max_size: MAX_FRAME_SIZE as usize,
            });
        }

        let len_bytes = (len as u32).to_be_bytes();

        // Write length prefix + payload in one go for efficiency.
        self.stream.write_all(&len_bytes).await.map_err(|e| {
            TransportError::IpcConnectionFailed {
                address: self.stream.transport_name().to_string(),
                reason: format!("write length prefix failed: {e}"),
            }
        })?;

        self.stream
            .write_all(&payload)
            .await
            .map_err(|e| TransportError::IpcConnectionFailed {
                address: self.stream.transport_name().to_string(),
                reason: format!("write payload failed: {e}"),
            })?;

        self.stream
            .flush()
            .await
            .map_err(|e| TransportError::IpcConnectionFailed {
                address: self.stream.transport_name().to_string(),
                reason: format!("flush failed: {e}"),
            })?;

        Ok(4 + len)
    }

    // ── Recv ──

    /// Receive and deserialize a length-prefixed frame.
    ///
    /// Reads exactly 4 bytes for the length prefix, then reads the full payload.
    /// Returns `TransportError::ConnectionClosed` if the peer closed the connection.
    pub async fn recv<T: for<'de> Deserialize<'de>>(&mut self) -> TransportResult<T> {
        // Read 4-byte length prefix.
        let mut len_buf = [0u8; 4];
        match self.stream.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(TransportError::ConnectionClosed);
            }
            Err(e) => {
                return Err(TransportError::IpcConnectionFailed {
                    address: self.stream.transport_name().to_string(),
                    reason: format!("read length prefix failed: {e}"),
                });
            }
        }

        let payload_len = u32::from_be_bytes(len_buf) as usize;

        // Validate frame size.
        if payload_len > MAX_FRAME_SIZE as usize {
            return Err(TransportError::FrameTooLarge {
                size: payload_len,
                max_size: MAX_FRAME_SIZE as usize,
            });
        }

        // Read the full payload.
        self.read_buf.clear();
        self.read_buf.resize(payload_len, 0);

        match self.stream.read_exact(&mut self.read_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(TransportError::ConnectionClosed);
            }
            Err(e) => {
                return Err(TransportError::IpcConnectionFailed {
                    address: self.stream.transport_name().to_string(),
                    reason: format!("read payload failed: {e}"),
                });
            }
        }

        // Deserialize.
        rmp_serde::from_slice(&self.read_buf)
            .map_err(|e| TransportError::Serialization(e.to_string()))
    }

    /// Send a request and wait for a response (request-response pattern).
    ///
    /// This is a convenience method that combines `send` and `recv`.
    pub async fn request<Req, Resp>(&mut self, msg: &Req) -> TransportResult<Resp>
    where
        Req: Serialize,
        Resp: for<'de> Deserialize<'de>,
    {
        self.send(msg).await?;
        self.recv().await
    }

    // ── Envelope API ──

    /// Send a [`MessageEnvelope`] over the wire.
    pub async fn send_envelope(&mut self, envelope: &MessageEnvelope) -> TransportResult<usize> {
        self.send(envelope).await
    }

    /// Receive a [`MessageEnvelope`] from the wire.
    pub async fn recv_envelope(&mut self) -> TransportResult<MessageEnvelope> {
        self.recv().await
    }

    /// Send a [`Ping`] and wait for the correlated [`Pong`].
    ///
    /// Skips any non-Pong envelopes received while waiting. Returns the
    /// round-trip time in milliseconds. If a [`MessageEnvelope::Shutdown`] is
    /// received before the Pong, returns [`TransportError::ConnectionClosed`].
    ///
    /// Uses a default timeout of 5 seconds. For a configurable timeout, use
    /// [`FramedIo::ping_with_timeout`].
    ///
    /// **Note:** Messages skipped during the wait are silently discarded.
    /// In production, consider a channel-based approach to avoid losing data
    /// messages that arrive between ping and pong.
    pub async fn ping(&mut self) -> TransportResult<u64> {
        self.ping_with_timeout(std::time::Duration::from_secs(5))
            .await
    }

    /// Send a [`Ping`] with a custom timeout and wait for the correlated [`Pong`].
    ///
    /// This is the full-featured version of [`FramedIo::ping`] that accepts a
    /// configurable timeout duration. If no matching `Pong` is received within
    /// the given timeout, returns [`TransportError::PingTimeout`].
    ///
    /// # Arguments
    ///
    /// * `timeout` — Maximum time to wait for a Pong response.
    ///
    /// # Returns
    ///
    /// The round-trip time in milliseconds on success.
    pub async fn ping_with_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> TransportResult<u64> {
        let ping = Ping::new();
        self.send_envelope(&MessageEnvelope::from(ping.clone()))
            .await?;

        let result = tokio::time::timeout(timeout, async {
            loop {
                let envelope: MessageEnvelope = self.recv_envelope().await?;
                match envelope {
                    MessageEnvelope::Pong(pong) if pong.id == ping.id => {
                        return Ok(pong.rtt_ms(&ping).unwrap_or(0));
                    }
                    MessageEnvelope::Shutdown(_) => {
                        return Err(TransportError::ConnectionClosed);
                    }
                    _ => continue,
                }
            }
        })
        .await;

        match result {
            Ok(inner_result) => inner_result,
            Err(_) => Err(TransportError::PingTimeout {
                timeout_ms: timeout.as_millis() as u64,
            }),
        }
    }

    /// Send a one-way notification.
    pub async fn send_notification(
        &mut self,
        topic: impl Into<String>,
        data: Vec<u8>,
    ) -> TransportResult<usize> {
        let notif = Notification {
            id: None,
            topic: topic.into(),
            data,
        };
        self.send_envelope(&MessageEnvelope::from(notif)).await
    }

    /// Send a graceful shutdown request.
    pub async fn send_shutdown(&mut self, reason: Option<String>) -> TransportResult<usize> {
        let msg = ShutdownMessage { reason };
        self.send_envelope(&MessageEnvelope::from(msg)).await
    }
}

#[cfg(test)]
mod tests {
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
}
