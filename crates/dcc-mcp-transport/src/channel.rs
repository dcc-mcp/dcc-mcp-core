//! Channel-based multiplexed I/O over [`FramedIo`].
//!
//! [`FramedChannel`] wraps a [`FramedIo`] and spawns a background read loop
//! that demultiplexes incoming [`MessageEnvelope`]s into typed channels:
//!
//! - **Data channel**: carries `Request`, `Response`, and `Notification`
//!   envelopes — consumed via [`FramedChannel::recv`].
//! - **Control channel**: carries `Pong` and `Shutdown` envelopes
//!   — consumed internally by [`FramedChannel::ping`] and shutdown detection.
//!
//! This design solves the data-loss problem in the plain [`FramedIo::ping`]
//! method, where non-Pong messages received during the ping wait are silently
//! discarded. With `FramedChannel`, all messages are preserved and routed to
//! the correct consumer.
//!
//! ## Usage
//!
//! ```ignore
//! use dcc_mcp_transport::framed::FramedIo;
//! use dcc_mcp_transport::channel::FramedChannel;
//! use dcc_mcp_transport::connector::IpcStream;
//!
//! let stream = connect(&addr, timeout).await?;
//! let framed = FramedIo::new(stream);
//! let mut channel = FramedChannel::new(framed);
//!
//! // Receive data messages (Request/Response/Notification)
//! let envelope = channel.recv().await?;
//!
//! // Ping without losing in-flight data messages
//! let rtt = channel.ping().await?;
//! ```

use tokio::sync::{mpsc, oneshot};

use crate::error::{TransportError, TransportResult};
use crate::framed::FramedIo;
use crate::message::{MessageEnvelope, Ping, Pong, Response};
use uuid::Uuid;

/// Capacity for the internal data envelope channel.
const DATA_CHANNEL_SIZE: usize = 256;

/// Capacity for the internal control envelope channel.
const CONTROL_CHANNEL_SIZE: usize = 64;

/// A ping request from the caller to the reader loop: "send this Ping,
/// and when the matching Pong arrives, return it via this oneshot".
struct PingRequest {
    /// Oneshot to deliver the matching Pong back to the caller.
    respond: oneshot::Sender<Pong>,
    /// The Ping to send over the wire.
    ping: Ping,
}

/// A call request from the caller to the reader loop:
/// "when a Response with this ID arrives, deliver it via this oneshot".
///
/// The request envelope is sent separately via `write_tx`. This struct
/// only registers the correlation so the reader can route the response.
struct CallRequest {
    /// The request ID to correlate against incoming [`Response`] envelopes.
    id: Uuid,
    /// Oneshot channel to deliver the matching response back to the caller.
    respond: oneshot::Sender<Response>,
}

/// Capacity for the outgoing write channel.
const WRITE_CHANNEL_SIZE: usize = 256;

/// A message dispatched by the background reader to control consumers.
///
/// Variants are produced in `reader_loop` and consumed either via
/// `control_rx` (in `FramedChannel`) or forwarded to `shutdown_rx`.
///
/// Note: variant fields carry data that is forwarded/transmitted rather
/// than directly pattern-matched in this crate, so dead_code is allowed.
#[allow(dead_code)]
enum ControlMessage {
    /// A Pong response (may or may not correlate with an active ping).
    Pong(Pong),
    /// Peer requested graceful shutdown.
    Shutdown(Option<String>),
}

/// Channel-based multiplexed I/O wrapper around [`FramedIo`].
///
/// Spawns a tokio task that continuously reads from the underlying stream
/// and routes envelopes to typed channels:
///
/// | Envelope Variant  | Destination Channel       |
/// |-------------------|---------------------------|
/// | Request           | Data channel (via `recv`) |
/// | Response          | Data channel (via `recv`) |
/// | Notification      | Data channel (via `recv`) |
/// | Ping              | Auto-reply (server-side)   |
/// | Pong              | Control channel (`ping`)   |
/// | Shutdown          | Control + shutdown_rx     |
///
/// The background reader task shuts down automatically when:
/// - The underlying stream returns an error / ConnectionClosed
/// - The `FramedChannel` is dropped (sender halves are dropped)
/// - [`FramedChannel::shutdown`] is called
pub struct FramedChannel {
    /// Handle to the background reader task.
    read_task: Option<tokio::task::JoinHandle<()>>,

    /// Receiver for data envelopes (Request/Response/Notification).
    data_rx: mpsc::Receiver<MessageEnvelope>,

    /// Receiver for control messages (Pong/Shutdown).
    control_rx: mpsc::Receiver<ControlMessage>,

    /// Sender half used to request a ping from the reader.
    /// The reader sends the Ping on the wire and delivers matching Pongs
    /// back via the oneshot inside each `PingRequest`.
    ping_tx: mpsc::Sender<PingRequest>,

    /// Sender for outgoing envelopes. The reader loop owns the underlying
    /// `FramedIo` write half, so all outgoing writes are serialised through
    /// this channel to avoid concurrent-access issues.
    write_tx: mpsc::Sender<MessageEnvelope>,

    /// Sender used to register a pending call correlation.
    ///
    /// The reader loop matches incoming [`Response`] envelopes against
    /// registered call IDs and delivers them via the corresponding oneshot.
    call_tx: mpsc::Sender<CallRequest>,

    /// Receiver for peer-initiated shutdown notifications.
    /// Select alongside `data_rx.recv()` for graceful shutdown detection.
    pub shutdown_rx: mpsc::Receiver<Option<String>>,

    /// Oneshot sender to signal the reader loop to stop cleanly.
    stop_tx: Option<oneshot::Sender<()>>,
}

impl FramedChannel {
    /// Create a new `FramedChannel` wrapping the given [`FramedIo`].
    ///
    /// Immediately spawns a background task that reads envelopes from the
    /// stream and dispatches them to channels.
    pub fn new(framed: FramedIo) -> Self {
        let (data_tx, data_rx) = mpsc::channel(DATA_CHANNEL_SIZE);
        let (control_tx, control_rx) = mpsc::channel(CONTROL_CHANNEL_SIZE);
        let (ping_tx, ping_rx) = mpsc::channel(4);
        let (write_tx, write_rx) = mpsc::channel(WRITE_CHANNEL_SIZE);
        let (call_tx, call_rx) = mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(4);
        let (stop_tx, stop_rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            Self::reader_loop(
                framed,
                data_tx,
                control_tx,
                ping_rx,
                write_rx,
                call_rx,
                shutdown_tx,
                stop_rx,
            )
            .await;
        });

        Self {
            read_task: Some(handle),
            data_rx,
            control_rx,
            ping_tx,
            write_tx,
            call_tx,
            shutdown_rx,
            stop_tx: Some(stop_tx),
        }
    }

    /// The background reader loop.
    ///
    /// Reads envelopes from the framed stream and dispatches them:
    /// - Incoming Pings → auto-reply with Pong (convenience for server-side)
    /// - Pongs → check pending ping requests; if matched, deliver via oneshot;
    ///   otherwise forward to control channel
    /// - Responses → check pending call correlations; if matched, deliver via
    ///   oneshot; otherwise forward to data channel
    /// - Shutdown → notify both control channel and shutdown_rx
    /// - All else → data channel (for `recv()`)
    #[allow(clippy::too_many_arguments)]
    async fn reader_loop(
        mut framed: FramedIo,
        data_tx: mpsc::Sender<MessageEnvelope>,
        control_tx: mpsc::Sender<ControlMessage>,
        mut ping_rx: mpsc::Receiver<PingRequest>,
        mut write_rx: mpsc::Receiver<MessageEnvelope>,
        mut call_rx: mpsc::Receiver<CallRequest>,
        shutdown_tx: mpsc::Sender<Option<String>>,
        mut stop_rx: oneshot::Receiver<()>,
    ) {
        // Track pending ping requests so we can match incoming Pongs.
        let mut pending_pings: Vec<(Uuid, oneshot::Sender<Pong>)> = Vec::new();
        // Track pending call correlations so we can match incoming Responses.
        let mut pending_calls: Vec<(Uuid, oneshot::Sender<Response>)> = Vec::new();

        loop {
            // Clean up cancelled oneshots.
            pending_pings.retain(|(_, tx)| !tx.is_closed());
            pending_calls.retain(|(_, tx)| !tx.is_closed());

            // Use select! to simultaneously wait for:
            // 1. A stop signal from shutdown()
            // 2. A new incoming envelope from the wire
            // 3. A new ping request from the caller
            // 4. An outgoing envelope from send()
            // 5. A new call registration from call()
            tokio::select! {
                biased;

                // Branch 0 (highest priority): stop signal.
                _ = &mut stop_rx => {
                    return;
                }

                // Branch 1: incoming envelope from the peer.
                recv_result = framed.recv_envelope() => {
                    match recv_result {
                        Ok(envelope) => match envelope {
                            MessageEnvelope::Ping(ping) => {
                                // Auto-reply to incoming pings (server-side convenience).
                                let pong = Pong::from_ping(&ping);
                                let _ = framed
                                    .send_envelope(&MessageEnvelope::from(pong))
                                    .await;
                            }
                            MessageEnvelope::Pong(pong) => {
                                let pong_id = pong.id;
                                // Check if this matches a pending ping request.
                                if let Some(pos) = pending_pings.iter().position(|(id, _)| *id == pong_id) {
                                    let (_, respond) = pending_pings.remove(pos);
                                    let _ = respond.send(pong);
                                } else {
                                    // Unmatched Pong → forward to control channel.
                                    let _ = control_tx.send(ControlMessage::Pong(pong)).await;
                                }
                            }
                            MessageEnvelope::Response(resp) => {
                                let resp_id = resp.id;
                                // Check if this matches a pending call registration.
                                if let Some(pos) = pending_calls.iter().position(|(id, _)| *id == resp_id) {
                                    let (_, respond) = pending_calls.remove(pos);
                                    let _ = respond.send(resp);
                                } else {
                                    // Unmatched Response → forward to data channel.
                                    if data_tx.is_closed() {
                                        return;
                                    }
                                    if data_tx.send(MessageEnvelope::Response(resp)).await.is_err() {
                                        return;
                                    }
                                }
                            }
                            MessageEnvelope::Shutdown(msg) => {
                                let reason = msg.reason.clone();
                                let _ = control_tx
                                    .send(ControlMessage::Shutdown(reason.clone()))
                                    .await;
                                let _ = shutdown_tx.send(reason).await;
                                return; // Peer shutdown — exit cleanly.
                            }
                            other => {
                                // Data message → data channel.
                                if data_tx.is_closed() {
                                    return;
                                }
                                if data_tx.send(other).await.is_err() {
                                    return; // Receiver dropped.
                                }
                            }
                        },
                        Err(_) => {
                            // Stream error or ConnectionClosed — exit the loop.
                            return;
                        }
                    }
                }

                // Branch 2: a new ping request from the caller.
                ping_req = ping_rx.recv() => {
                    match ping_req {
                        Some(req) => {
                            let ping_id = req.ping.id;
                            // Send the Ping on the wire.
                            match framed.send_envelope(&MessageEnvelope::from(req.ping)).await {
                                Ok(_) => {
                                    pending_pings.push((ping_id, req.respond));
                                }
                                Err(_) => {
                                    // Drop the oneshot sender — receiver will get cancellation error.
                                    drop(req.respond);
                                    return;
                                }
                            }
                        }
                        None => {
                            // ping_tx was dropped — no more pings will come, but continue reading.
                        }
                    }
                }

                // Branch 3: outgoing envelope from send().
                write_msg = write_rx.recv() => {
                    match write_msg {
                        Some(envelope) => {
                            if framed.send_envelope(&envelope).await.is_err() {
                                return; // Write error — exit.
                            }
                        }
                        None => {
                            // write_tx dropped — no more sends; continue reading.
                        }
                    }
                }

                // Branch 4: a new call registration from call().
                call_req = call_rx.recv() => {
                    match call_req {
                        Some(req) => {
                            pending_calls.push((req.id, req.respond));
                        }
                        None => {
                            // call_tx dropped — no more calls; continue reading.
                        }
                    }
                }
            }
        }
    }

    /// Send an arbitrary envelope to the peer.
    ///
    /// The write is serialised through an internal channel so it never races
    /// with the background reader's auto-replies (Pong, etc.).
    ///
    /// Returns `Err(ConnectionClosed)` if the background task has already
    /// exited (stream closed or `shutdown()` called).
    pub async fn send(&self, envelope: MessageEnvelope) -> TransportResult<()> {
        self.write_tx
            .send(envelope)
            .await
            .map_err(|_| TransportError::ConnectionClosed)
    }

    /// Send a [`Request`] envelope to the peer.
    ///
    /// Convenience wrapper over [`FramedChannel::send`].
    pub async fn send_request(
        &self,
        method: impl Into<String>,
        params: Vec<u8>,
    ) -> TransportResult<uuid::Uuid> {
        let req = crate::message::Request {
            id: uuid::Uuid::new_v4(),
            method: method.into(),
            params,
        };
        let id = req.id;
        self.send(MessageEnvelope::Request(req)).await?;
        Ok(id)
    }

    /// Send a [`Response`] envelope to the peer.
    ///
    /// Convenience wrapper over [`FramedChannel::send`].
    pub async fn send_response(
        &self,
        id: uuid::Uuid,
        success: bool,
        payload: Vec<u8>,
        error: Option<String>,
    ) -> TransportResult<()> {
        let resp = crate::message::Response {
            id,
            success,
            payload,
            error,
        };
        self.send(MessageEnvelope::Response(resp)).await
    }

    /// Send a [`Notification`] envelope to the peer.
    ///
    /// Convenience wrapper over [`FramedChannel::send`].
    pub async fn send_notify(
        &self,
        topic: impl Into<String>,
        data: Vec<u8>,
    ) -> TransportResult<()> {
        let notif = crate::message::Notification {
            id: None,
            topic: topic.into(),
            data,
        };
        self.send(MessageEnvelope::Notify(notif)).await
    }

    /// Send a [`Request`] and wait for the matching [`Response`] (by ID).
    ///
    /// This is the primary RPC helper for DCC interactions. It atomically:
    ///
    /// 1. Generates a unique request ID.
    /// 2. Registers a correlation oneshot with the reader loop so that the
    ///    matching [`Response`] is delivered directly, **without consuming
    ///    unrelated data messages** (Notifications, other Responses, etc.).
    /// 3. Sends the [`Request`] envelope to the peer.
    /// 4. Awaits the correlated [`Response`] with the given `timeout`.
    ///
    /// Returns:
    /// - `Ok(Response)` when `response.success == true`.
    /// - `Err(CallFailed)` when `response.success == false` (peer returned an error).
    /// - `Err(CallTimeout)` when no response arrives within the deadline.
    /// - `Err(ConnectionClosed)` if the channel is already closed.
    ///
    /// This method takes `&self` so it can be called from multiple concurrent
    /// tasks via `Arc<FramedChannel>`. Concurrent calls are safe — each gets
    /// its own oneshot and the reader loop routes responses by ID.
    pub async fn call(
        &self,
        method: impl Into<String>,
        params: Vec<u8>,
        timeout: std::time::Duration,
    ) -> TransportResult<Response> {
        let method = method.into();

        // Build the request.
        let req = crate::message::Request {
            id: Uuid::new_v4(),
            method: method.clone(),
            params,
        };
        let req_id = req.id;

        // Register the call correlation BEFORE sending the request, so we
        // never miss a fast response.
        let (resp_tx, resp_rx) = oneshot::channel();
        let call_req = CallRequest {
            id: req_id,
            respond: resp_tx,
        };
        self.call_tx
            .send(call_req)
            .await
            .map_err(|_| TransportError::ConnectionClosed)?;

        // Send the request envelope.
        self.send(MessageEnvelope::Request(req)).await?;

        // Wait for the correlated response within the deadline.
        match tokio::time::timeout(timeout, resp_rx).await {
            Ok(Ok(resp)) => {
                if resp.success {
                    Ok(resp)
                } else {
                    Err(TransportError::CallFailed {
                        method,
                        reason: resp.error.unwrap_or_else(|| "unknown error".to_string()),
                    })
                }
            }
            Ok(Err(_)) => Err(TransportError::Internal(
                "call oneshot cancelled unexpectedly".to_string(),
            )),
            Err(_) => Err(TransportError::CallTimeout {
                method,
                timeout_ms: timeout.as_millis() as u64,
            }),
        }
    }

    /// Receive the next data envelope (`Request`, `Response`, or `Notification`).
    ///
    /// Returns `Err(ConnectionClosed)` if the background reader has shut down
    /// (stream error or dropped) and the data channel is exhausted.
    pub async fn recv(&mut self) -> TransportResult<Option<MessageEnvelope>> {
        match self.data_rx.recv().await {
            Some(envelope) => Ok(Some(envelope)),
            None => {
                // Channel closed — await task cleanup.
                if let Some(task) = self.read_task.take() {
                    let _ = task.await;
                }
                Err(TransportError::ConnectionClosed)
            }
        }
    }

    /// Try to receive a data envelope without waiting.
    ///
    /// Returns `Ok(None)` if no envelope is available right now.
    pub fn try_recv(&mut self) -> TransportResult<Option<MessageEnvelope>> {
        match self.data_rx.try_recv() {
            Ok(envelope) => Ok(Some(envelope)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(TransportError::ConnectionClosed),
        }
    }

    /// Send a Ping and wait for the correlated Pong.
    ///
    /// Unlike [`FramedIo::ping`], this method **does not lose data messages**
    /// that arrive between the Ping send and the Pong receipt. All data
    /// envelopes are buffered in the data channel and can be retrieved via
    /// [`FramedChannel::recv`].
    ///
    /// Uses a default timeout of 5 seconds.
    pub async fn ping(&mut self) -> TransportResult<u64> {
        self.ping_with_timeout(std::time::Duration::from_secs(5))
            .await
    }

    /// Send a Ping with a custom timeout and wait for the correlated Pong.
    ///
    /// The Pong is matched by correlating the Ping/Pong UUID. If the timeout
    /// expires before the matching Pong arrives, returns
    /// [`TransportError::PingTimeout`].
    ///
    /// **Data messages that arrive during the wait are NOT lost.** They are
    /// available via [`FramedChannel::recv`] after the ping completes.
    pub async fn ping_with_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> TransportResult<u64> {
        let ping = Ping::new();
        let ping_timestamp = ping.timestamp_ms;

        let (pong_tx, pong_rx) = oneshot::channel();

        let request = PingRequest {
            respond: pong_tx,
            ping,
        };

        self.ping_tx
            .send(request)
            .await
            .map_err(|_| TransportError::Internal("reader task exited".to_string()))?;

        let result = tokio::time::timeout(timeout, pong_rx).await;

        match result {
            Ok(Ok(pong)) => {
                // Calculate RTT: pong.timestamp_ms - ping_timestamp (both are unix ms)
                let rtt = pong.timestamp_ms.saturating_sub(ping_timestamp);
                Ok(rtt)
            }
            Ok(Err(_)) => Err(TransportError::Internal(
                "Pong oneshot cancelled unexpectedly".to_string(),
            )),
            Err(_) => Err(TransportError::PingTimeout {
                timeout_ms: timeout.as_millis() as u64,
            }),
        }
    }

    /// Check whether the background reader is still running.
    pub fn is_running(&self) -> bool {
        !self.data_rx.is_closed() || !self.control_rx.is_closed() || self.read_task.is_some()
    }

    /// Shut down the channel and wait for the background reader task to finish.
    ///
    /// Sends a stop signal to the reader loop, which causes it to exit even if
    /// it is currently blocked waiting for data from the underlying stream.
    /// Then awaits the reader task for clean termination.
    pub async fn shutdown(mut self) -> TransportResult<()> {
        // Signal the reader loop to stop.
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }

        // Wait for the reader task to exit.
        if let Some(task) = self.read_task.take() {
            let _ = task.await;
        }

        Ok(())
    }
}

impl Drop for FramedChannel {
    fn drop(&mut self) {
        // Send stop signal if not already sent.
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        // Abort the task in case the stop signal isn't processed quickly.
        if let Some(task) = self.read_task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;
