//! Control-plane handler: validates an agent registration and pumps
//! frames in both directions between the agent socket and the routing
//! [`crate::handle::TunnelHandle`].
//!
//! Wire-level shape (per accepted agent connection):
//!
//! 1. Read first frame; reject anything other than `Register`.
//! 2. Validate JWT, protocol version, DCC scope, capacity.
//! 3. Mint `tunnel_id`, build `(TunnelHandle, frame_rx)` pair, register.
//! 4. Send `RegisterAck { ok: true, tunnel_id, public_url }`.
//! 5. Split the socket; spawn a writer task draining `frame_rx`.
//! 6. Read loop: dispatch `Heartbeat` / `Data` / `CloseSession` / `Error`.
//! 7. On EOF or error, evict from the registry, drop the handle so
//!    `frame_rx` closes, and let the writer task wind down.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use dcc_mcp_tunnel_protocol::{
    ErrorCode, Frame, RegisterAck, RegisterRequest, auth, frame::PROTOCOL_VERSION,
};

use crate::config::RelayConfig;
use crate::handle::TunnelHandle;
use crate::registry::{TunnelEntry, TunnelRegistry};
use crate::transport::{TransportError, read_frame, write_frame};

/// Bound on the agent's outbound queue. Picked to absorb a few jumbo
/// `Data` frames (8 MiB cap each) without unbounded growth, while still
/// applying back-pressure to over-eager frontend clients.
pub const AGENT_FRAME_QUEUE: usize = 32;

/// Bound on each per-session inbox handed back to a frontend client.
pub const SESSION_INBOX_QUEUE: usize = 32;

/// Drive a freshly-accepted agent connection through registration and
/// then loop on its read half until disconnect. Errors are logged, never
/// propagated — the relay treats per-tunnel failures as routine churn.
pub async fn handle_agent<S>(stream: S, registry: Arc<TunnelRegistry>, config: Arc<RelayConfig>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);

    let req = match read_frame(&mut reader).await {
        Ok(Some(Frame::Register(req))) => req,
        Ok(Some(other)) => {
            let _ = reject(
                &mut writer,
                ErrorCode::ProtocolMismatch,
                format!("expected Register first, got {other:?}"),
            )
            .await;
            return;
        }
        Ok(None) => {
            debug!("agent closed before sending Register");
            return;
        }
        Err(e) => {
            warn!(error = %e, "failed to read Register frame");
            return;
        }
    };

    let accepted = match accept(&mut writer, &req, &registry, &config).await {
        Some(acc) => acc,
        None => return,
    };
    let Accepted {
        tunnel_id,
        handle,
        mut frame_rx,
    } = accepted;
    info!(%tunnel_id, dcc = %req.dcc, "tunnel registered");

    let writer_task = {
        let tunnel_id = tunnel_id.clone();
        tokio::spawn(async move {
            while let Some(frame) = frame_rx.recv().await {
                if let Err(e) = write_frame(&mut writer, &frame).await {
                    warn!(%tunnel_id, error = %e, "agent writer failed");
                    return;
                }
            }
            let _ = writer.shutdown().await;
        })
    };

    if let Err(e) = read_loop(&mut reader, &handle, &registry, &tunnel_id).await {
        debug!(%tunnel_id, error = %e, "agent read loop terminated");
    }

    // Dropping our `Arc<TunnelHandle>` and removing the registry row drops
    // every `frame_tx` clone, which closes `frame_rx` and lets the writer
    // task return cleanly.
    drop(handle);
    let removed = registry.remove(&tunnel_id);
    let _ = writer_task.await;
    if removed.is_some() {
        info!(%tunnel_id, "tunnel evicted");
    }
}

struct Accepted {
    tunnel_id: String,
    handle: Arc<TunnelHandle>,
    frame_rx: mpsc::Receiver<Frame>,
}

async fn accept<W>(
    writer: &mut W,
    req: &RegisterRequest,
    registry: &Arc<TunnelRegistry>,
    config: &Arc<RelayConfig>,
) -> Option<Accepted>
where
    W: AsyncWrite + Unpin,
{
    if req.protocol_version != PROTOCOL_VERSION {
        let _ = reject(
            writer,
            ErrorCode::ProtocolMismatch,
            format!(
                "agent protocol_version={} relay={}",
                req.protocol_version, PROTOCOL_VERSION
            ),
        )
        .await;
        return None;
    }
    let claims = match auth::validate(&req.token, &config.jwt_secret) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, dcc = %req.dcc, "agent token rejected");
            let _ = reject(writer, ErrorCode::AuthFailed, e.to_string()).await;
            return None;
        }
    };
    if !claims.allowed_dcc.is_empty() && !claims.allowed_dcc.iter().any(|d| d == &req.dcc) {
        let _ = reject(
            writer,
            ErrorCode::DccNotAllowed,
            format!("token only allows: {}", claims.allowed_dcc.join(", ")),
        )
        .await;
        return None;
    }
    if config.max_tunnels != 0 && registry.len() >= config.max_tunnels {
        let _ = reject(
            writer,
            ErrorCode::Internal,
            format!("relay at capacity ({} tunnels)", config.max_tunnels),
        )
        .await;
        return None;
    }
    let tunnel_id = Uuid::new_v4().simple().to_string();
    let public_url = format!("{}/tunnel/{}", config.base_url, tunnel_id);
    let (frame_tx, frame_rx) = mpsc::channel::<Frame>(AGENT_FRAME_QUEUE);
    let handle = Arc::new(TunnelHandle::new(frame_tx));
    let entry = TunnelEntry {
        tunnel_id: tunnel_id.clone(),
        dcc: req.dcc.clone(),
        capabilities: req.capabilities.clone(),
        agent_version: req.agent_version.clone(),
        registered_at: Instant::now(),
        last_heartbeat: RwLock::new(Instant::now()),
        handle: Arc::clone(&handle),
    };
    registry.insert(entry);
    let ack = Frame::RegisterAck(RegisterAck {
        ok: true,
        tunnel_id: Some(tunnel_id.clone()),
        public_url: Some(public_url),
        error_code: None,
        message: None,
    });
    if let Err(e) = write_frame(writer, &ack).await {
        warn!(error = %e, "failed to write RegisterAck");
        registry.remove(&tunnel_id);
        return None;
    }
    Some(Accepted {
        tunnel_id,
        handle,
        frame_rx,
    })
}

async fn reject<W>(writer: &mut W, code: ErrorCode, message: String) -> Result<(), TransportError>
where
    W: AsyncWrite + Unpin,
{
    let frame = Frame::RegisterAck(RegisterAck {
        ok: false,
        tunnel_id: None,
        public_url: None,
        error_code: Some(code),
        message: Some(message),
    });
    write_frame(writer, &frame).await?;
    let _ = writer.shutdown().await;
    Ok(())
}

async fn read_loop<R>(
    reader: &mut R,
    handle: &Arc<TunnelHandle>,
    registry: &Arc<TunnelRegistry>,
    tunnel_id: &str,
) -> Result<(), TransportError>
where
    R: AsyncRead + Unpin,
{
    while let Some(frame) = read_frame(reader).await? {
        match frame {
            Frame::Heartbeat => {
                if let Some(entry) = registry.get(&tunnel_id.to_string()) {
                    entry.touch();
                }
            }
            Frame::Data {
                session_id,
                payload,
            } => {
                if let Some(inbox) = handle.session_inbox(session_id) {
                    if inbox.send(payload).await.is_err() {
                        handle.close_session(session_id);
                    }
                }
            }
            Frame::CloseSession { session_id, .. } => {
                handle.close_session(session_id);
            }
            Frame::Error { code, message, .. } => {
                warn!(%tunnel_id, ?code, %message, "agent reported error");
            }
            other => debug!(%tunnel_id, ?other, "unexpected frame from agent; ignoring"),
        }
    }
    Ok(())
}
