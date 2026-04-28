//! Data-plane handler for one accepted **frontend** connection.
//!
//! The frontend listener accepts a TCP socket from a remote MCP client,
//! reads a small `select_tunnel` preamble (a length-prefixed `tunnel_id`
//! string, no msgpack), then bridges the socket to one multiplexed
//! session on the corresponding tunnel.
//!
//! Subsequent PRs add `/dcc/<name>/<id>` HTTP routing and a WS bridge;
//! this MVP keeps the protocol small enough to drive end-to-end with
//! `tokio::net::TcpStream` and validate the framing & multiplexing without
//! pulling in a full HTTP stack.

use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{debug, info, warn};

use dcc_mcp_tunnel_protocol::{CloseReason, Frame, TunnelId};

use crate::control::SESSION_INBOX_QUEUE;
use crate::registry::TunnelRegistry;

/// Maximum tunnel-id length accepted in the `select_tunnel` preamble.
/// Prevents an attacker from forcing a huge allocation by sending a giant
/// length prefix when no agent is connected.
pub const MAX_TUNNEL_ID_LEN: u16 = 128;

/// Bytes copied per backend-side read into a single `Frame::Data`. Sized
/// to fit comfortably inside the codec's 8 MiB ceiling and to keep
/// per-frame fixed overhead amortised.
const READ_CHUNK: usize = 32 * 1024;

/// Drive a freshly-accepted frontend connection: read the preamble,
/// allocate a session on the matching tunnel, then full-duplex copy bytes.
pub async fn handle_frontend<S>(stream: S, registry: Arc<TunnelRegistry>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, writer) = tokio::io::split(stream);

    let tunnel_id = match read_select_tunnel(&mut reader).await {
        Ok(id) => id,
        Err(e) => {
            debug!(error = %e, "frontend preamble read failed");
            return;
        }
    };
    let handle = match registry.get(&tunnel_id) {
        Some(entry) => Arc::clone(&entry.handle),
        None => {
            warn!(%tunnel_id, "frontend selected unknown tunnel");
            // Close immediately so the remote client surfaces the error.
            return;
        }
    };

    let (session_id, mut inbox_rx) = handle.open_session(SESSION_INBOX_QUEUE);
    debug!(%tunnel_id, session_id, "frontend session opened");

    if handle
        .send(Frame::OpenSession {
            session_id,
            client_info: None,
        })
        .await
        .is_err()
    {
        handle.close_session(session_id);
        return;
    }

    // agent → frontend: drain the per-session inbox into the writer.
    let writer_handle = Arc::clone(&handle);
    let writer_task = tokio::spawn(async move {
        let mut writer = writer;
        while let Some(bytes) = inbox_rx.recv().await {
            if writer.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
        // Tell the agent to tear down its half so the local backend
        // socket gets a clean EOF too.
        let _ = writer_handle
            .send(Frame::CloseSession {
                session_id,
                reason: CloseReason::ClientGone,
            })
            .await;
        writer_handle.close_session(session_id);
    });

    // frontend → agent: chunk the reader into `Data` frames.
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let payload = buf[..n].to_vec();
                if handle
                    .send(Frame::Data {
                        session_id,
                        payload,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(e) => {
                debug!(%tunnel_id, session_id, error = %e, "frontend read error");
                break;
            }
        }
    }
    let _ = handle
        .send(Frame::CloseSession {
            session_id,
            reason: CloseReason::ClientGone,
        })
        .await;
    handle.close_session(session_id);
    drop(handle);
    let _ = writer_task.await;
    info!(%tunnel_id, session_id, "frontend session closed");
}

/// Read the `select_tunnel` preamble: 2-byte BE length, then UTF-8 bytes.
async fn read_select_tunnel<R>(reader: &mut R) -> std::io::Result<TunnelId>
where
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 2];
    reader.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf);
    if len == 0 || len > MAX_TUNNEL_ID_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid tunnel-id length: {len}"),
        ));
    }
    let mut id_bytes = vec![0u8; len as usize];
    reader.read_exact(&mut id_bytes).await?;
    String::from_utf8(id_bytes).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("non-utf8 id: {e}"))
    })
}

/// Counterpart used by the agent test client / the eventual SDK helper.
pub async fn write_select_tunnel<W>(writer: &mut W, tunnel_id: &str) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let bytes = tunnel_id.as_bytes();
    let len = u16::try_from(bytes.len())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "tunnel id too long"))?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(bytes).await?;
    writer.flush().await?;
    Ok(())
}
