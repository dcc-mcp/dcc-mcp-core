//! Python bindings for the DCC-Link adapter types.
//!
//! Exposes:
//! - [`PyDccLinkFrame`] — DCC-Link frame with msg_type, seq, body
//! - [`PyIpcChannelAdapter`] — thin wrapper over `ipckit::IpcChannel`
//! - [`PyGracefulIpcChannelAdapter`] — graceful channel with shutdown
//! - [`PySocketServerAdapter`] — multi-client Unix socket / named-pipe server
//!
//! ## DCC-side usage (Python)
//!
//! ```text
//! from dcc_mcp_core import IpcChannelAdapter, GracefulIpcChannelAdapter, DccLinkFrame
//!
//! # Server side
//! server = GracefulIpcChannelAdapter.create("my-dcc")
//! server.wait_for_client()
//!
//! # Client side
//! client = IpcChannelAdapter.connect("my-dcc")
//!
//! # Send/receive DCC-Link frames
//! frame = DccLinkFrame(msg_type=1, seq=0, body=b"hello")
//! client.send_frame(frame)
//! ```

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

#[cfg(feature = "python-bindings")]
use std::sync::Arc;
#[cfg(feature = "python-bindings")]
use std::time::Duration;

#[cfg(feature = "python-bindings")]
use crate::dcc_link::{
    DccLinkFrame, DccLinkType, GracefulIpcChannelAdapter, IpcChannelAdapter, SocketServerAdapter,
};

// ── PyDccLinkFrame ──────────────────────────────────────────────────────────

/// A DCC-Link frame with ``msg_type``, ``seq``, and ``body`` fields.
///
/// Args:
///     msg_type: Integer message type tag (1=Call, 2=Reply, 3=Err,
///               4=Progress, 5=Cancel, 6=Push, 7=Ping, 8=Pong).
///     seq:      Sequence number (uint64).
///     body:     Payload bytes.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "DccLinkFrame", from_py_object)]
#[derive(Clone)]
pub struct PyDccLinkFrame {
    inner: DccLinkFrame,
}

// NOTE: gen_stub_pymethods skipped — body() returns &[u8] and decode() takes &[u8]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyDccLinkFrame {
    #[new]
    #[pyo3(signature = (msg_type, seq, body=None))]
    fn new(msg_type: u8, seq: u64, body: Option<Vec<u8>>) -> PyResult<Self> {
        let msg_type = DccLinkType::try_from(msg_type)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        Ok(Self {
            inner: DccLinkFrame {
                msg_type,
                seq,
                body: body.unwrap_or_default(),
            },
        })
    }

    /// Message type tag (1=Call, 2=Reply, 3=Err, 4=Progress, 5=Cancel,
    /// 6=Push, 7=Ping, 8=Pong).
    #[getter]
    fn msg_type(&self) -> u8 {
        self.inner.msg_type as u8
    }

    /// Sequence number.
    #[getter]
    fn seq(&self) -> u64 {
        self.inner.seq
    }

    /// Payload bytes.
    #[getter]
    fn body(&self) -> &[u8] {
        &self.inner.body
    }

    /// Encode the frame to bytes (``[len][type][seq][body]``).
    fn encode(&self) -> PyResult<Vec<u8>> {
        self.inner
            .encode()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Decode a frame from bytes including the 4-byte length prefix.
    #[staticmethod]
    fn decode(data: &[u8]) -> PyResult<Self> {
        let frame = DccLinkFrame::decode(data)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self { inner: frame })
    }

    fn __repr__(&self) -> String {
        format!(
            "DccLinkFrame(msg_type={}, seq={}, body={} bytes)",
            self.inner.msg_type as u8,
            self.inner.seq,
            self.inner.body.len()
        )
    }
}

// ── PyIpcChannelAdapter ──────────────────────────────────────────────────────

/// Thin adapter over ``ipckit::IpcChannel`` using DCC-Link framing.
///
/// Create a server-side channel with :meth:`create` or connect as a client
/// with :meth:`connect`.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "IpcChannelAdapter")]
pub struct PyIpcChannelAdapter {
    inner: Arc<std::sync::Mutex<IpcChannelAdapter>>,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyIpcChannelAdapter {
    /// Create a server-side IPC channel with the given name.
    ///
    /// Args:
    ///     name: Channel name (used as the IPC endpoint identifier).
    ///
    /// Returns:
    ///     A new :class:`IpcChannelAdapter` in server mode.
    #[staticmethod]
    fn create(name: &str) -> PyResult<Self> {
        let inner = IpcChannelAdapter::create(name)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(std::sync::Mutex::new(inner)),
        })
    }

    /// Connect to an existing IPC channel by name.
    ///
    /// Args:
    ///     name: Channel name to connect to.
    ///
    /// Returns:
    ///     A new :class:`IpcChannelAdapter` in client mode.
    #[staticmethod]
    fn connect(name: &str) -> PyResult<Self> {
        let inner = IpcChannelAdapter::connect(name)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(std::sync::Mutex::new(inner)),
        })
    }

    /// Wait for a client to connect (server-side only).
    fn wait_for_client(&self) -> PyResult<()> {
        let mut inner = self.inner.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("lock poisoned: {e}"))
        })?;
        inner
            .wait_for_client()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Send a DCC-Link frame to the peer.
    ///
    /// Args:
    ///     frame: The :class:`DccLinkFrame` to send.
    fn send_frame(&self, frame: &PyDccLinkFrame) -> PyResult<()> {
        let mut inner = self.inner.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("lock poisoned: {e}"))
        })?;
        inner
            .send_frame(&frame.inner)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Receive a DCC-Link frame from the peer (blocking).
    ///
    /// Returns:
    ///     A :class:`DccLinkFrame`, or ``None`` if the channel is closed.
    fn recv_frame(&self) -> PyResult<Option<PyDccLinkFrame>> {
        let mut inner = self.inner.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("lock poisoned: {e}"))
        })?;
        match inner.recv_frame() {
            Ok(frame) => Ok(Some(PyDccLinkFrame { inner: frame })),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("channel closed") || msg.contains("Connection reset") {
                    Ok(None)
                } else {
                    Err(pyo3::exceptions::PyRuntimeError::new_err(msg))
                }
            }
        }
    }

    fn __repr__(&self) -> String {
        "IpcChannelAdapter".to_string()
    }
}

// ── PyGracefulIpcChannelAdapter ──────────────────────────────────────────────

/// Graceful IPC channel adapter with shutdown support.
///
/// Extends :class:`IpcChannelAdapter` with graceful shutdown. For
/// reentrancy-safe dispatch (``bind_affinity_thread``, ``submit``,
/// ``pump_pending``), use the Rust-level ``GracefulIpcChannelAdapter``
/// directly or the ``DeferredExecutor`` from the Python ``_core`` module.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "GracefulIpcChannelAdapter")]
pub struct PyGracefulIpcChannelAdapter {
    inner: Arc<std::sync::Mutex<GracefulIpcChannelAdapter>>,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyGracefulIpcChannelAdapter {
    /// Create a server-side graceful IPC channel.
    #[staticmethod]
    fn create(name: &str) -> PyResult<Self> {
        let inner = GracefulIpcChannelAdapter::create(name)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(std::sync::Mutex::new(inner)),
        })
    }

    /// Connect to an existing graceful IPC channel.
    #[staticmethod]
    fn connect(name: &str) -> PyResult<Self> {
        let inner = GracefulIpcChannelAdapter::connect(name)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(std::sync::Mutex::new(inner)),
        })
    }

    /// Wait for a client to connect (server-side only).
    fn wait_for_client(&self) -> PyResult<()> {
        let mut inner = self.inner.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("lock poisoned: {e}"))
        })?;
        inner
            .wait_for_client()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Send a DCC-Link frame to the peer.
    fn send_frame(&self, frame: &PyDccLinkFrame) -> PyResult<()> {
        let mut inner = self.inner.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("lock poisoned: {e}"))
        })?;
        inner
            .send_frame(&frame.inner)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Receive a DCC-Link frame from the peer (blocking).
    ///
    /// Returns:
    ///     A :class:`DccLinkFrame`, or ``None`` if the channel is closed.
    fn recv_frame(&self) -> PyResult<Option<PyDccLinkFrame>> {
        let mut inner = self.inner.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("lock poisoned: {e}"))
        })?;
        match inner.recv_frame() {
            Ok(frame) => Ok(Some(PyDccLinkFrame { inner: frame })),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("channel closed") || msg.contains("Connection reset") {
                    Ok(None)
                } else {
                    Err(pyo3::exceptions::PyRuntimeError::new_err(msg))
                }
            }
        }
    }

    /// Signal the channel to shut down gracefully.
    fn shutdown(&self) -> PyResult<()> {
        let inner = self.inner.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("lock poisoned: {e}"))
        })?;
        inner.shutdown();
        Ok(())
    }

    /// Bind the current thread as the affinity thread for reentrancy-safe
    /// dispatch. Call this **once** on the DCC main thread.
    ///
    /// This is a low-level method; for most Python use cases, prefer
    /// ``DeferredExecutor`` from ``dcc_mcp_core._core``.
    fn bind_affinity_thread(&self) -> PyResult<()> {
        let inner = self.inner.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("lock poisoned: {e}"))
        })?;
        inner.bind_affinity_thread();
        Ok(())
    }

    /// Drain pending work items on the affinity thread within the given
    /// budget. Returns the number of items processed.
    ///
    /// Call from the DCC host's idle callback (e.g. Maya ``scriptJob
    /// idleEvent``, Blender ``bpy.app.timers``).
    ///
    /// Args:
    ///     budget_ms: Budget in milliseconds. Defaults to 100.
    #[pyo3(signature = (budget_ms=100))]
    fn pump_pending(&self, budget_ms: u64) -> PyResult<usize> {
        let inner = self.inner.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("lock poisoned: {e}"))
        })?;
        Ok(inner.pump_pending(Duration::from_millis(budget_ms)))
    }

    fn __repr__(&self) -> String {
        "GracefulIpcChannelAdapter".to_string()
    }
}

// ── PySocketServerAdapter ───────────────────────────────────────────────────

/// Minimal wrapper for ``ipckit::SocketServer``.
///
/// Create a multi-client Unix socket or named-pipe server.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "SocketServerAdapter")]
pub struct PySocketServerAdapter {
    inner: Arc<SocketServerAdapter>,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PySocketServerAdapter {
    /// Create a new socket server.
    ///
    /// Args:
    ///     path: Socket path (Unix) or pipe name (Windows).
    ///     max_connections: Maximum concurrent connections. Defaults to 10.
    ///     connection_timeout_ms: Connection timeout in ms. Defaults to 30000.
    #[pyo3(signature = (path, max_connections=10, connection_timeout_ms=30000))]
    #[new]
    fn new(path: &str, max_connections: usize, connection_timeout_ms: u64) -> PyResult<Self> {
        let inner = SocketServerAdapter::new(
            path,
            max_connections,
            Duration::from_millis(connection_timeout_ms),
        )
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// The socket path this server is listening on.
    #[getter]
    fn socket_path(&self) -> &str {
        self.inner.socket_path()
    }

    /// Number of currently connected clients.
    #[getter]
    fn connection_count(&self) -> usize {
        self.inner.connection_count()
    }

    fn __repr__(&self) -> String {
        format!(
            "SocketServerAdapter(path={}, connections={})",
            self.inner.socket_path(),
            self.inner.connection_count()
        )
    }
}

// ── register_classes ────────────────────────────────────────────────────────

/// Register all DCC-Link Python classes into the given module.
#[cfg(feature = "python-bindings")]
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDccLinkFrame>()?;
    m.add_class::<PyIpcChannelAdapter>()?;
    m.add_class::<PyGracefulIpcChannelAdapter>()?;
    m.add_class::<PySocketServerAdapter>()?;
    Ok(())
}

// ── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(feature = "python-bindings")]
    #[test]
    fn test_py_dcc_link_frame_new() {
        let frame = PyDccLinkFrame::new(1, 42, Some(vec![1, 2, 3])).unwrap();
        assert_eq!(frame.msg_type(), 1);
        assert_eq!(frame.seq(), 42);
        assert_eq!(frame.body(), &[1, 2, 3]);
    }

    #[cfg(feature = "python-bindings")]
    #[test]
    fn test_py_dcc_link_frame_rejects_bad_type() {
        let result = PyDccLinkFrame::new(255, 0, None);
        assert!(result.is_err());
    }

    #[cfg(feature = "python-bindings")]
    #[test]
    fn test_py_dcc_link_frame_encode_decode() {
        let frame = PyDccLinkFrame::new(1, 99, Some(vec![4, 5, 6])).unwrap();
        let encoded = frame.encode().unwrap();
        let decoded = PyDccLinkFrame::decode(&encoded).unwrap();
        assert_eq!(decoded.msg_type(), 1);
        assert_eq!(decoded.seq(), 99);
        assert_eq!(decoded.body(), &[4, 5, 6]);
    }
}
