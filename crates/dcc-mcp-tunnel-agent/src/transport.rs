//! Async [`Frame`] I/O over an `AsyncRead + AsyncWrite` transport.
//!
//! Mirrors `dcc_mcp_tunnel_relay::transport` so the agent does not have
//! to depend on the relay crate just for the framing helpers.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use dcc_mcp_tunnel_protocol::{Decoder, Frame, ProtocolError, codec};

/// Read one complete [`Frame`] from `reader`. Returns `Ok(None)` cleanly
/// on EOF *between* frames; an EOF mid-frame surfaces as `Err`.
pub async fn read_frame<R>(reader: &mut R) -> Result<Option<Frame>, TransportError>
where
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(TransportError::Io(e)),
    }
    let len = u32::from_be_bytes(len_buf);
    if len > codec::MAX_FRAME_BYTES {
        return Err(TransportError::Protocol(ProtocolError::FrameTooLarge(
            len,
            codec::MAX_FRAME_BYTES,
        )));
    }
    let mut body = vec![0u8; len as usize];
    reader
        .read_exact(&mut body)
        .await
        .map_err(TransportError::Io)?;
    let mut dec = Decoder::new();
    dec.extend(&len_buf);
    dec.extend(&body);
    match dec.next_frame()? {
        Some(frame) => Ok(Some(frame)),
        None => Err(TransportError::Protocol(ProtocolError::Incomplete {
            needed: 4 + len as usize,
            have: 4 + len as usize,
        })),
    }
}

/// Encode `frame` and write it to `writer` in one shot, with a flush.
pub async fn write_frame<W>(writer: &mut W, frame: &Frame) -> Result<(), TransportError>
where
    W: AsyncWrite + Unpin,
{
    let bytes = codec::encode(frame)?;
    writer.write_all(&bytes).await.map_err(TransportError::Io)?;
    writer.flush().await.map_err(TransportError::Io)?;
    Ok(())
}

/// Errors surfaced by the async frame I/O helpers.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Underlying socket I/O failed.
    #[error("transport I/O: {0}")]
    Io(#[from] std::io::Error),

    /// Wire format violation.
    #[error("frame protocol: {0}")]
    Protocol(#[from] ProtocolError),
}
