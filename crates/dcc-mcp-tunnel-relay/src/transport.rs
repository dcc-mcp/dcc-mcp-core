//! Async [`Frame`] I/O over any `AsyncRead + AsyncWrite` transport.
//!
//! The MVP transport (issue #504) is plain TCP carrying the protocol
//! crate's length-prefixed msgpack codec. Every helper here operates on
//! `tokio::io` traits so a future PR that swaps TCP for a WebSocket leg
//! (tokio-tungstenite) only has to provide the adapter, not rewrite the
//! framing.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use dcc_mcp_tunnel_protocol::{Decoder, Frame, ProtocolError, codec};

/// Read one complete [`Frame`] from `reader`, blocking until enough bytes
/// arrive. Returns `Ok(None)` cleanly on EOF *between* frames; an EOF in
/// the middle of a partial frame surfaces as `Err`.
pub async fn read_frame<R>(reader: &mut R) -> Result<Option<Frame>, TransportError>
where
    R: AsyncRead + Unpin,
{
    // Read the 4-byte length prefix first. A clean EOF here means the peer
    // closed without owing us another frame.
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
    // Round-trip through `Decoder` so length-prefix + body parsing stays
    // identical to the unit-tested in-memory path.
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
        // The decoder cannot return `Ok(None)` here because we just fed it
        // the entire frame; this branch exists only to keep the match
        // exhaustive against future codec changes.
        None => Err(TransportError::Protocol(ProtocolError::Incomplete {
            needed: 4 + len as usize,
            have: 4 + len as usize,
        })),
    }
}

/// Encode `frame` and write it to `writer` in one shot. Flushes
/// immediately so the peer sees the frame even when the writer is buffered.
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
    /// Underlying socket I/O failed (closed, reset, write error).
    #[error("transport I/O: {0}")]
    Io(#[from] std::io::Error),

    /// Wire format violation (oversized frame, malformed msgpack, …).
    #[error("frame protocol: {0}")]
    Protocol(#[from] ProtocolError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_tunnel_protocol::{Frame, RegisterRequest, frame::PROTOCOL_VERSION};

    #[tokio::test]
    async fn round_trip_frame_over_duplex() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let frame = Frame::Register(RegisterRequest {
            protocol_version: PROTOCOL_VERSION,
            token: "tok".into(),
            dcc: "maya".into(),
            capabilities: vec![],
            agent_version: "test/0.0".into(),
        });
        write_frame(&mut a, &frame).await.unwrap();
        let got = read_frame(&mut b).await.unwrap().expect("frame");
        assert_eq!(got, frame);
    }

    #[tokio::test]
    async fn clean_eof_returns_none() {
        let (a, mut b) = tokio::io::duplex(1024);
        drop(a);
        let got = read_frame(&mut b).await.unwrap();
        assert!(got.is_none());
    }
}
