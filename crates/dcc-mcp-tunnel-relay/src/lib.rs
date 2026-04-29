//! Public-facing relay for the DCC-MCP zero-config remote-access tunnel.
//!
//! Issue #504. The MVP deliverables wired up here:
//!
//! - **Control plane** ([`control`]) — TCP agent listener, JWT validation,
//!   registration, heartbeat tracking, per-tunnel writer task.
//! - **Data plane** ([`data`]) — TCP frontend listener, `select_tunnel`
//!   preamble, per-session multiplexing, full-duplex byte forwarding.
//! - **Routing surface** ([`handle::TunnelHandle`]) — per-tunnel session
//!   allocator + bounded outbound frame queue.
//! - **Eviction** ([`eviction`]) — periodic sweeper that drops tunnels
//!   silent past `RelayConfig::stale_timeout`.
//! - **Server entry** ([`server::RelayServer`]) — binds both listeners,
//!   spawns the sweeper, exposes the resolved addresses.
//! - **Admin endpoint** ([`admin`]) — read-only `/tunnels` JSON listing
//!   plus `/healthz`, on a separate optional port.
//! - **WebSocket frontend** ([`ws_frontend`]) — accepts WS upgrades on
//!   `/tunnel/<id>` and bridges binary WS messages into the same per-
//!   session multiplexer used by the TCP frontend.
//!
//! See `dcc-mcp-tunnel-protocol` for the on-the-wire frame format and
//! `dcc-mcp-tunnel-agent` for the local sidecar that registers here.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod admin;
pub mod config;
pub mod control;
pub mod data;
pub mod eviction;
pub mod handle;
pub mod registry;
pub mod server;
pub mod transport;
pub mod ws_frontend;

pub use admin::TunnelSummary;
pub use config::RelayConfig;
pub use handle::TunnelHandle;
pub use registry::{TunnelEntry, TunnelRegistry};
pub use server::{OptionalBinds, RelayServer};
