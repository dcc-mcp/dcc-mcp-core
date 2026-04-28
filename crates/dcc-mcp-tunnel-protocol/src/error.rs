//! Errors raised by the codec / auth layers.

use thiserror::Error;

/// Errors that can occur while encoding, decoding, or authenticating frames.
///
/// These are intentionally library-agnostic — neither variant exposes a
/// transport handle, so the same type can describe failures observed by the
/// agent (decoding the relay's reply) and by the relay (decoding the agent's
/// register request).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProtocolError {
    /// The frame body could not be msgpack-decoded into the expected variant.
    #[error("msgpack decode failed: {0}")]
    Decode(#[from] rmp_serde::decode::Error),

    /// The frame could not be msgpack-encoded.
    #[error("msgpack encode failed: {0}")]
    Encode(#[from] rmp_serde::encode::Error),

    /// The 4-byte length prefix declares a frame larger than
    /// [`crate::codec::MAX_FRAME_BYTES`]. This is a hard guard against
    /// length-prefix denial-of-service.
    #[error("frame length {0} exceeds maximum {1}")]
    FrameTooLarge(u32, u32),

    /// The buffer ran out before a complete frame could be read.
    #[error("not enough bytes: need {needed}, have {have}")]
    Incomplete {
        /// Total bytes the next frame requires (including its 4-byte prefix).
        needed: usize,
        /// Bytes currently available in the buffer.
        have: usize,
    },

    /// JWT signing or validation failed (expired, bad signature, malformed
    /// header, etc.). The wrapped [`jsonwebtoken::errors::Error`] carries the
    /// specific cause.
    #[error("jwt error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
}
