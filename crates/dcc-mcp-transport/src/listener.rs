//! IPC listeners — async server-side acceptors for TCP, Named Pipe, and Unix Socket.
//!
//! Provides a unified [`IpcListener`] that binds to a [`TransportAddress`] and yields
//! [`IpcStream`] connections from DCC clients. This is the server-side counterpart to
//! the [`connect()`](crate::connector::connect) client-side connector.
//!
//! ## Architecture
//!
//! ```text
//!  IpcListener::bind(addr)
//!        │
//!        ▼
//!  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//!  │ TcpListener  │     │ NamedPipe   │     │ UnixListener│
//!  │ (all plats)  │     │ (Windows)   │     │ (Unix)      │
//!  └──────┬───────┘     └──────┬──────┘     └──────┬──────┘
//!         └────────────────────┼────────────────────┘
//!                              ▼
//!                      IpcListener enum
//!                     listener.accept() → IpcStream
//! ```
//!
//! ## DCC-side usage
//!
//! A DCC plugin (Maya, Houdini, Blender, etc.) starts a listener on startup:
//!
//! ```ignore
//! let addr = TransportAddress::default_local("maya");
//! let listener = IpcListener::bind(&addr).await?;
//!     tracing::info!("DCC server listening on {}", listener.local_address());
//!
//! loop {
//!     let stream = listener.accept().await?;
//!     let mut framed = FramedIo::new(stream);
//!     // Handle requests from the MCP server ...
//! }
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::net::TcpListener;

use crate::connector::IpcStream;
use crate::error::{TransportError, TransportResult};
use crate::ipc::TransportAddress;

// ── IpcListener ────────────────────────────────────────────────────────────

/// A unified async IPC listener that accepts incoming connections.
///
/// Supports TCP, Windows Named Pipes, and Unix Domain Sockets, matching the
/// transport types in [`IpcStream`].
pub enum IpcListener {
    /// TCP listener (all platforms).
    Tcp(TcpListener),

    /// Windows Named Pipe server.
    #[cfg(windows)]
    NamedPipe(NamedPipeListener),

    /// Unix Domain Socket listener.
    #[cfg(unix)]
    UnixSocket(tokio::net::UnixListener),
}

impl std::fmt::Debug for IpcListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp(l) => f
                .debug_struct("IpcListener::Tcp")
                .field("addr", &l.local_addr().ok())
                .finish(),
            #[cfg(windows)]
            Self::NamedPipe(l) => f
                .debug_struct("IpcListener::NamedPipe")
                .field("path", &l.path)
                .finish(),
            #[cfg(unix)]
            Self::UnixSocket(l) => f
                .debug_struct("IpcListener::UnixSocket")
                .field("addr", &l.local_addr().ok())
                .finish(),
        }
    }
}

impl IpcListener {
    /// Bind to the given address and start listening.
    ///
    /// # Platform behaviour
    ///
    /// | Address variant | Windows | macOS / Linux |
    /// |-----------------|---------|---------------|
    /// | `Tcp`           | ✅       | ✅             |
    /// | `NamedPipe`     | ✅       | ❌ (error)     |
    /// | `UnixSocket`    | ❌ (error) | ✅           |
    pub async fn bind(addr: &TransportAddress) -> TransportResult<Self> {
        tracing::debug!(address = %addr, "binding listener");

        let listener = bind_inner(addr).await?;

        tracing::info!(
            address = %addr,
            transport = listener.transport_name(),
            "listener bound"
        );

        Ok(listener)
    }

    /// Accept the next incoming connection.
    ///
    /// Blocks until a client connects or an error occurs.
    /// Returns an [`IpcStream`] ready for reading and writing.
    pub async fn accept(&self) -> TransportResult<IpcStream> {
        match self {
            Self::Tcp(listener) => {
                let (stream, peer) =
                    listener
                        .accept()
                        .await
                        .map_err(|e| TransportError::IpcConnectionFailed {
                            address: format!(
                                "tcp://{}",
                                listener
                                    .local_addr()
                                    .map(|a| a.to_string())
                                    .unwrap_or_default()
                            ),
                            reason: format!("accept failed: {e}"),
                        })?;

                // Optimise for low-latency: disable Nagle's algorithm.
                let _ = stream.set_nodelay(true);

                tracing::debug!(peer = %peer, "accepted TCP connection");
                Ok(IpcStream::Tcp(stream))
            }

            #[cfg(windows)]
            Self::NamedPipe(listener) => listener.accept().await,

            #[cfg(unix)]
            Self::UnixSocket(listener) => {
                let (stream, _peer) =
                    listener
                        .accept()
                        .await
                        .map_err(|e| TransportError::IpcConnectionFailed {
                            address: format!(
                                "unix://{}",
                                listener
                                    .local_addr()
                                    .ok()
                                    .and_then(|a| a.as_pathname().map(|p| p.display().to_string()))
                                    .unwrap_or_default()
                            ),
                            reason: format!("accept failed: {e}"),
                        })?;

                tracing::debug!("accepted Unix socket connection");
                Ok(IpcStream::UnixSocket(stream))
            }
        }
    }

    /// Get a human-readable name for the underlying transport.
    pub fn transport_name(&self) -> &'static str {
        match self {
            Self::Tcp(_) => "tcp",
            #[cfg(windows)]
            Self::NamedPipe(_) => "named_pipe",
            #[cfg(unix)]
            Self::UnixSocket(_) => "unix_socket",
        }
    }

    /// Get the local address that this listener is bound to.
    ///
    /// Returns a [`TransportAddress`] that clients can use to connect.
    pub fn local_address(&self) -> TransportResult<TransportAddress> {
        match self {
            Self::Tcp(l) => {
                let addr = l
                    .local_addr()
                    .map_err(|e| TransportError::Internal(e.to_string()))?;
                Ok(TransportAddress::tcp(addr.ip().to_string(), addr.port()))
            }
            #[cfg(windows)]
            Self::NamedPipe(l) => Ok(TransportAddress::named_pipe(&l.path)),
            #[cfg(unix)]
            Self::UnixSocket(l) => {
                let addr = l
                    .local_addr()
                    .map_err(|e| TransportError::Internal(e.to_string()))?;
                let path = addr
                    .as_pathname()
                    .ok_or_else(|| {
                        TransportError::Internal("unix socket has no pathname".to_string())
                    })?
                    .to_path_buf();
                Ok(TransportAddress::UnixSocket { path })
            }
        }
    }
}

// ── Windows Named Pipe Listener ────────────────────────────────────────────

/// Named Pipe listener for Windows.
///
/// Windows Named Pipes don't have a `listen`/`accept` model like sockets.
/// Instead, a server creates a pipe instance and waits for a client to connect.
/// After each connection, a new pipe instance must be created for the next client.
///
/// This struct wraps that pattern into a familiar `accept()` loop.
#[cfg(windows)]
pub struct NamedPipeListener {
    /// The pipe path (e.g. `\\.\pipe\dcc-mcp-maya`).
    path: String,
    /// Whether the listener has been shut down.
    shutdown: Arc<AtomicBool>,
    /// Counter for accepted connections (for logging/metrics).
    accept_count: AtomicU64,
}

#[cfg(windows)]
impl NamedPipeListener {
    /// Create a new Named Pipe listener.
    fn new(path: String) -> TransportResult<Self> {
        // Verify we can create a pipe instance.
        let _ = Self::create_pipe_instance(&path)?;

        Ok(Self {
            path,
            shutdown: Arc::new(AtomicBool::new(false)),
            accept_count: AtomicU64::new(0),
        })
    }

    /// Create a single Named Pipe server instance.
    fn create_pipe_instance(
        path: &str,
    ) -> TransportResult<tokio::net::windows::named_pipe::NamedPipeServer> {
        use tokio::net::windows::named_pipe::ServerOptions;

        ServerOptions::new()
            .first_pipe_instance(false)
            .create(path)
            .map_err(|e| TransportError::IpcConnectionFailed {
                address: format!("pipe://{path}"),
                reason: format!("failed to create pipe server instance: {e}"),
            })
    }

    /// Accept the next client connection.
    async fn accept(&self) -> TransportResult<IpcStream> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(TransportError::Shutdown);
        }

        let server = Self::create_pipe_instance(&self.path)?;

        // Wait for a client to connect to this pipe instance.
        server
            .connect()
            .await
            .map_err(|e| TransportError::IpcConnectionFailed {
                address: format!("pipe://{}", self.path),
                reason: format!("client connect wait failed: {e}"),
            })?;

        self.accept_count.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(
            path = %self.path,
            count = self.accept_count.load(Ordering::Relaxed),
            "accepted Named Pipe connection"
        );

        // Convert the connected NamedPipeServer to a NamedPipeClient-like stream.
        // NamedPipeServer implements AsyncRead + AsyncWrite, same as NamedPipeClient.
        // We wrap it in IpcStream::NamedPipe via the server type.
        Ok(IpcStream::NamedPipeServer(server))
    }
}

// ── bind_inner ─────────────────────────────────────────────────────────────

/// Inner bind logic dispatched by address variant.
async fn bind_inner(addr: &TransportAddress) -> TransportResult<IpcListener> {
    match addr {
        TransportAddress::Tcp { host, port } => bind_tcp(host, *port).await,

        #[cfg(windows)]
        TransportAddress::NamedPipe { path } => bind_named_pipe(path),

        #[cfg(not(windows))]
        TransportAddress::NamedPipe { path } => Err(TransportError::IpcNotSupported {
            transport: "named_pipe".to_string(),
            reason: format!("Named Pipes are only supported on Windows (attempted path: {path})"),
        }),

        #[cfg(unix)]
        TransportAddress::UnixSocket { path } => bind_unix_socket(path).await,

        #[cfg(not(unix))]
        TransportAddress::UnixSocket { path } => Err(TransportError::IpcNotSupported {
            transport: "unix_socket".to_string(),
            reason: format!(
                "Unix Domain Sockets are only supported on macOS/Linux (attempted path: {})",
                path.display()
            ),
        }),
    }
}

/// Bind a TCP listener.
async fn bind_tcp(host: &str, port: u16) -> TransportResult<IpcListener> {
    let addr = format!("{host}:{port}");
    let listener =
        TcpListener::bind(&addr)
            .await
            .map_err(|e| TransportError::IpcConnectionFailed {
                address: format!("tcp://{addr}"),
                reason: format!("bind failed: {e}"),
            })?;

    tracing::debug!(
        local_addr = %listener.local_addr().unwrap(),
        "TCP listener bound"
    );

    Ok(IpcListener::Tcp(listener))
}

/// Bind a Windows Named Pipe listener.
#[cfg(windows)]
fn bind_named_pipe(path: &str) -> TransportResult<IpcListener> {
    let pipe_path = if path.starts_with(r"\\.\pipe\") {
        path.to_string()
    } else {
        format!(r"\\.\pipe\{path}")
    };

    let listener = NamedPipeListener::new(pipe_path)?;
    Ok(IpcListener::NamedPipe(listener))
}

/// Bind a Unix Domain Socket listener.
#[cfg(unix)]
async fn bind_unix_socket(path: &std::path::Path) -> TransportResult<IpcListener> {
    // Remove stale socket file if it exists.
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }

    let listener =
        tokio::net::UnixListener::bind(path).map_err(|e| TransportError::IpcConnectionFailed {
            address: format!("unix://{}", path.display()),
            reason: format!("bind failed: {e}"),
        })?;

    tracing::debug!(path = %path.display(), "Unix socket listener bound");

    Ok(IpcListener::UnixSocket(listener))
}

// ── AcceptGuard ────────────────────────────────────────────────────────────

/// A listener with an accept loop that tracks connection count.
///
/// Useful for DCC-side servers that need to track how many clients are connected
/// and support graceful shutdown.
pub struct ListenerHandle {
    /// The underlying listener.
    listener: IpcListener,
    /// Whether the listener should stop accepting.
    shutdown: Arc<AtomicBool>,
    /// Number of connections accepted so far.
    accept_count: AtomicU64,
}

impl ListenerHandle {
    /// Wrap a listener into a handle with shutdown support.
    pub fn new(listener: IpcListener) -> Self {
        Self {
            listener,
            shutdown: Arc::new(AtomicBool::new(false)),
            accept_count: AtomicU64::new(0),
        }
    }

    /// Accept the next connection, returning `None` if shutdown was requested.
    pub async fn accept(&self) -> Option<TransportResult<IpcStream>> {
        if self.shutdown.load(Ordering::SeqCst) {
            return None;
        }

        let result = self.listener.accept().await;
        if result.is_ok() {
            self.accept_count.fetch_add(1, Ordering::Relaxed);
        }
        Some(result)
    }

    /// Request the listener to stop accepting new connections.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        tracing::info!(
            transport = self.listener.transport_name(),
            accepted = self.accept_count.load(Ordering::Relaxed),
            "listener shutdown requested"
        );
    }

    /// Check if shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Get the number of connections accepted so far.
    pub fn accept_count(&self) -> u64 {
        self.accept_count.load(Ordering::Relaxed)
    }

    /// Get the local address of the listener.
    pub fn local_address(&self) -> TransportResult<TransportAddress> {
        self.listener.local_address()
    }

    /// Get the transport name.
    pub fn transport_name(&self) -> &'static str {
        self.listener.transport_name()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::connect;
    use crate::framed::FramedIo;
    use crate::message::{Request, Response};
    use std::time::Duration;

    // ── TCP listener tests ──

    mod tcp_listener {
        use super::*;

        #[tokio::test]
        async fn test_bind_tcp_ephemeral_port() {
            let addr = TransportAddress::tcp("127.0.0.1", 0);
            let listener = IpcListener::bind(&addr).await.unwrap();

            assert_eq!(listener.transport_name(), "tcp");

            let local = listener.local_address().unwrap();
            if let TransportAddress::Tcp { host, port } = local {
                assert_eq!(host, "127.0.0.1");
                assert_ne!(port, 0, "should have been assigned a real port");
            } else {
                panic!("expected Tcp address");
            }
        }

        #[tokio::test]
        async fn test_accept_tcp_connection() {
            let addr = TransportAddress::tcp("127.0.0.1", 0);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let local = listener.local_address().unwrap();

            // Connect from client side.
            let accept_fut = listener.accept();
            let connect_fut = connect(&local, Duration::from_secs(5));

            let (server_result, client_result) = tokio::join!(accept_fut, connect_fut);

            let server_stream = server_result.unwrap();
            let _client_stream = client_result.unwrap();

            assert_eq!(server_stream.transport_name(), "tcp");
        }

        #[tokio::test]
        async fn test_tcp_framed_roundtrip() {
            let addr = TransportAddress::tcp("127.0.0.1", 0);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let local = listener.local_address().unwrap();

            let server_fut = async {
                let stream = listener.accept().await.unwrap();
                let mut framed = FramedIo::new(stream);
                let req: Request = framed.recv().await.unwrap();
                let resp = Response {
                    id: req.id,
                    success: true,
                    payload: b"hello back".to_vec(),
                    error: None,
                };
                framed.send(&resp).await.unwrap();
            };

            let client_fut = async {
                let stream = connect(&local, Duration::from_secs(5)).await.unwrap();
                let mut framed = FramedIo::new(stream);

                let req = Request {
                    id: uuid::Uuid::new_v4(),
                    method: "ping".to_string(),
                    params: b"hello".to_vec(),
                };
                framed.send(&req).await.unwrap();

                let resp: Response = framed.recv().await.unwrap();
                assert!(resp.success);
                assert_eq!(resp.payload, b"hello back");
            };

            tokio::join!(server_fut, client_fut);
        }

        #[tokio::test]
        async fn test_tcp_multiple_accepts() {
            let addr = TransportAddress::tcp("127.0.0.1", 0);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let local = listener.local_address().unwrap();

            // Accept 3 connections sequentially.
            for i in 0..3 {
                let accept_fut = listener.accept();
                let connect_fut = connect(&local, Duration::from_secs(5));
                let (server_result, _client_result) = tokio::join!(accept_fut, connect_fut);
                let stream = server_result.unwrap();
                assert_eq!(stream.transport_name(), "tcp", "connection {i}");
            }
        }

        #[tokio::test]
        async fn test_bind_tcp_invalid_address() {
            // Binding to an invalid address should fail.
            let addr = TransportAddress::tcp("999.999.999.999", 0);
            let result = IpcListener::bind(&addr).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                TransportError::IpcConnectionFailed { address, .. } => {
                    assert!(address.starts_with("tcp://"));
                }
                other => panic!("expected IpcConnectionFailed, got: {other:?}"),
            }
        }
    }

    // ── ListenerHandle tests ──

    mod listener_handle_tests {
        use super::*;

        #[tokio::test]
        async fn test_handle_accept_and_count() {
            let addr = TransportAddress::tcp("127.0.0.1", 0);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let local = listener.local_address().unwrap();
            let handle = ListenerHandle::new(listener);

            assert_eq!(handle.accept_count(), 0);
            assert!(!handle.is_shutdown());

            let accept_fut = handle.accept();
            let connect_fut = connect(&local, Duration::from_secs(5));
            let (server_result, _client_result) = tokio::join!(accept_fut, connect_fut);

            assert!(server_result.unwrap().is_ok());
            assert_eq!(handle.accept_count(), 1);
        }

        #[tokio::test]
        async fn test_handle_shutdown() {
            let addr = TransportAddress::tcp("127.0.0.1", 0);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let handle = ListenerHandle::new(listener);

            handle.shutdown();
            assert!(handle.is_shutdown());

            let result = handle.accept().await;
            assert!(result.is_none(), "should return None after shutdown");
        }

        #[tokio::test]
        async fn test_handle_transport_name() {
            let addr = TransportAddress::tcp("127.0.0.1", 0);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let handle = ListenerHandle::new(listener);
            assert_eq!(handle.transport_name(), "tcp");
        }

        #[tokio::test]
        async fn test_handle_local_address() {
            let addr = TransportAddress::tcp("127.0.0.1", 0);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let handle = ListenerHandle::new(listener);

            let local = handle.local_address().unwrap();
            if let TransportAddress::Tcp { port, .. } = local {
                assert_ne!(port, 0);
            } else {
                panic!("expected Tcp address");
            }
        }
    }

    // ── Windows Named Pipe listener tests ──

    #[cfg(windows)]
    mod named_pipe_listener {
        use super::*;

        #[tokio::test]
        async fn test_bind_named_pipe() {
            let pipe_name = format!("dcc-mcp-test-listener-{}", uuid::Uuid::new_v4());
            let addr = TransportAddress::named_pipe(&pipe_name);
            let listener = IpcListener::bind(&addr).await.unwrap();

            assert_eq!(listener.transport_name(), "named_pipe");

            let local = listener.local_address().unwrap();
            if let TransportAddress::NamedPipe { path } = local {
                assert!(path.contains(&pipe_name));
            } else {
                panic!("expected NamedPipe address");
            }
        }

        #[tokio::test]
        async fn test_named_pipe_accept() {
            let pipe_name = format!("dcc-mcp-test-accept-{}", uuid::Uuid::new_v4());
            let addr = TransportAddress::named_pipe(&pipe_name);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let local = listener.local_address().unwrap();

            let accept_fut = listener.accept();
            let connect_fut = connect(&local, Duration::from_secs(5));

            let (server_result, client_result) = tokio::join!(accept_fut, connect_fut);
            assert!(server_result.is_ok());
            assert!(client_result.is_ok());
        }

        #[tokio::test]
        async fn test_named_pipe_framed_roundtrip() {
            let pipe_name = format!("dcc-mcp-test-framed-{}", uuid::Uuid::new_v4());
            let addr = TransportAddress::named_pipe(&pipe_name);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let local = listener.local_address().unwrap();

            let server_fut = async {
                let stream = listener.accept().await.unwrap();
                let mut framed = FramedIo::new(stream);
                let req: Request = framed.recv().await.unwrap();
                let resp = Response {
                    id: req.id,
                    success: true,
                    payload: b"pipe-response".to_vec(),
                    error: None,
                };
                framed.send(&resp).await.unwrap();
            };

            let client_fut = async {
                let stream = connect(&local, Duration::from_secs(5)).await.unwrap();
                let mut framed = FramedIo::new(stream);
                let req = Request {
                    id: uuid::Uuid::new_v4(),
                    method: "test".to_string(),
                    params: b"pipe-request".to_vec(),
                };
                framed.send(&req).await.unwrap();
                let resp: Response = framed.recv().await.unwrap();
                assert!(resp.success);
                assert_eq!(resp.payload, b"pipe-response");
            };

            tokio::join!(server_fut, client_fut);
        }
    }

    // ── Unix Socket listener tests ──

    #[cfg(unix)]
    mod unix_socket_listener {
        use super::*;

        #[tokio::test]
        async fn test_bind_unix_socket() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("test.sock");
            let addr = TransportAddress::unix_socket(&path);
            let listener = IpcListener::bind(&addr).await.unwrap();

            assert_eq!(listener.transport_name(), "unix_socket");
        }

        #[tokio::test]
        async fn test_unix_socket_accept() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("test-accept.sock");
            let addr = TransportAddress::unix_socket(&path);
            let listener = IpcListener::bind(&addr).await.unwrap();
            let local = listener.local_address().unwrap();

            let accept_fut = listener.accept();
            let connect_fut = connect(&local, Duration::from_secs(5));

            let (server_result, client_result) = tokio::join!(accept_fut, connect_fut);
            assert!(server_result.is_ok());
            assert!(client_result.is_ok());
        }

        #[tokio::test]
        async fn test_unix_socket_removes_stale_file() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("test-stale.sock");

            // Create a stale file at the socket path.
            std::fs::write(&path, "stale").unwrap();
            assert!(path.exists());

            // Binding should succeed (removes stale file).
            let addr = TransportAddress::unix_socket(&path);
            let listener = IpcListener::bind(&addr).await.unwrap();
            assert_eq!(listener.transport_name(), "unix_socket");
        }
    }

    // ── Platform-unsupported listener tests ──

    #[cfg(not(windows))]
    mod not_windows {
        use super::*;

        #[tokio::test]
        async fn test_bind_named_pipe_unsupported() {
            let addr = TransportAddress::named_pipe("dcc-mcp-test");
            let result = IpcListener::bind(&addr).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                TransportError::IpcNotSupported { transport, .. } => {
                    assert_eq!(transport, "named_pipe");
                }
                other => panic!("expected IpcNotSupported, got: {other:?}"),
            }
        }
    }

    #[cfg(not(unix))]
    mod not_unix {
        use super::*;

        #[tokio::test]
        async fn test_bind_unix_socket_unsupported() {
            let addr = TransportAddress::unix_socket("/tmp/dcc-mcp-test.sock");
            let result = IpcListener::bind(&addr).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                TransportError::IpcNotSupported { transport, .. } => {
                    assert_eq!(transport, "unix_socket");
                }
                other => panic!("expected IpcNotSupported, got: {other:?}"),
            }
        }
    }
}
