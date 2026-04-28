//! Long-running agent loop: connect → register → multiplex sessions.
//!
//! One agent process owns one [`AgentClient`]. The current MVP runs a
//! single registration attempt; the [`crate::ReconnectPolicy`] field on
//! [`crate::AgentConfig`] is wired through but the back-off loop itself
//! lands in PR 5 of #504. Tests drive [`run_once`] directly so they can
//! assert on the registration outcome without retry timing noise.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use dcc_mcp_tunnel_protocol::{
    CloseReason, Frame, RegisterAck, RegisterRequest, SessionId, frame::PROTOCOL_VERSION,
};

use crate::AgentConfig;
use crate::transport::{TransportError, read_frame, write_frame};

const SESSION_INBOX_QUEUE: usize = 32;
const READ_CHUNK: usize = 32 * 1024;

/// Outcome of a single registration attempt — surfaced to tests + the
/// future reconnect loop.
#[derive(Debug)]
pub struct Registered {
    /// Tunnel id minted by the relay.
    pub tunnel_id: String,
    /// Public URL the relay told us to advertise.
    pub public_url: Option<String>,
}

/// Run one full session: dial relay → register → multiplex → return on
/// disconnect. Errors are surfaced to the caller so the reconnect loop
/// (PR 5) can decide whether to retry.
pub async fn run_once(config: AgentConfig) -> Result<Registered, ClientError> {
    let stream = TcpStream::connect(&config.relay_url)
        .await
        .map_err(ClientError::Connect)?;
    let (mut reader, writer) = tokio::io::split(stream);
    let writer = Arc::new(tokio::sync::Mutex::new(writer));

    let req = RegisterRequest {
        protocol_version: PROTOCOL_VERSION,
        token: config.token.clone(),
        dcc: config.dcc.clone(),
        capabilities: config.capabilities.clone(),
        agent_version: config.agent_version.clone(),
    };
    {
        let mut w = writer.lock().await;
        write_frame(&mut *w, &Frame::Register(req)).await?;
    }

    let ack = match read_frame(&mut reader).await? {
        Some(Frame::RegisterAck(ack)) => ack,
        other => {
            return Err(ClientError::HandshakeProtocol(format!(
                "expected RegisterAck, got {other:?}"
            )));
        }
    };
    if !ack.ok {
        return Err(ClientError::Rejected(ack));
    }
    let tunnel_id = ack
        .tunnel_id
        .clone()
        .ok_or_else(|| ClientError::HandshakeProtocol("ack ok=true but tunnel_id None".into()))?;
    info!(%tunnel_id, "agent registered with relay");

    // Per-session inbound channels — populated when the relay sends
    // `OpenSession`; drained by the per-session bridging task.
    let sessions: Arc<DashMap<SessionId, mpsc::Sender<Vec<u8>>>> = Arc::new(DashMap::new());

    let local_target = config.local_target.clone();
    let writer_for_dispatch = Arc::clone(&writer);
    let sessions_for_dispatch = Arc::clone(&sessions);

    while let Some(frame) = read_frame(&mut reader).await? {
        match frame {
            Frame::OpenSession { session_id, .. } => {
                let (tx, rx) = mpsc::channel::<Vec<u8>>(SESSION_INBOX_QUEUE);
                sessions_for_dispatch.insert(session_id, tx);
                let local_target = local_target.clone();
                let writer = Arc::clone(&writer_for_dispatch);
                let sessions = Arc::clone(&sessions_for_dispatch);
                tokio::spawn(async move {
                    if let Err(e) = bridge_session(session_id, &local_target, rx, writer).await {
                        debug!(session_id, error = %e, "session bridge ended");
                    }
                    sessions.remove(&session_id);
                });
            }
            Frame::Data {
                session_id,
                payload,
            } => {
                if let Some(tx) = sessions_for_dispatch.get(&session_id).map(|s| s.clone()) {
                    if tx.send(payload).await.is_err() {
                        sessions_for_dispatch.remove(&session_id);
                    }
                }
            }
            Frame::CloseSession { session_id, .. } => {
                sessions_for_dispatch.remove(&session_id);
            }
            Frame::Heartbeat => {}
            Frame::Error { code, message, .. } => warn!(?code, %message, "relay reported error"),
            other => debug!(?other, "unexpected frame from relay; ignoring"),
        }
    }
    Ok(Registered {
        tunnel_id,
        public_url: ack.public_url,
    })
}

async fn bridge_session(
    session_id: SessionId,
    local_target: &str,
    mut inbox: mpsc::Receiver<Vec<u8>>,
    writer: Arc<tokio::sync::Mutex<tokio::io::WriteHalf<TcpStream>>>,
) -> Result<(), ClientError> {
    let backend = TcpStream::connect(local_target)
        .await
        .map_err(ClientError::Connect)?;
    let (mut backend_reader, mut backend_writer) = tokio::io::split(backend);

    // backend → relay: read local bytes, wrap in Data frames.
    let writer_for_relay = Arc::clone(&writer);
    let relay_pump = tokio::spawn(async move {
        let mut buf = vec![0u8; READ_CHUNK];
        loop {
            match backend_reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let mut w = writer_for_relay.lock().await;
                    if write_frame(
                        &mut *w,
                        &Frame::Data {
                            session_id,
                            payload: buf[..n].to_vec(),
                        },
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let mut w = writer_for_relay.lock().await;
        let _ = write_frame(
            &mut *w,
            &Frame::CloseSession {
                session_id,
                reason: CloseReason::BackendGone,
            },
        )
        .await;
    });

    // relay → backend: drain the per-session inbox.
    while let Some(bytes) = inbox.recv().await {
        if backend_writer.write_all(&bytes).await.is_err() {
            break;
        }
    }
    let _ = backend_writer.shutdown().await;
    let _ = relay_pump.await;
    Ok(())
}

/// Errors surfaced by [`run_once`].
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// TCP dial to the relay or local backend failed.
    #[error("connect: {0}")]
    Connect(std::io::Error),

    /// Frame I/O failure with the relay.
    #[error("transport: {0}")]
    Transport(#[from] TransportError),

    /// Handshake violated the wire protocol.
    #[error("handshake protocol: {0}")]
    HandshakeProtocol(String),

    /// Relay sent `RegisterAck { ok: false, .. }`.
    #[error("relay rejected registration: {0:?}")]
    Rejected(RegisterAck),
}
