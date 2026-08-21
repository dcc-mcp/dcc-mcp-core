//! Compatibility exports for shared tunnel frame I/O.

pub use dcc_mcp_tunnel_protocol::tokio_io::{FrameIoError, read_frame, write_frame};

/// Historical name for tunnel frame I/O errors.
#[deprecated(
    since = "0.20.9",
    note = "use FrameIoError; TransportError is reserved for the general transport layer"
)]
pub type TransportError = FrameIoError;
