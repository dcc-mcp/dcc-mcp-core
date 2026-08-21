//! Wire protocol shared by `dcc-mcp-tunnel-relay` and `dcc-mcp-tunnel-agent`.
//!
//! This crate is intentionally network-free: it defines the on-the-wire
//! [`Frame`] enum used to multiplex one or more MCP sessions across a single
//! WebSocket between a local DCC and a public relay (issue #504), the binary
//! length-prefixed [`codec`] that serialises those frames with msgpack, and
//! the [`auth`] module that issues + validates the bearer JWTs the relay
//! uses to gate registration.
//!
//! Splitting these primitives out lets:
//!
//! - the **relay** (server, `dcc-mcp-tunnel-relay`) pull them in for inbound
//!   parsing without having to depend on the agent's reconnect loop;
//! - the **agent** (local sidecar, `dcc-mcp-tunnel-agent`) reuse the same
//!   types when sending without forking the schema;
//! - external tooling — telemetry probes, Wireshark dissectors, third-party
//!   relay implementations — link against a tiny runtime-free default build
//!   to inspect tunnel traffic.
//!
//! The default feature set has no tokio, no `bytes`, and no async machinery.
//! Enable `tokio-io` to share the canonical async frame reader and writer used
//! by the agent and relay. The codec itself operates on `Vec<u8>` and `&[u8]`
//! so it round-trips cleanly under `#[test]` without a runtime.
//!
//! # Quick orientation
//!
//! | Need | Use |
//! |---|---|
//! | Build a tunnel control / data frame | [`Frame`] variants + [`codec::encode`] |
//! | Parse one frame from a buffer | [`codec::decode`] / [`codec::Decoder`] |
//! | Mint a bearer token for an agent | [`auth::issue`] |
//! | Validate an inbound `Authorization: Bearer …` | [`auth::validate`] |
//! | Frame protocol version (negotiated in `Register`) | [`PROTOCOL_VERSION`] |

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod auth;
pub mod codec;
pub mod error;
pub mod frame;
#[cfg(feature = "tokio-io")]
pub mod tokio_io;

pub use auth::{TunnelClaims, issue, validate};
pub use codec::{Decoder, MAX_FRAME_BYTES, decode, encode};
pub use error::ProtocolError;
pub use frame::{
    CloseReason, ErrorCode, Frame, PROTOCOL_VERSION, RegisterAck, RegisterRequest, SessionId,
    TunnelId,
};
#[cfg(feature = "tokio-io")]
pub use tokio_io::FrameIoError;
