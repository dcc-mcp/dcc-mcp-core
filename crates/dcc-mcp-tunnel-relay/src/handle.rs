//! Per-tunnel routing handle.
//!
//! Each accepted agent connection produces one [`TunnelHandle`], stored on
//! the matching [`crate::TunnelEntry`]. Two parties hold (cloned) `Arc`s
//! to it:
//!
//! - The **control-plane reader** (`crate::control`) writes inbound `Data`
//!   frames into the right session's inbox via [`TunnelHandle::session_inbox`].
//! - The **frontend listener** (`crate::data`) allocates a new session via
//!   [`TunnelHandle::open_session`] and pushes outbound bytes back to the
//!   agent through the shared `frame_tx`.
//!
//! All locks are short-lived; the per-tunnel writer task drains `frame_tx`
//! single-threaded so the agent socket itself is never contended.

use std::sync::atomic::{AtomicU32, Ordering};

use dashmap::DashMap;
use tokio::sync::mpsc;

use dcc_mcp_tunnel_protocol::{Frame, SessionId};

/// Bytes that arrived from the agent for one multiplexed session.
pub type AgentBytes = Vec<u8>;

/// Inbound channel handed to the frontend listener so it can read agent →
/// frontend bytes for a single session.
pub type SessionInboxRx = mpsc::Receiver<AgentBytes>;

/// Routing surface for one accepted tunnel.
#[derive(Debug)]
pub struct TunnelHandle {
    /// Outbound queue: anything pushed here is encoded by the per-tunnel
    /// writer task and sent to the agent. Bounded so a slow agent applies
    /// back-pressure to frontend producers (issue #504, hardening goal).
    frame_tx: mpsc::Sender<Frame>,

    /// Per-session inboxes — populated when the frontend opens a session
    /// and drained when the control-plane reader receives a `Data` frame.
    sessions: DashMap<SessionId, mpsc::Sender<AgentBytes>>,

    /// Monotonic session-id allocator. Wraps at `u32::MAX`; collisions
    /// with still-active sessions are statistically impossible at the
    /// MVP's per-tunnel session ceiling.
    next_session: AtomicU32,
}

impl TunnelHandle {
    /// Build a handle that forwards outbound frames into `frame_tx`.
    pub fn new(frame_tx: mpsc::Sender<Frame>) -> Self {
        Self {
            frame_tx,
            sessions: DashMap::new(),
            next_session: AtomicU32::new(1),
        }
    }

    /// Reserve a fresh session and return the inbound receiver the
    /// frontend listener should drain. The returned `SessionId` is stable
    /// for the lifetime of the session.
    pub fn open_session(&self, inbox_capacity: usize) -> (SessionId, SessionInboxRx) {
        let session_id = self.next_session.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(inbox_capacity);
        self.sessions.insert(session_id, tx);
        (session_id, rx)
    }

    /// Look up the inbound channel for a session. Returns `None` once
    /// `close_session` has been called or the receiver has been dropped.
    pub fn session_inbox(&self, id: SessionId) -> Option<mpsc::Sender<AgentBytes>> {
        self.sessions.get(&id).map(|s| s.clone())
    }

    /// Drop the per-session inbox so further `Data` frames for `id` are
    /// silently discarded. Called from both directions on `CloseSession`.
    pub fn close_session(&self, id: SessionId) {
        self.sessions.remove(&id);
    }

    /// Send a frame toward the agent. `Err` only when the writer task has
    /// already shut down (agent disconnected); callers treat this as a
    /// terminal condition for their session.
    pub async fn send(&self, frame: Frame) -> Result<(), mpsc::error::SendError<Frame>> {
        self.frame_tx.send(frame).await
    }

    /// Best-effort, non-blocking send. Used by the eviction sweeper which
    /// must not park behind a saturated agent.
    pub fn try_send(&self, frame: Frame) -> Result<(), mpsc::error::TrySendError<Frame>> {
        self.frame_tx.try_send(frame)
    }

    /// Number of currently-open sessions on this tunnel — used by
    /// `/tunnels` listings and by the eviction sweeper to skip tunnels
    /// that still have active traffic.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}
