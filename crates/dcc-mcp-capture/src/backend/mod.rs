//! Platform-specific capture backends.
//!
//! | Backend | Platform | Priority |
//! |---------|----------|----------|
//! | [`windows::DxgiBackend`] | Windows | 1st |
//! | [`unix::X11Backend`] | Linux (X11) | 1st |
//! | [`mock::MockBackend`] | All | Fallback |

pub mod mock;
pub mod unix;
pub mod windows;

use crate::capture::DccCapture;
use crate::types::CaptureBackendKind;

/// Create the best available backend for the current platform.
///
/// Selection order:
/// 1. DXGI Desktop Duplication (Windows only)
/// 2. X11 XShmGetImage (Linux only)
/// 3. Mock backend (universal fallback)
pub fn best_available() -> (Box<dyn DccCapture>, CaptureBackendKind) {
    let dxgi = windows::DxgiBackend::new();
    if dxgi.is_available() {
        return (Box::new(dxgi), CaptureBackendKind::DxgiDesktopDuplication);
    }

    let x11 = unix::X11Backend::new();
    if x11.is_available() {
        return (Box::new(x11), CaptureBackendKind::X11Xshm);
    }

    let mock = mock::MockBackend::new(1920, 1080);
    (Box::new(mock), CaptureBackendKind::Mock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_best_available_returns_a_backend() {
        let (backend, kind) = best_available();
        // Whatever backend was selected, it must report itself as available.
        assert!(backend.is_available());
        assert_eq!(backend.backend_kind(), kind);
    }

    #[test]
    fn test_best_available_can_capture() {
        let (backend, kind) = best_available();
        // If the best backend is Mock, capture must succeed.
        // If it's a GPU backend (DXGI/X11), it may fail in headless CI — that's OK.
        if kind == crate::types::CaptureBackendKind::Mock {
            let cfg = crate::types::CaptureConfig::default();
            let result = backend.capture(&cfg);
            assert!(result.is_ok(), "Mock capture failed: {:?}", result.err());
        }
        // For non-mock backends we only assert is_available was reported correctly.
        assert!(backend.is_available());
    }
}
