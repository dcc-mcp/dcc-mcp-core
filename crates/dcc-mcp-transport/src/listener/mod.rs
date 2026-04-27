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

#[cfg(any(unix, windows))]
use ipckit::AsyncLocalSocketListener;
use tokio::net::TcpListener;

use crate::connector::{IpcStream, LocalSocketKind};
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

    /// Local IPC listener backed by ipckit (Named Pipe / Unix Socket).
    #[cfg(any(unix, windows))]
    LocalSocket {
        listener: AsyncLocalSocketListener,
        kind: LocalSocketKind,
    },
}

impl std::fmt::Debug for IpcListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp(l) => f
                .debug_struct("IpcListener::Tcp")
                .field("addr", &l.local_addr().ok())
                .finish(),
            #[cfg(any(unix, windows))]
            Self::LocalSocket { kind, .. } => f
                .debug_struct("IpcListener::LocalSocket")
                .field("kind", kind)
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

            #[cfg(any(unix, windows))]
            Self::LocalSocket { listener, kind } => {
                let stream =
                    listener
                        .accept()
                        .await
                        .map_err(|e| TransportError::IpcConnectionFailed {
                            address: match kind {
                                LocalSocketKind::NamedPipe => "pipe://<local-socket>".to_string(),
                                LocalSocketKind::UnixSocket => "unix://<local-socket>".to_string(),
                            },
                            reason: format!("ipckit accept failed: {e}"),
                        })?;
                Ok(IpcStream::LocalSocket {
                    stream,
                    kind: *kind,
                })
            }
        }
    }

    /// Get a human-readable name for the underlying transport.
    pub fn transport_name(&self) -> &'static str {
        match self {
            Self::Tcp(_) => "tcp",
            #[cfg(any(unix, windows))]
            Self::LocalSocket { kind, .. } => match kind {
                LocalSocketKind::NamedPipe => "named_pipe",
                LocalSocketKind::UnixSocket => "unix_socket",
            },
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
            #[cfg(any(unix, windows))]
            Self::LocalSocket { listener, kind } => {
                let name = listener.name();
                match kind {
                    LocalSocketKind::NamedPipe => Ok(TransportAddress::named_pipe(name)),
                    LocalSocketKind::UnixSocket => Ok(TransportAddress::unix_socket(
                        std::path::PathBuf::from(name),
                    )),
                }
            }
        }
    }
}

// ── bind_inner ─────────────────────────────────────────────────────────────

/// Inner bind logic dispatched by address variant.
async fn bind_inner(addr: &TransportAddress) -> TransportResult<IpcListener> {
    match addr {
        TransportAddress::Tcp { host, port } => bind_tcp(host, *port).await,

        #[cfg(windows)]
        TransportAddress::NamedPipe { path } => bind_named_pipe(path).await,

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
        local_addr = ?listener.local_addr().ok(),
        "TCP listener bound"
    );

    Ok(IpcListener::Tcp(listener))
}

/// Bind a Windows Named Pipe listener backed by ipckit local socket.
#[cfg(windows)]
async fn bind_named_pipe(path: &str) -> TransportResult<IpcListener> {
    let listener = AsyncLocalSocketListener::bind(path).await.map_err(|e| {
        TransportError::IpcConnectionFailed {
            address: format!("pipe://{path}"),
            reason: format!("ipckit bind failed: {e}"),
        }
    })?;
    Ok(IpcListener::LocalSocket {
        listener,
        kind: LocalSocketKind::NamedPipe,
    })
}

/// Bind a Unix Domain Socket listener.
#[cfg(unix)]
async fn bind_unix_socket(path: &std::path::Path) -> TransportResult<IpcListener> {
    // Remove stale socket file if it exists (previous process may have crashed).
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }

    let path_string = path.display().to_string();
    let listener = AsyncLocalSocketListener::bind(&path_string)
        .await
        .map_err(|e| TransportError::IpcConnectionFailed {
            address: format!("unix://{}", path.display()),
            reason: format!("ipckit bind failed: {e}"),
        })?;

    tracing::debug!(path = %path.display(), "ipckit local socket listener bound");

    Ok(IpcListener::LocalSocket {
        listener,
        kind: LocalSocketKind::UnixSocket,
    })
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
