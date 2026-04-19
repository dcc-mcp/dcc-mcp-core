//! dcc-mcp-transport: Async transport layer for the DCC-MCP ecosystem.
//!
//! Provides connection pooling, service discovery, session management,
//! IPC transport abstractions, and wire protocol support for communication
//! between MCP servers and DCC applications.
//!
//! ## IPC Transport
//!
//! The `ipc` module provides low-latency inter-process communication:
//! - **Named Pipes** (Windows): < 0.5ms latency, > 1GB/s throughput
//! - **Unix Domain Sockets** (macOS/Linux): < 0.1ms latency, > 1GB/s throughput
//! - **Automatic selection**: chooses the optimal transport based on platform and locality

pub mod channel;
pub mod circuit_breaker;
pub mod config;
pub mod connector;
pub mod dcc_link;
pub mod discovery;
pub mod error;
pub mod event_bridge;
pub mod framed;
pub mod ipc;
pub mod listener;
pub mod message;
pub mod pool;
pub mod python;
pub mod routing;
pub mod session;
pub mod transport;

// Re-export primary types
pub use channel::FramedChannel;
pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerRegistry, CircuitBreakerStats, CircuitState,
};
pub use config::{PoolConfig, SessionConfig, TransportConfig};
pub use connector::{IpcStream, connect};
pub use dcc_link::{
    DccLinkFrame, DccLinkType, GracefulIpcChannelAdapter, IpcChannelAdapter, SocketServerAdapter,
};
pub use discovery::ServiceRegistry;
pub use discovery::types::{ServiceEntry, ServiceKey, ServiceStatus};
pub use error::{TransportError, TransportResult};
pub use event_bridge::{EventBridge, EventBridgeService, NoopBridge};
pub use framed::FramedIo;
pub use ipc::{IpcConfig, PlatformCapabilities, TransportAddress, TransportScheme};
pub use listener::{IpcListener, ListenerHandle};
pub use message::{MessageEnvelope, Notification, Ping, Pong, Request, Response, ShutdownMessage};
pub use pool::{ActiveConnection, ConnectionPool, ConnectionState, PooledConnection};
pub use routing::{InstanceRouter, RoutingStrategy};
pub use session::{Session, SessionManager, SessionMetrics, SessionState};
pub use transport::TransportManager;

// Re-export Python bindings
#[cfg(feature = "python-bindings")]
pub use python::{
    PyDccLinkFrame, PyFramedChannel, PyGracefulIpcChannelAdapter, PyIpcChannelAdapter,
    PyIpcListener, PyListenerHandle, PyRoutingStrategy, PyServiceEntry, PyServiceStatus,
    PySocketServerAdapter, PyTransportAddress, PyTransportManager, PyTransportScheme,
    py_connect_ipc,
};
