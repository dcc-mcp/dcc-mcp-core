//! Local sidecar that bridges a DCC's MCP HTTP server to a public relay.
//!
//! Issue #504. Modules:
//!
//! - [`config`] — operator-supplied wiring (relay URL, JWT, local target).
//! - [`transport`] — async [`Frame`] I/O over `AsyncRead + AsyncWrite`.
//! - [`client`] — registration loop + per-session bridge to the local
//!   MCP HTTP server.
//!
//! See `dcc-mcp-tunnel-protocol` for the on-the-wire frame format and
//! `dcc-mcp-tunnel-relay` for the public-facing server.
//!
//! [`Frame`]: dcc_mcp_tunnel_protocol::Frame

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod client;
pub mod config;
pub mod transport;

pub use client::{ClientError, Registered, run_once};
pub use config::{AgentConfig, ReconnectPolicy};
