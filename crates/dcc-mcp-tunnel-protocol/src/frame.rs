//! On-the-wire frames exchanged between agent and relay.
//!
//! The protocol multiplexes one or more MCP **sessions** across a single
//! WebSocket between a local DCC adapter and a public relay. Sessions are
//! identified by [`SessionId`]; tunnels (one DCC ↔ relay leg) by
//! [`TunnelId`]. The wire format is msgpack-serialised [`Frame`] enums
//! framed by [`crate::codec`]'s 4-byte length prefix.

use serde::{Deserialize, Serialize};

/// Bumped whenever a frame variant gains or loses a non-optional field, or
/// the JWT claim shape changes. The agent reports its supported version in
/// [`RegisterRequest::protocol_version`]; the relay rejects any value other
/// than its own with [`ErrorCode::ProtocolMismatch`].
pub const PROTOCOL_VERSION: u16 = 1;

/// Opaque tunnel identifier. Issued by the relay during registration.
///
/// Format is intentionally unspecified — agents must echo whatever the
/// relay returned in [`RegisterAck::tunnel_id`] without parsing it. In
/// practice the relay generates a URL-safe random token wide enough to
/// resist online enumeration (≥128 bits of entropy).
pub type TunnelId = String;

/// Per-session identifier *within* a single tunnel.
///
/// Stable for the lifetime of the multiplexed MCP session. Reused after a
/// graceful [`Frame::CloseSession`] only when the agent has acknowledged the
/// close — the relay never reuses an in-flight ID.
pub type SessionId = u32;

/// Why a session was torn down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    /// Remote client (browser tab, AI assistant) closed cleanly.
    ClientGone,
    /// Local backend (the DCC's MCP HTTP server) closed cleanly.
    BackendGone,
    /// Idle session evicted by the relay's TTL policy.
    IdleTimeout,
    /// The whole tunnel is shutting down.
    TunnelClosing,
    /// Unrecoverable error elsewhere — see the accompanying [`Frame::Error`].
    Error,
}

/// Recoverable error codes carried in [`Frame::Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCode {
    /// JWT missing, expired, or signed by an unknown key.
    AuthFailed,
    /// `RegisterRequest::protocol_version` not understood by the relay.
    ProtocolMismatch,
    /// The DCC type quoted in `RegisterRequest::dcc` is not in the JWT's
    /// allowed-DCC list.
    DccNotAllowed,
    /// Frame larger than the relay's configured ceiling.
    FrameTooLarge,
    /// Session ID referenced by a `Data` / `CloseSession` frame is unknown
    /// or already closed.
    UnknownSession,
    /// Catch-all for relay-side bugs the agent cannot react to.
    Internal,
}

/// Initial frame an agent sends to advertise itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRequest {
    /// Agent's protocol version. Must equal [`PROTOCOL_VERSION`] today.
    pub protocol_version: u16,

    /// Bearer JWT minted by [`crate::auth::issue`]. The relay calls
    /// [`crate::auth::validate`] before accepting the registration.
    pub token: String,

    /// DCC application running this agent (`"maya"`, `"houdini"`, …). Used
    /// for routing (`/dcc/<name>/<id>`) and visible in `/tunnels` listings.
    /// Must be in the JWT's `allowed_dcc` claim.
    pub dcc: String,

    /// Free-form capability tags forwarded to remote clients on connect.
    /// Examples: `"scene.mutate"`, `"usd"`, `"capture.window"`.
    pub capabilities: Vec<String>,

    /// Build-time identifier of the agent, e.g. `"dcc-mcp-tunnel-agent/0.1"`.
    /// Surfaced in `/tunnels` listings only — not used for routing.
    pub agent_version: String,
}

/// Relay's reply to [`RegisterRequest`]. Carried in [`Frame::RegisterAck`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterAck {
    /// `true` ⇔ the relay accepted the registration; `false` triggers an
    /// immediate disconnect after this frame is sent.
    pub ok: bool,

    /// Tunnel ID assigned by the relay. The agent uses this for logging and
    /// when reconnecting after a transient network drop. `None` on failure.
    pub tunnel_id: Option<TunnelId>,

    /// Public URL the agent can advertise to its operator (e.g.
    /// `wss://relay.example.com/tunnel/abc123`). `None` on failure.
    pub public_url: Option<String>,

    /// Set on `ok = false` to explain the rejection.
    pub error_code: Option<ErrorCode>,

    /// Optional human-readable diagnostic appended by the relay.
    pub message: Option<String>,
}

/// All frames carried over the agent ↔ relay WebSocket. Serde-tagged on
/// `t` so adding a variant in a future protocol revision is forward
/// compatible (older parties see `untagged` decoding fail and report
/// [`ErrorCode::ProtocolMismatch`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Frame {
    /// Agent → relay: present credentials and metadata.
    Register(RegisterRequest),

    /// Relay → agent: accept or reject the registration.
    RegisterAck(RegisterAck),

    /// Agent → relay: keep-alive ping. Cadence is configured by the agent
    /// (issue #504 design: ~10 s); the relay evicts tunnels silent longer
    /// than its own `stale_timeout_secs` (default 30 s).
    Heartbeat,

    /// Relay → agent: a remote client connected and was assigned this
    /// session_id. The agent should open a downstream link to its local
    /// MCP server.
    OpenSession {
        /// Per-tunnel unique session identifier.
        session_id: SessionId,
        /// Optional client-side hint: User-Agent, source IP, etc. Not parsed
        /// by the agent — passed through to telemetry only.
        client_info: Option<String>,
    },

    /// Either direction: graceful teardown of a single multiplexed session.
    /// The other end must respond with its own `CloseSession` once it has
    /// drained the in-flight `Data` frames.
    CloseSession {
        /// Session being closed.
        session_id: SessionId,
        /// Why.
        reason: CloseReason,
    },

    /// Bidirectional payload for one multiplexed session.
    Data {
        /// Session this byte chunk belongs to.
        session_id: SessionId,
        /// Opaque payload — the relay never inspects MCP message bodies.
        payload: Vec<u8>,
    },

    /// Recoverable error. Receiver may continue using other sessions on the
    /// same tunnel.
    Error {
        /// Session the error pertains to, if any.
        session_id: Option<SessionId>,
        /// Machine-readable code.
        code: ErrorCode,
        /// Human-readable detail. Logged but never echoed to remote clients.
        message: String,
    },
}
