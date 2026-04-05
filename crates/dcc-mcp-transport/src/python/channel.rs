//! Python bindings for [`FramedChannel`] — channel-based multiplexed I/O.
//!
//! Exposes `PyFramedChannel` as a synchronous Python class, bridging the async
//! Tokio channel operations to blocking Python calls via an internal Runtime.
//!
//! ## DCC-side usage (Python)
//!
//! ```text
//! from dcc_mcp_core import IpcListener, TransportAddress, connect_ipc
//!
//! # Server side: accept a connection and get a FramedChannel
//! addr = TransportAddress.tcp("127.0.0.1", 0)
//! listener = IpcListener.bind(addr)
//! local = listener.local_address()
//!
//! # Client side: connect and get a FramedChannel
//! channel = connect_ipc(local)
//!
//! # ── Primary RPC helper ──────────────────────────────────────────────
//! # call() sends a Request and waits for the matching Response by UUID.
//! # Unrelated messages (Notifications) are NOT lost during the wait.
//! result = channel.call("execute_python", b'print("hello")')
//! assert result["success"]
//!
//! # ── Low-level send/recv ─────────────────────────────────────────────
//! # Send a Request (returns request UUID string)
//! req_id = channel.send_request("execute_python", b'print("hello")')
//!
//! # Send a Response
//! channel.send_response(req_id, success=True, payload=b"ok")
//!
//! # Send a one-way Notification
//! channel.send_notify("scene_changed", b"scene_data")
//!
//! # Receive the next data message (blocks until available)
//! msg = channel.recv()  # Returns dict or None on connection close
//!
//! # Non-blocking receive — returns None immediately if no message
//! msg = channel.try_recv()
//!
//! # Ping (heartbeat), returns RTT in milliseconds
//! rtt = channel.ping()
//!
//! # Check if background reader is still active
//! running = channel.is_running
//!
//! # Graceful shutdown
//! channel.shutdown()
//! ```

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;

#[cfg(feature = "python-bindings")]
use std::time::Duration;

#[cfg(feature = "python-bindings")]
use crate::channel::FramedChannel;
#[cfg(feature = "python-bindings")]
use crate::connector::connect;
#[cfg(feature = "python-bindings")]
use crate::framed::FramedIo;
#[cfg(feature = "python-bindings")]
use crate::message::MessageEnvelope;

#[cfg(feature = "python-bindings")]
use super::types::PyTransportAddress;

// ── envelope_to_py ────────────────────────────────────────────────────────

/// Convert a [`MessageEnvelope`] to a Python dict with typed fields.
///
/// The returned dict always contains:
/// - `"type"`: `"request"` | `"response"` | `"notify"` | `"ping"` | `"pong"` | `"shutdown"`
///
/// Additional fields depend on the variant:
/// - `request`:  `"id"`, `"method"`, `"params"` (bytes)
/// - `response`: `"id"`, `"success"`, `"payload"` (bytes), `"error"`
/// - `notify`:   `"id"` (str or None), `"topic"`, `"data"` (bytes)
/// - `ping`:     `"id"`, `"timestamp_ms"`
/// - `pong`:     `"id"`, `"timestamp_ms"`
/// - `shutdown`: `"reason"` (str or None)
#[cfg(feature = "python-bindings")]
fn envelope_to_py(py: Python<'_>, envelope: MessageEnvelope) -> PyResult<Py<PyAny>> {
    let d = PyDict::new(py);
    match envelope {
        MessageEnvelope::Request(req) => {
            d.set_item("type", "request")?;
            d.set_item("id", req.id.to_string())?;
            d.set_item("method", req.method)?;
            d.set_item("params", req.params.as_slice())?;
        }
        MessageEnvelope::Response(resp) => {
            d.set_item("type", "response")?;
            d.set_item("id", resp.id.to_string())?;
            d.set_item("success", resp.success)?;
            d.set_item("payload", resp.payload.as_slice())?;
            match resp.error {
                Some(e) => d.set_item("error", e)?,
                None => d.set_item("error", py.None())?,
            }
        }
        MessageEnvelope::Notify(notif) => {
            d.set_item("type", "notify")?;
            match notif.id {
                Some(id) => d.set_item("id", id.to_string())?,
                None => d.set_item("id", py.None())?,
            }
            d.set_item("topic", notif.topic)?;
            d.set_item("data", notif.data.as_slice())?;
        }
        MessageEnvelope::Ping(ping) => {
            d.set_item("type", "ping")?;
            d.set_item("id", ping.id.to_string())?;
            d.set_item("timestamp_ms", ping.timestamp_ms)?;
        }
        MessageEnvelope::Pong(pong) => {
            d.set_item("type", "pong")?;
            d.set_item("id", pong.id.to_string())?;
            d.set_item("timestamp_ms", pong.timestamp_ms)?;
        }
        MessageEnvelope::Shutdown(msg) => {
            d.set_item("type", "shutdown")?;
            match msg.reason {
                Some(r) => d.set_item("reason", r)?,
                None => d.set_item("reason", py.None())?,
            }
        }
    }
    Ok(d.unbind().into_any())
}

// ── PyFramedChannel ───────────────────────────────────────────────────────

/// Python-facing channel-based multiplexed I/O wrapper.
///
/// Wraps [`FramedChannel`] with a Tokio runtime for async→sync bridging.
/// Receives data envelopes (Request/Response/Notify) while keeping
/// control messages (Ping/Pong/Shutdown) handled automatically in the background.
///
/// Obtain a `FramedChannel` via:
/// - `IpcListener.accept()` — server-side (waits for an incoming client)
/// - `connect_ipc(addr)` — client-side connection to a DCC server
///
/// Example (server):
/// ```text
/// from dcc_mcp_core import IpcListener, TransportAddress
///
/// addr = TransportAddress.tcp("127.0.0.1", 0)
/// listener = IpcListener.bind(addr)
/// local = listener.local_address()
/// channel = listener.accept()   # blocks until client connects
/// msg = channel.recv()          # {"type": "request", "id": ..., "method": ..., "params": ...}
/// ```
///
/// Example (client):
/// ```text
/// from dcc_mcp_core import connect_ipc, TransportAddress
///
/// addr = TransportAddress.tcp("127.0.0.1", 18812)
/// channel = connect_ipc(addr)
/// rtt = channel.ping()          # send heartbeat, get RTT in ms
/// channel.shutdown()
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "FramedChannel")]
pub struct PyFramedChannel {
    /// Inner channel. Taken (set to None) after `shutdown()`.
    inner: Option<FramedChannel>,
    /// Dedicated Tokio runtime for blocking async→sync calls.
    runtime: tokio::runtime::Runtime,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyFramedChannel {
    // ── recv ──────────────────────────────────────────────────────────────

    /// Receive the next data envelope, blocking until one is available.
    ///
    /// Data envelopes are ``Request``, ``Response``, and ``Notify`` messages.
    /// ``Ping``/``Pong``/``Shutdown`` messages are handled automatically in
    /// the background and are **not** returned here.
    ///
    /// Args:
    ///     timeout_ms: Maximum wait time in milliseconds. ``None`` (default)
    ///         waits indefinitely. If the timeout expires, returns ``None``.
    ///
    /// Returns:
    ///     A dict with a ``"type"`` key and variant-specific fields, or
    ///     ``None`` if the connection was closed or the timeout expired.
    ///
    /// Raises:
    ///     RuntimeError: If the channel has already been shut down.
    #[pyo3(signature = (timeout_ms=None))]
    fn recv(&mut self, py: Python<'_>, timeout_ms: Option<u64>) -> PyResult<Py<PyAny>> {
        let channel = self.inner.as_mut().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("FramedChannel has been shut down")
        })?;

        let result = match timeout_ms {
            None => self.runtime.block_on(channel.recv()),
            Some(ms) => {
                let timeout = Duration::from_millis(ms);
                self.runtime.block_on(async {
                    tokio::time::timeout(timeout, channel.recv())
                        .await
                        .unwrap_or(Ok(None))
                })
            }
        };

        match result {
            Ok(Some(envelope)) => envelope_to_py(py, envelope),
            Ok(None) => Ok(py.None().clone_ref(py)),
            Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(e.to_string())),
        }
    }

    // ── try_recv ──────────────────────────────────────────────────────────

    /// Try to receive a data envelope without blocking.
    ///
    /// Returns the next available envelope if one is already buffered, or
    /// ``None`` if the buffer is empty.
    ///
    /// Raises:
    ///     RuntimeError: If the channel has been shut down.
    fn try_recv(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let channel = self.inner.as_mut().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("FramedChannel has been shut down")
        })?;

        match channel.try_recv() {
            Ok(Some(envelope)) => envelope_to_py(py, envelope),
            Ok(None) => Ok(py.None().clone_ref(py)),
            Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(e.to_string())),
        }
    }

    // ── ping ──────────────────────────────────────────────────────────────

    /// Send a heartbeat ping and wait for the correlated pong.
    ///
    /// Unlike a plain framed ping, **data messages that arrive during the wait
    /// are NOT lost** — they are buffered and available via :meth:`recv`.
    ///
    /// Args:
    ///     timeout_ms: Timeout in milliseconds. Defaults to 5000.
    ///
    /// Returns:
    ///     Round-trip time in milliseconds.
    ///
    /// Raises:
    ///     RuntimeError: If the channel has been shut down or the timeout
    ///         expires before a pong is received.
    #[pyo3(signature = (timeout_ms=5000))]
    fn ping(&mut self, timeout_ms: u64) -> PyResult<u64> {
        let channel = self.inner.as_mut().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("FramedChannel has been shut down")
        })?;

        let timeout = Duration::from_millis(timeout_ms);
        self.runtime
            .block_on(channel.ping_with_timeout(timeout))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    // ── is_running ────────────────────────────────────────────────────────

    /// Whether the background reader task is still running.
    ///
    /// Returns ``False`` if the channel has been shut down or the underlying
    /// connection was closed.
    #[getter]
    fn is_running(&self) -> bool {
        self.inner.as_ref().map(|c| c.is_running()).unwrap_or(false)
    }

    // ── send ──────────────────────────────────────────────────────────────

    /// Send a Request to the peer.
    ///
    /// Args:
    ///     method:    Method name string (e.g. ``"execute_python"``).
    ///     params:    Serialised parameters as bytes (optional, defaults to empty).
    ///
    /// Returns:
    ///     The request ID as a string (UUID v4).
    ///
    /// Raises:
    ///     RuntimeError: If the channel has been shut down or the connection
    ///         was lost.
    #[pyo3(signature = (method, params=None))]
    fn send_request(&self, method: &str, params: Option<Vec<u8>>) -> PyResult<String> {
        let channel = self.inner.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("FramedChannel has been shut down")
        })?;
        let params = params.unwrap_or_default();
        let id = self
            .runtime
            .block_on(channel.send_request(method, params))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(id.to_string())
    }

    /// Send a Response to the peer.
    ///
    /// Args:
    ///     request_id: UUID string of the correlated request.
    ///     success:    Whether the request succeeded.
    ///     payload:    Serialised result bytes (optional, defaults to empty).
    ///     error:      Optional error message string.
    ///
    /// Raises:
    ///     RuntimeError: If the channel has been shut down or the connection
    ///         was lost.
    ///     ValueError: If ``request_id`` is not a valid UUID string.
    #[pyo3(signature = (request_id, success, payload=None, error=None))]
    fn send_response(
        &self,
        request_id: &str,
        success: bool,
        payload: Option<Vec<u8>>,
        error: Option<String>,
    ) -> PyResult<()> {
        let channel = self.inner.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("FramedChannel has been shut down")
        })?;
        let id = uuid::Uuid::parse_str(request_id)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid UUID: {e}")))?;
        let payload = payload.unwrap_or_default();
        self.runtime
            .block_on(channel.send_response(id, success, payload, error))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Send a Notification (one-way event) to the peer.
    ///
    /// Args:
    ///     topic:  Event topic string (e.g. ``"scene_changed"``).
    ///     data:   Serialised event data bytes (optional, defaults to empty).
    ///
    /// Raises:
    ///     RuntimeError: If the channel has been shut down or the connection
    ///         was lost.
    #[pyo3(signature = (topic, data=None))]
    fn send_notify(&self, topic: &str, data: Option<Vec<u8>>) -> PyResult<()> {
        let channel = self.inner.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("FramedChannel has been shut down")
        })?;
        let data = data.unwrap_or_default();
        self.runtime
            .block_on(channel.send_notify(topic, data))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    // ── call (RPC helper) ─────────────────────────────────────────────────

    /// Send a Request and wait for the matching Response — the primary RPC helper.
    ///
    /// Atomically sends a Request and waits for the correlated Response. Any
    /// unrelated data messages (Notifications, other Responses) that arrive
    /// during the wait are **not lost** — they remain available via
    /// :meth:`recv`.
    ///
    /// This is the recommended way to invoke DCC commands from the Agent side:
    ///
    /// .. code-block:: text
    ///
    ///     # Simple synchronous RPC call (blocks until DCC replies)
    ///     result = channel.call("execute_python", b'print("hello")')
    ///     # result is a dict: {"id": "...", "success": True, "payload": b"", "error": None}
    ///
    /// Args:
    ///     method:     Method name string (e.g. ``"execute_python"``).
    ///     params:     Serialised parameters as bytes (optional, defaults to empty).
    ///     timeout_ms: Timeout in milliseconds. Defaults to 30000 (30 s).
    ///
    /// Returns:
    ///     A dict with keys ``"id"`` (str), ``"success"`` (bool),
    ///     ``"payload"`` (bytes), and ``"error"`` (str or ``None``).
    ///
    /// Raises:
    ///     RuntimeError: On timeout, connection failure, or if the channel is
    ///         shut down. The error message indicates the cause:
    ///
    ///         - ``"call '<method>' timed out after <N>ms"``
    ///         - ``"call '<method>' failed: <reason>"`` (peer returned error response)
    ///         - ``"connection closed by peer"``
    #[pyo3(signature = (method, params=None, timeout_ms=30000))]
    fn call(
        &self,
        py: Python<'_>,
        method: &str,
        params: Option<Vec<u8>>,
        timeout_ms: u64,
    ) -> PyResult<Py<PyAny>> {
        let channel = self.inner.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("FramedChannel has been shut down")
        })?;
        let params = params.unwrap_or_default();
        let timeout = Duration::from_millis(timeout_ms);

        let response = self
            .runtime
            .block_on(channel.call(method, params, timeout))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        // Convert the Response struct to a Python dict.
        let d = pyo3::types::PyDict::new(py);
        d.set_item("id", response.id.to_string())?;
        d.set_item("success", response.success)?;
        d.set_item("payload", response.payload.as_slice())?;
        d.set_item("error", response.error)?;
        Ok(d.into())
    }

    // ── shutdown ──────────────────────────────────────────────────────────
    ///
    /// Sends a stop signal to the background reader task and waits for it to
    /// finish. After calling this method, the channel cannot be used.
    ///
    /// Idempotent: calling multiple times has no effect (second call is a no-op).
    fn shutdown(&mut self) -> PyResult<()> {
        if let Some(channel) = self.inner.take() {
            self.runtime
                .block_on(channel.shutdown())
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        }
        Ok(())
    }

    // ── dunder ────────────────────────────────────────────────────────────

    fn __repr__(&self) -> String {
        match &self.inner {
            Some(c) => format!("FramedChannel(running={})", c.is_running()),
            None => "FramedChannel(shutdown)".to_string(),
        }
    }

    fn __bool__(&self) -> bool {
        self.inner.is_some()
    }
}

// ── Constructor helpers ───────────────────────────────────────────────────

/// Build a `PyFramedChannel` from an already-constructed `FramedIo`.
///
/// This is an internal helper used by both `PyIpcListener::accept()` and
/// `py_connect_ipc()`.
#[cfg(feature = "python-bindings")]
pub fn framed_io_to_py_channel(
    framed: FramedIo,
    runtime: tokio::runtime::Runtime,
) -> PyResult<PyFramedChannel> {
    // FramedChannel::new() calls tokio::spawn, so it must run inside
    // the runtime context. We enter the runtime guard to satisfy that
    // requirement without blocking.
    let channel = {
        let _guard = runtime.enter();
        FramedChannel::new(framed)
    };
    Ok(PyFramedChannel {
        inner: Some(channel),
        runtime,
    })
}

// ── connect_ipc() top-level function ─────────────────────────────────────

/// Connect to a DCC server and return a :class:`FramedChannel`.
///
/// Client-side counterpart to `IpcListener.accept()`. After connecting,
/// you can send/receive framed messages over the channel.
///
/// Args:
///     addr:       Transport address to connect to.
///     timeout_ms: Connection timeout in milliseconds. Defaults to 10000.
///
/// Returns:
///     A :class:`FramedChannel` ready for use.
///
/// Raises:
///     RuntimeError: If the connection cannot be established within the timeout.
///
/// Example:
///     ```text
///     from dcc_mcp_core import connect_ipc, TransportAddress
///
///     addr = TransportAddress.tcp("127.0.0.1", 18812)
///     channel = connect_ipc(addr)
///     rtt = channel.ping()
///     channel.shutdown()
///     ```
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "connect_ipc", signature = (addr, timeout_ms=10000))]
pub fn py_connect_ipc(addr: &PyTransportAddress, timeout_ms: u64) -> PyResult<PyFramedChannel> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("failed to create tokio runtime: {e}"))
    })?;

    let timeout = Duration::from_millis(timeout_ms);
    let stream = runtime
        .block_on(connect(&addr.inner, timeout))
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

    let framed = FramedIo::new(stream);
    framed_io_to_py_channel(framed, runtime)
}

// ── register_classes ──────────────────────────────────────────────────────

/// Register all channel Python classes and functions into the given module.
#[cfg(feature = "python-bindings")]
pub fn register_classes(m: &Bound<'_, pyo3::types::PyModule>) -> PyResult<()> {
    m.add_class::<PyFramedChannel>()?;
    m.add_function(pyo3::wrap_pyfunction!(py_connect_ipc, m)?)?;
    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use crate::connector::IpcStream;
    use crate::framed::FramedIo;

    // ── helpers ───────────────────────────────────────────────────────────

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

    // Builds PyFramedChannel without depending on Python interpreter.
    // Only valid when python-bindings feature is enabled.
    #[cfg(feature = "python-bindings")]
    fn make_py_channel(framed: FramedIo) -> PyFramedChannel {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        framed_io_to_py_channel(framed, runtime).unwrap()
    }

    // ── recv (no-pyo3 logic tests) ────────────────────────────────────────

    /// These tests verify the underlying FramedChannel mechanics without
    /// requiring a Python interpreter. The Python dict conversion layer
    /// (envelope_to_py) is covered by the Python pytest suite.
    mod recv_tests {
        use super::*;
        use crate::channel::FramedChannel;
        use crate::message::{MessageEnvelope, Request};
        use uuid::Uuid;

        #[tokio::test]
        async fn test_channel_try_recv_empty() {
            let (client_framed, _server) = framed_pair().await;
            let mut channel = FramedChannel::new(client_framed);
            // try_recv must return Ok(None) immediately
            let result = channel.try_recv();
            assert!(matches!(result, Ok(None)));
        }

        #[tokio::test]
        async fn test_channel_recv_data_message() {
            let (client_framed, mut server_framed) = framed_pair().await;
            let mut channel = FramedChannel::new(client_framed);

            let req = Request {
                id: Uuid::new_v4(),
                method: "hello".to_string(),
                params: vec![],
            };
            server_framed
                .send_envelope(&MessageEnvelope::Request(req))
                .await
                .unwrap();

            tokio::time::sleep(std::time::Duration::from_millis(20)).await;

            let result = channel.try_recv().unwrap();
            assert!(matches!(result, Some(MessageEnvelope::Request(_))));
        }
    }

    // ── ping (no-pyo3 logic tests) ────────────────────────────────────────

    mod ping_tests {
        use super::*;
        use crate::channel::FramedChannel;

        #[tokio::test]
        async fn test_framed_channel_ping_no_peer() {
            let (client_framed, server_framed) = framed_pair().await;
            drop(server_framed);
            let mut channel = FramedChannel::new(client_framed);
            // No peer to reply — should error with PingTimeout or ConnectionClosed
            let result = channel
                .ping_with_timeout(std::time::Duration::from_millis(50))
                .await;
            assert!(result.is_err());
        }
    }

    // ── PyFramedChannel lifecycle (python-bindings gated) ─────────────────
    //
    // Note: PyFramedChannel owns a tokio::runtime::Runtime, which cannot be
    // dropped inside an async context (#[tokio::test]). The underlying
    // FramedChannel semantics are already tested in channel.rs (9 tests).
    // Here we only verify the is_running / shutdown / bool / repr accessors
    // using plain sync #[test] with a manual outer runtime.

    #[cfg(feature = "python-bindings")]
    mod py_lifecycle {
        use super::*;

        /// Runs a sync closure inside an inner tokio runtime.
        /// PyFramedChannel (with its own Runtime) is created and dropped
        /// *within* this sync closure so no async context is present at drop time.
        fn with_channel<F>(f: F)
        where
            F: FnOnce(PyFramedChannel),
        {
            // Create framed pair synchronously via a temporary runtime.
            let (client_framed, _server_framed) = {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let pair = rt.block_on(async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let port = listener.local_addr().unwrap().port();
                    let connect_fut = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"));
                    let (client, server) = tokio::join!(connect_fut, listener.accept());
                    (client.unwrap(), server.unwrap().0)
                });
                pair
            };

            let client_framed =
                crate::framed::FramedIo::new(crate::connector::IpcStream::Tcp(client_framed));
            let channel = make_py_channel(client_framed);
            // f takes ownership; channel (and its runtime) is dropped here — sync context.
            f(channel);
        }

        #[test]
        fn test_py_channel_is_running() {
            with_channel(|channel| {
                assert!(channel.is_running());
            });
        }

        #[test]
        fn test_py_channel_bool_true() {
            with_channel(|channel| {
                assert!(channel.__bool__());
            });
        }

        #[test]
        fn test_py_channel_repr_contains_framed_channel() {
            with_channel(|channel| {
                assert!(channel.__repr__().contains("FramedChannel"));
            });
        }

        #[test]
        fn test_py_channel_shutdown_stops_running() {
            with_channel(|mut channel| {
                channel.shutdown().unwrap();
                assert!(!channel.is_running());
            });
        }

        #[test]
        fn test_py_channel_shutdown_idempotent() {
            with_channel(|mut channel| {
                channel.shutdown().unwrap();
                channel.shutdown().unwrap(); // must not panic
            });
        }

        #[test]
        fn test_py_channel_bool_false_after_shutdown() {
            with_channel(|mut channel| {
                channel.shutdown().unwrap();
                assert!(!channel.__bool__());
            });
        }

        #[test]
        fn test_py_channel_repr_shutdown() {
            with_channel(|mut channel| {
                channel.shutdown().unwrap();
                assert!(channel.__repr__().contains("shutdown"));
            });
        }

        #[test]
        fn test_py_channel_ping_after_shutdown_errors() {
            with_channel(|mut channel| {
                channel.shutdown().unwrap();
                assert!(channel.ping(1000).is_err());
            });
        }
    }
}
