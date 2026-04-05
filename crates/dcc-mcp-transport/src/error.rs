//! Transport error types.

use thiserror::Error;

/// Errors that can occur in the transport layer.
#[derive(Debug, Error)]
pub enum TransportError {
    /// Connection failed.
    #[error("connection failed to {host}:{port}: {reason}")]
    ConnectionFailed {
        host: String,
        port: u16,
        reason: String,
    },

    /// Connection timed out.
    #[error("connection timed out after {timeout_ms}ms")]
    ConnectionTimeout { timeout_ms: u64 },

    /// Connection pool exhausted (all connections in use).
    #[error("connection pool exhausted for DCC type '{dcc_type}' (max: {max_connections})")]
    PoolExhausted {
        dcc_type: String,
        max_connections: usize,
    },

    /// Acquire timeout waiting for a pooled connection.
    #[error("acquire timeout after {timeout_ms}ms for DCC type '{dcc_type}'")]
    AcquireTimeout { dcc_type: String, timeout_ms: u64 },

    /// Service not found in registry.
    #[error("service not found: dcc_type={dcc_type}, instance_id={instance_id}")]
    ServiceNotFound {
        dcc_type: String,
        instance_id: String,
    },

    /// Service already registered.
    #[error("service already registered: dcc_type={dcc_type}, instance_id={instance_id}")]
    ServiceAlreadyRegistered {
        dcc_type: String,
        instance_id: String,
    },

    /// Serialization / deserialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Registry file error.
    #[error("registry file error: {0}")]
    RegistryFile(String),

    /// Transport is already shut down.
    #[error("transport is shut down")]
    Shutdown,

    /// Session not found.
    #[error("session not found: {session_id}")]
    SessionNotFound { session_id: String },

    /// Session is in an invalid state for the requested operation.
    #[error("session {session_id} is in state {state}, expected {expected}")]
    InvalidSessionState {
        session_id: String,
        state: String,
        expected: String,
    },

    /// Reconnection failed after max retries.
    #[error("reconnection failed for session {session_id} after {retries} retries: {reason}")]
    ReconnectionFailed {
        session_id: String,
        retries: u32,
        reason: String,
    },

    /// IPC connection failed (generic, works for TCP/Pipe/Socket).
    #[error("IPC connection failed to {address}: {reason}")]
    IpcConnectionFailed { address: String, reason: String },

    /// IPC transport not supported on this platform.
    #[error("IPC transport '{transport}' not supported: {reason}")]
    IpcNotSupported { transport: String, reason: String },

    /// Frame exceeds the maximum allowed size.
    #[error("frame too large: {size} bytes (max: {max_size} bytes)")]
    FrameTooLarge { size: usize, max_size: usize },

    /// Peer closed the connection.
    #[error("connection closed by peer")]
    ConnectionClosed,

    /// Ping (heartbeat) timed out waiting for Pong response.
    #[error("ping timed out after {timeout_ms}ms")]
    PingTimeout { timeout_ms: u64 },

    /// RPC call timed out waiting for a Response with the matching request ID.
    #[error("call '{method}' timed out after {timeout_ms}ms")]
    CallTimeout { method: String, timeout_ms: u64 },

    /// RPC call returned an error response from the peer.
    #[error("call '{method}' failed: {reason}")]
    CallFailed { method: String, reason: String },

    /// Reconnect failed after exhausting all retry attempts (pool-level).
    #[error(
        "reconnect failed for {dcc_type}/{instance_id} after {attempts} attempt(s): {last_error}"
    )]
    ReconnectFailed {
        /// DCC type (e.g. "maya").
        dcc_type: String,
        /// Instance UUID.
        instance_id: uuid::Uuid,
        /// Total attempts made (including the first one).
        attempts: u32,
        /// Error from the last attempt.
        last_error: String,
    },

    /// Generic internal error.
    #[error("{0}")]
    Internal(String),

    /// Circuit breaker is open — request rejected to prevent cascading failures.
    #[error("circuit breaker '{name}' is open — DCC connection unavailable")]
    CircuitOpen {
        /// The circuit breaker name (endpoint identifier).
        name: String,
    },
}

/// Result type alias for transport operations.
pub type TransportResult<T> = Result<T, TransportError>;

impl TransportError {
    /// Convenience constructor for a connection-refused style error.
    ///
    /// Used internally to initialise a `last_error` placeholder before the first retry.
    pub(crate) fn connection_refused(host: &str, port: u16) -> Self {
        Self::ConnectionFailed {
            host: host.to_string(),
            port,
            reason: "connection refused".to_string(),
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    mod test_display {
        use super::*;

        #[test]
        fn connection_failed_display() {
            let err = TransportError::ConnectionFailed {
                host: "127.0.0.1".to_string(),
                port: 8080,
                reason: "refused".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("127.0.0.1"), "missing host: {s}");
            assert!(s.contains("8080"), "missing port: {s}");
            assert!(s.contains("refused"), "missing reason: {s}");
        }

        #[test]
        fn connection_timeout_display() {
            let err = TransportError::ConnectionTimeout { timeout_ms: 5000 };
            let s = err.to_string();
            assert!(s.contains("5000"), "{s}");
        }

        #[test]
        fn pool_exhausted_display() {
            let err = TransportError::PoolExhausted {
                dcc_type: "maya".to_string(),
                max_connections: 4,
            };
            let s = err.to_string();
            assert!(s.contains("maya"), "{s}");
            assert!(s.contains('4'), "{s}");
        }

        #[test]
        fn acquire_timeout_display() {
            let err = TransportError::AcquireTimeout {
                dcc_type: "blender".to_string(),
                timeout_ms: 2000,
            };
            let s = err.to_string();
            assert!(s.contains("blender"), "{s}");
            assert!(s.contains("2000"), "{s}");
        }

        #[test]
        fn service_not_found_display() {
            let err = TransportError::ServiceNotFound {
                dcc_type: "houdini".to_string(),
                instance_id: "abc-123".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("houdini"), "{s}");
            assert!(s.contains("abc-123"), "{s}");
        }

        #[test]
        fn service_already_registered_display() {
            let err = TransportError::ServiceAlreadyRegistered {
                dcc_type: "maya".to_string(),
                instance_id: "xyz".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("maya"), "{s}");
            assert!(s.contains("xyz"), "{s}");
        }

        #[test]
        fn serialization_display() {
            let err = TransportError::Serialization("bad JSON".to_string());
            let s = err.to_string();
            assert!(s.contains("bad JSON"), "{s}");
        }

        #[test]
        fn io_error_display() {
            let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
            let err = TransportError::Io(io_err);
            let s = err.to_string();
            assert!(s.contains("pipe broken"), "{s}");
        }

        #[test]
        fn registry_file_display() {
            let err = TransportError::RegistryFile("no such file".to_string());
            let s = err.to_string();
            assert!(s.contains("no such file"), "{s}");
        }

        #[test]
        fn shutdown_display() {
            let err = TransportError::Shutdown;
            let s = err.to_string();
            assert!(s.contains("shut down"), "{s}");
        }

        #[test]
        fn session_not_found_display() {
            let err = TransportError::SessionNotFound {
                session_id: "sess-42".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("sess-42"), "{s}");
        }

        #[test]
        fn invalid_session_state_display() {
            let err = TransportError::InvalidSessionState {
                session_id: "s1".to_string(),
                state: "closed".to_string(),
                expected: "open".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("s1"), "{s}");
            assert!(s.contains("closed"), "{s}");
            assert!(s.contains("open"), "{s}");
        }

        #[test]
        fn reconnection_failed_display() {
            let err = TransportError::ReconnectionFailed {
                session_id: "s9".to_string(),
                retries: 3,
                reason: "no route".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("s9"), "{s}");
            assert!(s.contains('3'), "{s}");
            assert!(s.contains("no route"), "{s}");
        }

        #[test]
        fn ipc_connection_failed_display() {
            let err = TransportError::IpcConnectionFailed {
                address: "\\\\.\\pipe\\test".to_string(),
                reason: "access denied".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("access denied"), "{s}");
        }

        #[test]
        fn ipc_not_supported_display() {
            let err = TransportError::IpcNotSupported {
                transport: "unix_socket".to_string(),
                reason: "Windows only supports Named Pipes".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("unix_socket"), "{s}");
            assert!(s.contains("Windows"), "{s}");
        }

        #[test]
        fn frame_too_large_display() {
            let err = TransportError::FrameTooLarge {
                size: 1_048_576,
                max_size: 65_536,
            };
            let s = err.to_string();
            assert!(s.contains("1048576"), "{s}");
            assert!(s.contains("65536"), "{s}");
        }

        #[test]
        fn connection_closed_display() {
            let err = TransportError::ConnectionClosed;
            let s = err.to_string();
            assert!(s.contains("closed"), "{s}");
        }

        #[test]
        fn ping_timeout_display() {
            let err = TransportError::PingTimeout { timeout_ms: 30_000 };
            let s = err.to_string();
            assert!(s.contains("30000"), "{s}");
        }

        #[test]
        fn call_timeout_display() {
            let err = TransportError::CallTimeout {
                method: "render_frame".to_string(),
                timeout_ms: 60_000,
            };
            let s = err.to_string();
            assert!(s.contains("render_frame"), "{s}");
            assert!(s.contains("60000"), "{s}");
        }

        #[test]
        fn call_failed_display() {
            let err = TransportError::CallFailed {
                method: "create_node".to_string(),
                reason: "permission denied".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("create_node"), "{s}");
            assert!(s.contains("permission denied"), "{s}");
        }

        #[test]
        fn reconnect_failed_display() {
            let err = TransportError::ReconnectFailed {
                dcc_type: "unreal".to_string(),
                instance_id: Uuid::nil(),
                attempts: 5,
                last_error: "refused".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("unreal"), "{s}");
            assert!(s.contains('5'), "{s}");
            assert!(s.contains("refused"), "{s}");
        }

        #[test]
        fn internal_display() {
            let err = TransportError::Internal("unexpected state".to_string());
            let s = err.to_string();
            assert!(s.contains("unexpected state"), "{s}");
        }
    }

    mod test_constructors {
        use super::*;

        #[test]
        fn connection_refused_constructor() {
            let err = TransportError::connection_refused("localhost", 9000);
            match err {
                TransportError::ConnectionFailed { host, port, reason } => {
                    assert_eq!(host, "localhost");
                    assert_eq!(port, 9000);
                    assert!(reason.contains("refused"));
                }
                other => panic!("unexpected variant: {other:?}"),
            }
        }

        #[test]
        fn io_from_conversion() {
            let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
            let err: TransportError = io_err.into();
            assert!(matches!(err, TransportError::Io(_)));
        }
    }

    mod test_debug {
        use super::*;

        #[test]
        fn all_variants_are_debug() {
            // Smoke test: all variants must derive Debug without panicking.
            let variants: Vec<TransportError> = vec![
                TransportError::ConnectionFailed {
                    host: "h".to_string(),
                    port: 1,
                    reason: "r".to_string(),
                },
                TransportError::ConnectionTimeout { timeout_ms: 1 },
                TransportError::PoolExhausted {
                    dcc_type: "t".to_string(),
                    max_connections: 1,
                },
                TransportError::AcquireTimeout {
                    dcc_type: "t".to_string(),
                    timeout_ms: 1,
                },
                TransportError::ServiceNotFound {
                    dcc_type: "t".to_string(),
                    instance_id: "i".to_string(),
                },
                TransportError::ServiceAlreadyRegistered {
                    dcc_type: "t".to_string(),
                    instance_id: "i".to_string(),
                },
                TransportError::Serialization("e".to_string()),
                TransportError::Io(std::io::Error::new(std::io::ErrorKind::Other, "test")),
                TransportError::RegistryFile("f".to_string()),
                TransportError::Shutdown,
                TransportError::SessionNotFound {
                    session_id: "s".to_string(),
                },
                TransportError::InvalidSessionState {
                    session_id: "s".to_string(),
                    state: "a".to_string(),
                    expected: "b".to_string(),
                },
                TransportError::ReconnectionFailed {
                    session_id: "s".to_string(),
                    retries: 1,
                    reason: "r".to_string(),
                },
                TransportError::IpcConnectionFailed {
                    address: "a".to_string(),
                    reason: "r".to_string(),
                },
                TransportError::IpcNotSupported {
                    transport: "t".to_string(),
                    reason: "r".to_string(),
                },
                TransportError::FrameTooLarge {
                    size: 1,
                    max_size: 0,
                },
                TransportError::ConnectionClosed,
                TransportError::PingTimeout { timeout_ms: 1 },
                TransportError::CallTimeout {
                    method: "m".to_string(),
                    timeout_ms: 1,
                },
                TransportError::CallFailed {
                    method: "m".to_string(),
                    reason: "r".to_string(),
                },
                TransportError::ReconnectFailed {
                    dcc_type: "t".to_string(),
                    instance_id: Uuid::nil(),
                    attempts: 1,
                    last_error: "e".to_string(),
                },
                TransportError::Internal("msg".to_string()),
            ];

            for v in &variants {
                let debug = format!("{v:?}");
                assert!(!debug.is_empty());
            }
        }
    }
}
