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

    /// Generic internal error.
    #[error("{0}")]
    Internal(String),
}

/// Result type alias for transport operations.
pub type TransportResult<T> = Result<T, TransportError>;
