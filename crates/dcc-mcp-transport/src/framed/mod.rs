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

#[cfg(test)]
mod tests;

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
