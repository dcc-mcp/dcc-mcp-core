//! Error types for `dcc-mcp-capture`.

use thiserror::Error;

/// All errors that can be produced by the capture subsystem.
#[derive(Debug, Error)]
pub enum CaptureError {
    /// The requested capture backend is not supported on this platform.
    #[error("capture backend not supported: {0}")]
    BackendNotSupported(String),

    /// A platform-specific OS / API error occurred.
    #[error("platform error: {0}")]
    Platform(String),

    /// The target window or process could not be found.
    #[error("target not found: {0}")]
    TargetNotFound(String),

    /// Image encoding or decoding failed.
    #[error("image error: {0}")]
    Image(String),

    /// The requested output format is not supported.
    #[error("unsupported format: {0:?}")]
    UnsupportedFormat(String),

    /// The capture configuration is invalid.
    #[error("invalid config: {0}")]
    InvalidConfig(String),

    /// The capture operation timed out.
    #[error("capture timed out after {0}ms")]
    Timeout(u64),

    /// An unexpected internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Convenience alias for `Result<T, CaptureError>`.
pub type CaptureResult<T> = Result<T, CaptureError>;

// ── Conversions from platform error types ──────────────────────────────────

impl From<image::ImageError> for CaptureError {
    fn from(e: image::ImageError) -> Self {
        CaptureError::Image(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_backend_not_supported() {
        let e = CaptureError::BackendNotSupported("DXGI".to_string());
        assert!(e.to_string().contains("DXGI"));
    }

    #[test]
    fn test_error_display_target_not_found() {
        let e = CaptureError::TargetNotFound("Maya pid=1234".to_string());
        assert!(e.to_string().contains("1234"));
    }

    #[test]
    fn test_error_display_timeout() {
        let e = CaptureError::Timeout(5000);
        assert!(e.to_string().contains("5000"));
    }

    #[test]
    fn test_error_from_image_error() {
        use std::io::Cursor;
        // Craft an invalid PNG to trigger an image error.
        let bad_data = b"not a png";
        let err = image::load(Cursor::new(bad_data), image::ImageFormat::Png).unwrap_err();
        let capture_err: CaptureError = err.into();
        assert!(matches!(capture_err, CaptureError::Image(_)));
    }
}
