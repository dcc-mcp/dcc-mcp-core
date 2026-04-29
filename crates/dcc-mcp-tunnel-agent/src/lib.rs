//! Local sidecar that bridges a DCC's MCP HTTP server to a public relay.
//!
//! Issue #504. Modules:
//!
//! - [`config`] — operator-supplied wiring (relay URL, JWT, local target).
//! - [`transport`] — async [`Frame`] I/O over `AsyncRead + AsyncWrite`.
//! - [`client`] — registration loop + per-session bridge to the local
//!   MCP HTTP server.
//! - [`reconnect`] — outer loop that re-establishes the relay leg with
//!   the configured back-off policy after a disconnect.
//!
//! See `dcc-mcp-tunnel-protocol` for the on-the-wire frame format and
//! `dcc-mcp-tunnel-relay` for the public-facing server.
//!
//! [`Frame`]: dcc_mcp_tunnel_protocol::Frame

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod client;
pub mod config;
pub mod reconnect;
pub mod transport;

pub use client::{ClientError, Registered, run_once};
pub use config::{AgentConfig, ReconnectPolicy};
pub use reconnect::{ReconnectExit, run_with_reconnect};
