//! Tokio-based I/O for the tunnel frame protocol.
//!
//! This module is available with the `tokio-io` feature. It is the single
//! owner of async frame transport shared by the tunnel agent and relay.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Decoder, Frame, ProtocolError, codec};

/// Read one complete [`Frame`] from `reader`.
///
/// A clean EOF between frames returns `Ok(None)`; an EOF or protocol error
/// within a frame is returned as [`FrameIoError`].
pub async fn read_frame<R>(reader: &mut R) -> Result<Option<Frame>, FrameIoError>
where
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0_u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(FrameIoError::Io(error)),
    }

    let len = u32::from_be_bytes(len_buf);
    if len > codec::MAX_FRAME_BYTES {
        return Err(FrameIoError::Protocol(ProtocolError::FrameTooLarge(
            len,
            codec::MAX_FRAME_BYTES,
        )));
    }

    let mut body = vec![0_u8; len as usize];
    reader.read_exact(&mut body).await?;

    let mut decoder = Decoder::new();
    decoder.extend(&len_buf);
    decoder.extend(&body);
    match decoder.next_frame()? {
        Some(frame) => Ok(Some(frame)),
        None => Err(FrameIoError::Protocol(ProtocolError::Incomplete {
            needed: 4 + len as usize,
            have: 4 + len as usize,
        })),
    }
}

/// Encode `frame`, write it to `writer`, and flush the writer.
pub async fn write_frame<W>(writer: &mut W, frame: &Frame) -> Result<(), FrameIoError>
where
    W: AsyncWrite + Unpin,
{
    let bytes = codec::encode(frame)?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

/// Errors produced while reading or writing a tunnel protocol frame.
#[must_use]
#[derive(Debug, thiserror::Error)]
pub enum FrameIoError {
    /// Underlying async stream I/O failed.
    #[error("tunnel frame I/O: {0}")]
    Io(#[from] std::io::Error),

    /// The length-prefixed tunnel frame violated the wire protocol.
    #[error("tunnel frame protocol: {0}")]
    Protocol(#[from] ProtocolError),
}
