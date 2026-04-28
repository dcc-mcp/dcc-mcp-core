//! Local sidecar that bridges a DCC's MCP HTTP server to a public relay.
//!
//! Issue #504 ships in five PRs; this crate is the **client** half. The
//! current PR (#1 of 5) only lands configuration types and the reconnect
//! policy enum — the actual WebSocket loop and per-session multiplexer
//! land in PRs 2 and 3 respectively.
//!
//! See `dcc-mcp-tunnel-protocol` for the on-the-wire frame format and
//! `dcc-mcp-tunnel-relay` for the public-facing server.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod config;

pub use config::{AgentConfig, ReconnectPolicy};
