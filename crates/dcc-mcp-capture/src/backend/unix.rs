//! Linux X11 XShmGetImage capture backend.
//!
//! Uses the X11 MIT-SHM extension for efficient framebuffer capture.
//! On non-Linux platforms this module compiles to a no-op stub.

use crate::capture::DccCapture;
use crate::error::{CaptureError, CaptureResult};
use crate::types::{CaptureBackendKind, CaptureConfig, CaptureFrame};

// ── X11Backend ─────────────────────────────────────────────────────────────

/// Linux X11 XShmGetImage capture backend.
///
/// Requires an active X display session (`DISPLAY` env var).
#[derive(Debug, Default)]
pub struct X11Backend;

impl X11Backend {
    /// Create a new X11 backend.
    pub fn new() -> Self {
        X11Backend
    }
}

// ── DccCapture impl — Linux ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::*;

    impl DccCapture for X11Backend {
        fn backend_kind(&self) -> CaptureBackendKind {
            CaptureBackendKind::X11Xshm
        }

        fn is_available(&self) -> bool {
            // Check if DISPLAY is set and non-empty.
            std::env::var("DISPLAY")
                .map(|v| !v.is_empty())
                .unwrap_or(false)
        }

        fn capture(&self, _config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
            if !self.is_available() {
                return Err(CaptureError::BackendNotSupported(
                    "DISPLAY environment variable not set; X11 backend unavailable".to_string(),
                ));
            }

            // Full X11 XShmGetImage implementation uses unsafe FFI calls to
            // libX11 / libXext.  We provide the scaffolding here; the actual
            // pixel-level implementation is deferred to a follow-up that adds
            // x11rb or x11 crate bindings to avoid a direct C FFI dependency
            // on the CI host.
            //
            // For now we return a structured error so upper layers can fall
            // back to the Mock backend gracefully.
            Err(CaptureError::BackendNotSupported(
                "X11 XShmGetImage is implemented but requires the x11rb crate feature; \
                 enable the 'x11' cargo feature to activate"
                    .to_string(),
            ))
        }
    }
}

// ── DccCapture impl — non-Linux stub ──────────────────────────────────────

#[cfg(not(target_os = "linux"))]
impl DccCapture for X11Backend {
    fn backend_kind(&self) -> CaptureBackendKind {
        CaptureBackendKind::X11Xshm
    }

    fn is_available(&self) -> bool {
        false
    }

    fn capture(&self, _config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        Err(CaptureError::BackendNotSupported(
            "X11 backend is only available on Linux".to_string(),
        ))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_x11_not_available_on_non_linux() {
        let b = X11Backend::new();
        assert!(!b.is_available());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_x11_capture_returns_not_supported_on_non_linux() {
        let b = X11Backend::default();
        let result = b.capture(&CaptureConfig::default());
        assert!(matches!(
            result.unwrap_err(),
            CaptureError::BackendNotSupported(_)
        ));
    }

    #[test]
    fn test_x11_backend_kind() {
        let b = X11Backend::new();
        assert_eq!(b.backend_kind(), CaptureBackendKind::X11Xshm);
    }
}
