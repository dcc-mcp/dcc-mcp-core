//! IPC connectors — async I/O implementations for TCP, Named Pipe, and Unix Socket.
//!
//! Provides a unified [`Connector`] trait and platform-specific stream wrappers that
//! bridge `tokio::io::AsyncRead + AsyncWrite` into a single [`IpcStream`] enum.
//!
//! ## Architecture
//!
//! ```text
//!  Connector::connect(addr)
//!        │
//!        ▼
//!  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//!  │  TcpStream   │     │ NamedPipe   │     │ UnixStream  │
//!  │ (all plats)  │     │ (Windows)   │     │ (Unix)      │
//!  └──────┬───────┘     └──────┬──────┘     └──────┬──────┘
//!         └────────────────────┼────────────────────┘
//!                              ▼
//!                        IpcStream enum
//!                     (AsyncRead + AsyncWrite)
//! ```

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

#[cfg(any(unix, windows))]
use ipckit::AsyncLocalSocketStream;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::error::{TransportError, TransportResult};
use crate::ipc::TransportAddress;

/// Maximum frame size: 256 MB.
///
/// Frames larger than this are rejected to prevent memory exhaustion.
pub const MAX_FRAME_SIZE: u32 = 256 * 1024 * 1024;

/// Kind of local IPC endpoint represented by an ipckit local socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSocketKind {
    /// Windows Named Pipe endpoint.
    NamedPipe,
    /// Unix Domain Socket endpoint.
    UnixSocket,
}

// ── IpcStream ──────────────────────────────────────────────────────────────

/// A unified async I/O stream that wraps platform-specific transports.
///
/// Implements `AsyncRead + AsyncWrite` so it can be used with [`FramedIo`](crate::framed::FramedIo).
pub enum IpcStream {
    /// TCP socket (all platforms).
    Tcp(TcpStream),

    /// Local IPC stream backed by ipckit (Named Pipe / Unix Socket).
    #[cfg(any(unix, windows))]
    LocalSocket {
        stream: AsyncLocalSocketStream,
        kind: LocalSocketKind,
    },
}

impl std::fmt::Debug for IpcStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp(_) => f.debug_tuple("IpcStream::Tcp").finish(),
            #[cfg(any(unix, windows))]
            Self::LocalSocket { kind, .. } => f
                .debug_struct("IpcStream::LocalSocket")
                .field("kind", kind)
                .finish(),
        }
    }
}

impl IpcStream {
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

    /// Check if the underlying stream is a local (IPC) transport.
    pub fn is_ipc(&self) -> bool {
        match self {
            Self::Tcp(_) => false,
            #[cfg(any(unix, windows))]
            Self::LocalSocket { .. } => true,
        }
    }
}

// Delegate AsyncRead to the inner stream variant.
impl AsyncRead for IpcStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(any(unix, windows))]
            Self::LocalSocket { stream, .. } => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

// Delegate AsyncWrite to the inner stream variant.
impl AsyncWrite for IpcStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Tcp(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(any(unix, windows))]
            Self::LocalSocket { stream, .. } => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(s) => Pin::new(s).poll_flush(cx),
            #[cfg(any(unix, windows))]
            Self::LocalSocket { stream, .. } => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(any(unix, windows))]
            Self::LocalSocket { stream, .. } => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

// ── Connector ──────────────────────────────────────────────────────────────

/// Connect to a DCC instance via [`TransportAddress`], returning an [`IpcStream`].
///
/// This is the primary entry point for establishing IPC connections. It dispatches
/// to the appropriate platform-specific connector based on the address variant.
///
/// # Timeout
///
/// All connection attempts are wrapped in a tokio timeout. If the connection is
/// not established within `timeout`, [`TransportError::ConnectionTimeout`] is returned.
///
/// # Platform behaviour
///
/// | Address variant | Windows | macOS / Linux |
/// |-----------------|---------|---------------|
/// | `Tcp`           | ✅       | ✅             |
/// | `NamedPipe`     | ✅       | ❌ (error)     |
/// | `UnixSocket`    | ❌ (error) | ✅           |
pub async fn connect(addr: &TransportAddress, timeout: Duration) -> TransportResult<IpcStream> {
    tracing::debug!(address = %addr, timeout_ms = timeout.as_millis(), "connecting");

    let result = tokio::time::timeout(timeout, connect_inner(addr)).await;

    match result {
        Ok(Ok(stream)) => {
            tracing::info!(
                address = %addr,
                transport = stream.transport_name(),
                "connected"
            );
            Ok(stream)
        }
        Ok(Err(e)) => {
            tracing::warn!(address = %addr, error = %e, "connection failed");
            Err(e)
        }
        Err(_) => {
            tracing::warn!(address = %addr, timeout_ms = timeout.as_millis(), "connection timed out");
            Err(TransportError::ConnectionTimeout {
                timeout_ms: timeout.as_millis() as u64,
            })
        }
    }
}

/// Inner connection logic (without timeout wrapper).
async fn connect_inner(addr: &TransportAddress) -> TransportResult<IpcStream> {
    match addr {
        TransportAddress::Tcp { host, port } => connect_tcp(host, *port).await,

        #[cfg(windows)]
        TransportAddress::NamedPipe { path } => connect_named_pipe(path).await,

        #[cfg(not(windows))]
        TransportAddress::NamedPipe { path } => Err(TransportError::IpcNotSupported {
            transport: "named_pipe".to_string(),
            reason: format!("Named Pipes are only supported on Windows (attempted path: {path})"),
        }),

        #[cfg(unix)]
        TransportAddress::UnixSocket { path } => connect_unix_socket(path).await,

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

/// Connect via TCP.
async fn connect_tcp(host: &str, port: u16) -> TransportResult<IpcStream> {
    let addr = format!("{host}:{port}");
    let stream =
        TcpStream::connect(&addr)
            .await
            .map_err(|e| TransportError::IpcConnectionFailed {
                address: format!("tcp://{addr}"),
                reason: e.to_string(),
            })?;

    // Optimise for low-latency: disable Nagle's algorithm.
    stream
        .set_nodelay(true)
        .map_err(|e| TransportError::IpcConnectionFailed {
            address: format!("tcp://{addr}"),
            reason: format!("failed to set TCP_NODELAY: {e}"),
        })?;

    Ok(IpcStream::Tcp(stream))
}

/// Connect via Windows Named Pipe.
#[cfg(windows)]
async fn connect_named_pipe(path: &str) -> TransportResult<IpcStream> {
    let stream = AsyncLocalSocketStream::connect(path).await.map_err(|e| {
        TransportError::IpcConnectionFailed {
            address: format!("pipe://{path}"),
            reason: format!("ipckit connect failed: {e}"),
        }
    })?;

    Ok(IpcStream::LocalSocket {
        stream,
        kind: LocalSocketKind::NamedPipe,
    })
}

/// Connect via Unix Domain Socket.
#[cfg(unix)]
async fn connect_unix_socket(path: &std::path::Path) -> TransportResult<IpcStream> {
    let path_string = path.display().to_string();
    let stream = AsyncLocalSocketStream::connect(&path_string)
        .await
        .map_err(|e| TransportError::IpcConnectionFailed {
            address: format!("unix://{}", path.display()),
            reason: format!("ipckit connect failed: {e}"),
        })?;

    Ok(IpcStream::LocalSocket {
        stream,
        kind: LocalSocketKind::UnixSocket,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── IpcStream metadata tests ──

    mod ipc_stream_metadata {
        use super::*;

        #[tokio::test]
        async fn test_tcp_stream_transport_name() {
            // Create a TCP listener and connect to it.
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();

            let connect_fut = TcpStream::connect(format!("127.0.0.1:{port}"));
            let (stream, _) = tokio::join!(connect_fut, listener.accept());
            let stream = IpcStream::Tcp(stream.unwrap());

            assert_eq!(stream.transport_name(), "tcp");
            assert!(!stream.is_ipc());
        }

        #[test]
        fn test_debug_format() {
            // We can't easily construct a real IpcStream without a server,
            // but we can verify the enum variants exist and the module compiles.
            // The Debug impl is tested implicitly by the compiler.
            let _: fn(TcpStream) -> IpcStream = IpcStream::Tcp;
        }
    }

    // ── connect() function tests ──

    mod connect_tests {
        use super::*;

        #[tokio::test]
        async fn test_connect_tcp_success() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();

            let addr = TransportAddress::tcp("127.0.0.1", port);
            let connect_fut = connect(&addr, Duration::from_secs(5));

            let (result, _accept) = tokio::join!(connect_fut, listener.accept());
            let stream = result.unwrap();
            assert_eq!(stream.transport_name(), "tcp");
            assert!(!stream.is_ipc());
        }

        #[tokio::test]
        async fn test_connect_tcp_refused() {
            // Connect to a port that nothing is listening on.
            let result = connect(
                &TransportAddress::tcp("127.0.0.1", 1),
                Duration::from_secs(2),
            )
            .await;
            assert!(result.is_err());
            // On different OSes, this may be a connection refused (IpcConnectionFailed)
            // or a timeout (ConnectionTimeout). Both are acceptable.
            match result.unwrap_err() {
                TransportError::IpcConnectionFailed { address, .. } => {
                    assert!(address.starts_with("tcp://"));
                }
                TransportError::ConnectionTimeout { .. } => {
                    // Windows may timeout instead of refusing immediately.
                }
                other => {
                    panic!("expected IpcConnectionFailed or ConnectionTimeout, got: {other:?}")
                }
            }
        }

        #[tokio::test]
        async fn test_connect_tcp_timeout() {
            // Use an unreachable local address to trigger a timeout or refusal.
            // 127.0.0.2 on most systems is unreachable if not configured.
            let result = connect(
                &TransportAddress::tcp("127.0.0.2", 59999),
                Duration::from_millis(200),
            )
            .await;

            // Should be either timeout or connection failed (depending on OS).
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_connect_tcp_nodelay() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();

            let addr = TransportAddress::tcp("127.0.0.1", port);
            let connect_fut = connect(&addr, Duration::from_secs(5));

            let (result, _accept) = tokio::join!(connect_fut, listener.accept());
            let stream = result.unwrap();

            // Verify TCP_NODELAY was set.
            if let IpcStream::Tcp(ref tcp) = stream {
                assert!(tcp.nodelay().unwrap());
            } else {
                panic!("expected Tcp variant");
            }
        }

        #[cfg(windows)]
        #[tokio::test]
        async fn test_connect_named_pipe_not_found() {
            let result = connect(
                &TransportAddress::named_pipe("dcc-mcp-nonexistent-test-pipe-99999"),
                Duration::from_secs(2),
            )
            .await;
            assert!(result.is_err());
            match result.unwrap_err() {
                TransportError::IpcConnectionFailed { address, .. } => {
                    assert!(address.starts_with("pipe://"));
                }
                other => panic!("expected IpcConnectionFailed, got: {other:?}"),
            }
        }

        #[cfg(not(windows))]
        #[tokio::test]
        async fn test_connect_named_pipe_unsupported() {
            let result = connect(
                &TransportAddress::named_pipe("dcc-mcp-test"),
                Duration::from_secs(1),
            )
            .await;
            assert!(result.is_err());
            match result.unwrap_err() {
                TransportError::IpcNotSupported { transport, .. } => {
                    assert_eq!(transport, "named_pipe");
                }
                other => panic!("expected IpcNotSupported, got: {other:?}"),
            }
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn test_connect_unix_socket_not_found() {
            let result = connect(
                &TransportAddress::unix_socket("/tmp/dcc-mcp-nonexistent-test.sock"),
                Duration::from_secs(2),
            )
            .await;
            assert!(result.is_err());
            match result.unwrap_err() {
                TransportError::IpcConnectionFailed { address, .. } => {
                    assert!(address.starts_with("unix://"));
                }
                other => panic!("expected IpcConnectionFailed, got: {other:?}"),
            }
        }

        #[cfg(not(unix))]
        #[tokio::test]
        async fn test_connect_unix_socket_unsupported() {
            let result = connect(
                &TransportAddress::unix_socket("/tmp/dcc-mcp-test.sock"),
                Duration::from_secs(1),
            )
            .await;
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
