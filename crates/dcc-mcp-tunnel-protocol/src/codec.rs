//! Length-prefixed msgpack frame codec.
//!
//! Wire format (big-endian):
//!
//! ```text
//!  ┌──────────────┬───────────────────────────┐
//!  │  4-byte len  │  msgpack body (Frame)     │
//!  │  (u32 BE)    │  len bytes                │
//!  └──────────────┴───────────────────────────┘
//! ```
//!
//! The 4-byte prefix is the body length **only** — it does not include
//! the prefix itself. A complete frame on the wire occupies `4 + len`
//! bytes.
//!
//! This module is transport-agnostic: callers feed it `Vec<u8>` and `&[u8]`
//! buffers. The agent and relay both wrap the same primitives around their
//! WebSocket message handlers.

use crate::error::ProtocolError;
use crate::frame::Frame;

/// Hard upper bound on a single frame's body size (8 MiB).
///
/// Larger payloads must be chunked into multiple [`Frame::Data`] frames by
/// the producer. This guard exists primarily to bound a malicious peer's
/// allocation when an oversized length prefix is read off the wire.
pub const MAX_FRAME_BYTES: u32 = 8 * 1024 * 1024;

/// Serialise a single frame into a fresh `Vec<u8>` ready to write to a
/// transport. The returned buffer always starts with the 4-byte length
/// prefix, so it can be passed straight to `write_all`.
pub fn encode(frame: &Frame) -> Result<Vec<u8>, ProtocolError> {
    let body = rmp_serde::to_vec_named(frame)?;
    let len = u32::try_from(body.len())
        .map_err(|_| ProtocolError::FrameTooLarge(u32::MAX, MAX_FRAME_BYTES))?;
    if len > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(len, MAX_FRAME_BYTES));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decode a single frame from `buf`. The buffer must start with the
/// 4-byte length prefix. Returns `(frame, bytes_consumed)` so the caller
/// can advance its read cursor.
///
/// For streaming consumers that build up partial frames across multiple
/// reads, use [`Decoder`] instead.
pub fn decode(buf: &[u8]) -> Result<(Frame, usize), ProtocolError> {
    if buf.len() < 4 {
        return Err(ProtocolError::Incomplete {
            needed: 4,
            have: buf.len(),
        });
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if len > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(len, MAX_FRAME_BYTES));
    }
    let total = 4usize.saturating_add(len as usize);
    if buf.len() < total {
        return Err(ProtocolError::Incomplete {
            needed: total,
            have: buf.len(),
        });
    }
    let frame: Frame = rmp_serde::from_slice(&buf[4..total])?;
    Ok((frame, total))
}

/// Streaming decoder that buffers partial reads from a transport.
///
/// Push bytes via [`Decoder::extend`] as they arrive, then call
/// [`Decoder::next_frame`] in a loop until it returns `Ok(None)`. The
/// buffer compacts itself after each successful pop.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    /// New empty decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append freshly read bytes from the transport.
    pub fn extend(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// Pop the next complete frame, if one is available. Returns `Ok(None)`
    /// when the buffer holds only a partial frame.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, ProtocolError> {
        match decode(&self.buf) {
            Ok((frame, consumed)) => {
                self.buf.drain(..consumed);
                Ok(Some(frame))
            }
            Err(ProtocolError::Incomplete { .. }) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// Read-only view of the unconsumed bytes — useful in tests and panic
    /// reports.
    pub fn pending(&self) -> &[u8] {
        &self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{
        CloseReason, ErrorCode, Frame, PROTOCOL_VERSION, RegisterAck, RegisterRequest,
    };

    fn sample_register() -> Frame {
        Frame::Register(RegisterRequest {
            protocol_version: PROTOCOL_VERSION,
            token: "header.payload.signature".into(),
            dcc: "maya".into(),
            capabilities: vec!["scene.read".into(), "usd".into()],
            agent_version: "dcc-mcp-tunnel-agent/0.1".into(),
        })
    }

    #[test]
    fn round_trip_register() {
        let bytes = encode(&sample_register()).unwrap();
        let (decoded, n) = decode(&bytes).unwrap();
        assert_eq!(n, bytes.len());
        assert_eq!(decoded, sample_register());
    }

    #[test]
    fn round_trip_data() {
        let frame = Frame::Data {
            session_id: 42,
            payload: vec![1, 2, 3, 4, 5],
        };
        let bytes = encode(&frame).unwrap();
        let (decoded, _) = decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn round_trip_register_ack_with_message() {
        let frame = Frame::RegisterAck(RegisterAck {
            ok: false,
            tunnel_id: None,
            public_url: None,
            error_code: Some(ErrorCode::DccNotAllowed),
            message: Some("token only allows: houdini, blender".into()),
        });
        let bytes = encode(&frame).unwrap();
        let (decoded, _) = decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn round_trip_close_session() {
        let frame = Frame::CloseSession {
            session_id: 7,
            reason: CloseReason::IdleTimeout,
        };
        let bytes = encode(&frame).unwrap();
        let (decoded, _) = decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn streaming_decoder_handles_split_frames() {
        let bytes = encode(&sample_register()).unwrap();
        let (a, b) = bytes.split_at(7); // arbitrary mid-frame split
        let mut dec = Decoder::new();
        dec.extend(a);
        assert!(matches!(dec.next_frame(), Ok(None)));
        dec.extend(b);
        let popped = dec.next_frame().unwrap().expect("complete frame");
        assert_eq!(popped, sample_register());
        assert!(dec.pending().is_empty());
    }

    #[test]
    fn rejects_oversized_length_prefix() {
        let mut bytes = vec![0u8; 4];
        bytes[..4].copy_from_slice(&(MAX_FRAME_BYTES + 1).to_be_bytes());
        match decode(&bytes) {
            Err(ProtocolError::FrameTooLarge(got, max)) => {
                assert_eq!(got, MAX_FRAME_BYTES + 1);
                assert_eq!(max, MAX_FRAME_BYTES);
            }
            other => panic!("expected FrameTooLarge, got {other:?}"),
        }
    }
}
