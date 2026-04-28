//! Public-facing relay for the DCC-MCP zero-config remote-access tunnel.
//!
//! Issue #504 ships in five PRs; this crate is the **server** half. The
//! current PR (#1 of 5) only lands configuration types and the in-memory
//! tunnel registry — no network listeners yet. Subsequent PRs add the
//! control-plane WebSocket handler, the data-plane multiplexer, and the
//! frontend transports (WSS / TCP / HTTP-SSE).
//!
//! See `dcc-mcp-tunnel-protocol` for the on-the-wire frame format and
//! `dcc-mcp-tunnel-agent` for the local sidecar that registers here.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod config;
pub mod registry;

pub use config::RelayConfig;
pub use registry::{TunnelEntry, TunnelRegistry};
