//! dcc-mcp-transport: Async transport layer for the DCC-MCP ecosystem.
//!
//! Provides connection pooling, service discovery, session management,
//! and wire protocol support for communication between MCP servers and DCC applications.

pub mod config;
pub mod discovery;
pub mod error;
pub mod message;
pub mod pool;
pub mod python;
pub mod session;
pub mod transport;

// Re-export primary types
pub use config::{PoolConfig, SessionConfig, TransportConfig};
pub use discovery::ServiceRegistry;
pub use discovery::types::{ServiceEntry, ServiceKey, ServiceStatus};
pub use error::{TransportError, TransportResult};
pub use message::{Request, Response};
pub use pool::{ConnectionPool, ConnectionState, PooledConnection};
pub use session::{Session, SessionManager, SessionMetrics, SessionState};
pub use transport::TransportManager;

// Re-export Python bindings
#[cfg(feature = "python-bindings")]
pub use python::{PyServiceEntry, PyServiceStatus, PyTransportManager};
