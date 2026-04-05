//! `DccCapture` trait — the platform-agnostic capture abstraction.
//!
//! All concrete backends implement this trait.  The high-level API
//! ([`crate::Capturer`]) selects the best available backend at runtime
//! and delegates to it via a boxed trait object.

use crate::error::CaptureResult;
use crate::types::{CaptureBackendKind, CaptureConfig, CaptureFrame};

// ── DccCapture trait ───────────────────────────────────────────────────────

/// Platform-agnostic screenshot / frame-capture interface.
///
/// Implementors should be `Send + Sync` so they can be shared across threads
/// (e.g. stored in `Arc<dyn DccCapture>`).
pub trait DccCapture: Send + Sync {
    /// Returns the backend identifier for diagnostics.
    fn backend_kind(&self) -> CaptureBackendKind;

    /// Capture a single frame according to `config`.
    ///
    /// Implementations are expected to honour `config.timeout_ms` and return
    /// [`CaptureError::Timeout`][crate::error::CaptureError::Timeout] when
    /// exceeded.
    fn capture(&self, config: &CaptureConfig) -> CaptureResult<CaptureFrame>;

    /// Returns `true` if this backend is available on the current system.
    ///
    /// This is a best-effort probe; some backends (e.g. DXGI) can only be
    /// confirmed available after a first successful capture.
    fn is_available(&self) -> bool;
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::mock::MockBackend;
    use crate::types::CaptureFormat;

    #[test]
    fn test_mock_backend_is_available() {
        let b = MockBackend::new(320, 240);
        assert!(b.is_available());
    }

    #[test]
    fn test_mock_backend_kind() {
        let b = MockBackend::new(1, 1);
        assert_eq!(b.backend_kind(), CaptureBackendKind::Mock);
    }

    #[test]
    fn test_capture_via_trait_object() {
        let backend: Box<dyn DccCapture> = Box::new(MockBackend::new(640, 480));
        let cfg = CaptureConfig::default();
        let frame = backend.capture(&cfg).unwrap();
        assert_eq!(frame.width, 640);
        assert_eq!(frame.height, 480);
    }

    #[test]
    fn test_capture_png_format() {
        let backend = MockBackend::new(100, 100);
        let cfg = CaptureConfig::builder().format(CaptureFormat::Png).build();
        let frame = backend.capture(&cfg).unwrap();
        assert_eq!(frame.format, CaptureFormat::Png);
        // PNG magic bytes: 0x89 50 4E 47
        assert!(frame.data.starts_with(b"\x89PNG"));
    }

    #[test]
    fn test_capture_jpeg_format() {
        let backend = MockBackend::new(100, 100);
        let cfg = CaptureConfig::builder()
            .format(CaptureFormat::Jpeg)
            .jpeg_quality(80)
            .build();
        let frame = backend.capture(&cfg).unwrap();
        assert_eq!(frame.format, CaptureFormat::Jpeg);
        // JPEG magic: 0xFF 0xD8
        assert!(frame.data.starts_with(&[0xFF, 0xD8]));
    }

    #[test]
    fn test_capture_raw_bgra_format() {
        let w = 8u32;
        let h = 8u32;
        let backend = MockBackend::new(w, h);
        let cfg = CaptureConfig::builder()
            .format(CaptureFormat::RawBgra)
            .build();
        let frame = backend.capture(&cfg).unwrap();
        assert_eq!(frame.format, CaptureFormat::RawBgra);
        // Raw BGRA: exactly w * h * 4 bytes
        assert_eq!(frame.data.len(), (w * h * 4) as usize);
    }
}
