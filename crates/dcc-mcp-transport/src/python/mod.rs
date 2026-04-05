//! Python bindings for the transport layer via PyO3.
//!
//! Exposes `PyTransportManager`, `PyServiceEntry`, `PyServiceStatus`,
//! `PyRoutingStrategy`, `PyTransportAddress`, `PyTransportScheme`,
//! `PyIpcListener`, `PyListenerHandle`, and `PyFramedChannel` as Python classes.
//!
//! Async operations are bridged to synchronous calls via an internal Tokio runtime.
//!
//! ## Submodules
//!
//! - [`types`] — Python-facing enum/struct definitions
//! - [`manager`] — `PyTransportManager` implementation
//! - [`listener`] — `PyIpcListener` and `PyListenerHandle` implementation
//! - [`channel`] — `PyFramedChannel` implementation and `connect_ipc()` function
//! - [`helpers`] — internal conversion helpers

pub mod channel;
pub mod helpers;
pub mod listener;
pub mod manager;
pub mod types;

// Re-export everything for backward compatibility with the flat `python::*` path.

#[cfg(feature = "python-bindings")]
pub use channel::{PyFramedChannel, py_connect_ipc};

#[cfg(feature = "python-bindings")]
pub use listener::{PyIpcListener, PyListenerHandle};

#[cfg(feature = "python-bindings")]
pub use manager::PyTransportManager;

#[cfg(feature = "python-bindings")]
pub use types::{
    PyRoutingStrategy, PyServiceEntry, PyServiceStatus, PyTransportAddress, PyTransportScheme,
};
