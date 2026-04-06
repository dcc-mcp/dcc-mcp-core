//! Python bindings for [`IpcListener`] and [`ListenerHandle`].
//!
//! Exposes `PyIpcListener` and `PyListenerHandle` as Python classes, bridging
//! async Tokio operations to synchronous Python calls via an internal runtime.
//!
//! ## DCC-side usage (Python)
//!
//! ```python,ignore
//! from dcc_mcp_core import IpcListener, TransportAddress
//!
//! # Bind a TCP listener on an ephemeral port
//! addr = TransportAddress.tcp("127.0.0.1", 0)
//! listener = IpcListener.bind(addr)
//! local = listener.local_address()
//! print(f"Listening on {local}")
//!
//! # Accept one connection (returns a FramedChannel)
//! channel = listener.accept()
//! msg = channel.recv()
//! ```

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "python-bindings")]
use crate::framed::FramedIo;
#[cfg(feature = "python-bindings")]
use crate::listener::{IpcListener, ListenerHandle};

#[cfg(feature = "python-bindings")]
use super::channel::{PyFramedChannel, framed_io_to_py_channel};
#[cfg(feature = "python-bindings")]
use super::types::PyTransportAddress;

// ── PyIpcListener ─────────────────────────────────────────────────────────

/// Python-facing IPC listener for DCC server-side applications.
///
/// Wraps [`IpcListener`] with a Tokio runtime for async→sync bridging.
/// Supports TCP, Windows Named Pipes, and Unix Domain Sockets.
///
/// ```python,ignore
/// from dcc_mcp_core import IpcListener, TransportAddress
///
/// # TCP listener on ephemeral port
/// addr = TransportAddress.tcp("127.0.0.1", 0)
/// listener = IpcListener.bind(addr)
///
/// # Get the actual bound address
/// local = listener.local_address()
/// print(f"Server bound to: {local}")
///
/// # Transport type
/// print(listener.transport_name)  # "tcp"
///
/// # Wrap in a ListenerHandle for connection tracking
/// handle = listener.into_handle()
/// print(handle.accept_count)  # 0
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "IpcListener")]
pub struct PyIpcListener {
    inner: Option<IpcListener>,
    _runtime: tokio::runtime::Runtime,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyIpcListener {
    /// Bind a listener to the given transport address.
    ///
    /// Args:
    ///     addr: Transport address (TCP, Named Pipe, or Unix Socket).
    ///
    /// Returns:
    ///     IpcListener bound to the address.
    ///
    /// Raises:
    ///     RuntimeError: If binding fails (port in use, permission denied, etc.).
    #[staticmethod]
    fn bind(addr: &PyTransportAddress) -> PyResult<Self> {
        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!(
                "failed to create tokio runtime: {e}"
            ))
        })?;

        let inner = runtime
            .block_on(IpcListener::bind(&addr.inner))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        Ok(Self {
            inner: Some(inner),
            _runtime: runtime,
        })
    }

    /// Get the local address that this listener is bound to.
    ///
    /// Returns:
    ///     TransportAddress that clients can connect to.
    ///
    /// Raises:
    ///     RuntimeError: If the listener has already been consumed by `into_handle()`.
    fn local_address(&self) -> PyResult<PyTransportAddress> {
        let listener = self.inner.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err(
                "listener has been consumed (call into_handle() was made)",
            )
        })?;
        listener
            .local_address()
            .map(|addr| PyTransportAddress { inner: addr })
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Get the transport type name ("tcp", "named_pipe", or "unix_socket").
    #[getter]
    fn transport_name(&self) -> PyResult<&'static str> {
        let listener = self.inner.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("listener has been consumed")
        })?;
        Ok(listener.transport_name())
    }

    /// Wrap this listener in a `ListenerHandle` for connection tracking and shutdown control.
    ///
    /// Consumes the `IpcListener`. After calling `into_handle()`, this object
    /// can no longer be used directly.
    ///
    /// Returns:
    ///     ListenerHandle wrapping this listener.
    ///
    /// Raises:
    ///     RuntimeError: If called more than once.
    #[allow(clippy::wrong_self_convention)]
    fn into_handle(&mut self) -> PyResult<PyListenerHandle> {
        let inner = self.inner.take().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("listener has already been consumed")
        })?;

        let handle = ListenerHandle::new(inner);
        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!(
                "failed to create tokio runtime for handle: {e}"
            ))
        })?;

        Ok(PyListenerHandle {
            inner: handle,
            _runtime: runtime,
        })
    }

    /// Accept the next incoming connection, returning a [`FramedChannel`].
    ///
    /// Blocks until a client connects or an error occurs. The returned
    /// `FramedChannel` provides `recv()` / `ping()` / `shutdown()` methods
    /// for full-duplex framed communication with the connected client.
    ///
    /// Args:
    ///     timeout_ms: Maximum wait time in milliseconds. ``None`` (default)
    ///         waits indefinitely.
    ///
    /// Returns:
    ///     A :class:`FramedChannel` connected to the newly accepted client.
    ///
    /// Raises:
    ///     RuntimeError: If no listener is bound, if the timeout expires, or
    ///         if an I/O error occurs during accept.
    #[pyo3(signature = (timeout_ms=None))]
    fn accept(&self, timeout_ms: Option<u64>) -> PyResult<PyFramedChannel> {
        let listener = self.inner.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err(
                "listener has been consumed (into_handle() was called)",
            )
        })?;

        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!(
                "failed to create tokio runtime: {e}"
            ))
        })?;

        let stream = match timeout_ms {
            None => runtime
                .block_on(listener.accept())
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?,
            Some(ms) => {
                let timeout = std::time::Duration::from_millis(ms);
                runtime
                    .block_on(async {
                        tokio::time::timeout(timeout, listener.accept())
                            .await
                            .map_err(|_| {
                                crate::error::TransportError::Internal(format!(
                                    "accept timed out after {ms}ms"
                                ))
                            })
                            .and_then(|r| r)
                    })
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?
            }
        };

        let framed = FramedIo::new(stream);
        framed_io_to_py_channel(framed, runtime)
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            Some(l) => format!("IpcListener(transport={})", l.transport_name()),
            None => "IpcListener(consumed)".to_string(),
        }
    }
}

// ── PyListenerHandle ──────────────────────────────────────────────────────

/// Python-facing listener handle with connection tracking and shutdown control.
///
/// Wraps [`ListenerHandle`] with a Tokio runtime for async→sync bridging.
///
/// ```python,ignore
/// from dcc_mcp_core import IpcListener, TransportAddress
///
/// addr = TransportAddress.tcp("127.0.0.1", 0)
/// listener = IpcListener.bind(addr)
/// handle = listener.into_handle()
///
/// print(handle.accept_count)   # 0
/// print(handle.is_shutdown)    # False
/// print(handle.transport_name) # "tcp"
///
/// local = handle.local_address()
///
/// # Gracefully stop accepting
/// handle.shutdown()
/// print(handle.is_shutdown)    # True
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ListenerHandle")]
pub struct PyListenerHandle {
    inner: ListenerHandle,
    _runtime: tokio::runtime::Runtime,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyListenerHandle {
    /// Number of connections accepted so far.
    #[getter]
    fn accept_count(&self) -> u64 {
        self.inner.accept_count()
    }

    /// Whether shutdown has been requested.
    #[getter]
    fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }

    /// Transport type name ("tcp", "named_pipe", or "unix_socket").
    #[getter]
    fn transport_name(&self) -> &'static str {
        self.inner.transport_name()
    }

    /// Get the local address of the listener.
    ///
    /// Returns:
    ///     TransportAddress of the bound listener.
    fn local_address(&self) -> PyResult<PyTransportAddress> {
        self.inner
            .local_address()
            .map(|addr| PyTransportAddress { inner: addr })
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Request the listener to stop accepting new connections.
    ///
    /// Idempotent: calling multiple times has no effect.
    fn shutdown(&self) {
        self.inner.shutdown();
    }

    fn __repr__(&self) -> String {
        format!(
            "ListenerHandle(transport={}, accept_count={}, shutdown={})",
            self.inner.transport_name(),
            self.inner.accept_count(),
            self.inner.is_shutdown(),
        )
    }
}

// ── register_classes ─────────────────────────────────────────────────────

/// Register all listener Python classes into the given module.
#[cfg(feature = "python-bindings")]
pub fn register_classes(m: &Bound<'_, pyo3::types::PyModule>) -> PyResult<()> {
    m.add_class::<PyIpcListener>()?;
    m.add_class::<PyListenerHandle>()?;
    Ok(())
}
