//! Python bindings for the DCC-Link transport layer via PyO3.
//!
//! Exposes the core types and ipckit-backed adapters to Python:
//!
//! - [`types`] — `ServiceStatus`, `TransportAddress`, `TransportScheme`,
//!   `ServiceEntry`
//! - [`dcc_link`] — `DccLinkFrame`, `IpcChannelAdapter`,
//!   `GracefulIpcChannelAdapter`, `SocketServerAdapter`
//!
//! The legacy `TransportManager` / `FramedChannel` / `IpcListener` bindings
//! were removed in v0.14 (issue #251). Use the DccLink adapters for framed
//! per-connection messaging and `SocketServerAdapter` for multi-client
//! servers; use [`crate::discovery::FileRegistry`] directly via
//! `PyServiceEntry` for service registration.

pub mod dcc_link;
pub mod types;

#[cfg(feature = "python-bindings")]
pub use dcc_link::{
    PyDccLinkFrame, PyGracefulIpcChannelAdapter, PyIpcChannelAdapter, PySocketServerAdapter,
};

#[cfg(feature = "python-bindings")]
pub use types::{PyServiceEntry, PyServiceStatus, PyTransportAddress, PyTransportScheme};
