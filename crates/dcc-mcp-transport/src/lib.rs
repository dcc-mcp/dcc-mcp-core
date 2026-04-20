//! dcc-mcp-transport: DCC-Link transport layer built on top of `ipckit`.
//!
//! This crate provides the on-the-wire substrate for the DCC-MCP ecosystem:
//!
//! - **DCC-Link framing** — the
//!   `[u32 len][u8 type][u64 seq][msgpack body]` frame (see [`DccLinkFrame`]).
//! - **Async IPC transport** — [`IpcStream`]/[`IpcListener`] backed by
//!   [`ipckit::AsyncLocalSocketStream`]/[`AsyncLocalSocketListener`] for
//!   Named Pipes (Windows) and Unix Domain Sockets (macOS/Linux).
//! - **ipckit adapters** — [`IpcChannelAdapter`], [`GracefulIpcChannelAdapter`]
//!   and [`SocketServerAdapter`] provide the channel/server abstractions for
//!   DCC hosts.
//! - **Service discovery** — [`discovery::FileRegistry`] for sharing live
//!   DCC-server entries between the gateway and MCP HTTP servers.
//! - **Event bridging** — [`EventBridgeService`] bridges
//!   [`ipckit::EventStream`] into MCP `notifications/progress` and
//!   `notifications/cancelled` events.
//!
//! ## History
//!
//! Earlier revisions also shipped a hand-rolled framing/multiplexing stack
//! (`FramedIo`, `FramedChannel`, `TransportManager`, connection pool,
//! session manager, circuit breaker, `InstanceRouter`, `MessageEnvelope`).
//! That stack was removed in **v0.14** — see issue #251 — as part of the
//! migration to ipckit. Callers should use the DccLink adapters for per-
//! connection framing, `SocketServerAdapter` for multi-client servers, and
//! `FileRegistry` for discovery. There is **no backward-compatibility
//! shim**; the removed symbols are gone from the public API.
//!
//! ## Platform support
//!
//! | Transport      | Platform         | Typical latency | Throughput  |
//! |----------------|------------------|-----------------|-------------|
//! | TCP            | All              | ~10ms           | ~100MB/s    |
//! | Named Pipe     | Windows          | < 0.5ms         | > 1GB/s     |
//! | Unix Socket    | macOS / Linux    | < 0.1ms         | > 1GB/s     |

pub mod connector;
pub mod dcc_link;
pub mod discovery;
pub mod error;
pub mod event_bridge;
pub mod ipc;
pub mod listener;
pub mod python;

// Re-export primary types
pub use connector::{IpcStream, LocalSocketKind, MAX_FRAME_SIZE, connect};
pub use dcc_link::{
    DccLinkFrame, DccLinkType, GracefulIpcChannelAdapter, IpcChannelAdapter, SocketServerAdapter,
};
pub use discovery::ServiceRegistry;
pub use discovery::types::{ServiceEntry, ServiceKey, ServiceStatus};
pub use error::{TransportError, TransportResult};
pub use event_bridge::{EventBridge, EventBridgeService, NoopBridge};
pub use ipc::{IpcConfig, PlatformCapabilities, TransportAddress, TransportScheme};
pub use listener::{IpcListener, ListenerHandle};

// Re-export Python bindings
#[cfg(feature = "python-bindings")]
pub use python::{
    PyDccLinkFrame, PyGracefulIpcChannelAdapter, PyIpcChannelAdapter, PyServiceEntry,
    PyServiceStatus, PySocketServerAdapter, PyTransportAddress, PyTransportScheme,
};
