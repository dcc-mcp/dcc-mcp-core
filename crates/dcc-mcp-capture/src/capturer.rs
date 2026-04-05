//! High-level `Capturer` — the main user-facing entry point.
//!
//! `Capturer` wraps a boxed [`DccCapture`] backend and adds:
//! - Automatic backend selection at construction time
//! - Frame metadata enrichment
//! - Statistics tracking (capture count, total bytes)

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::backend;
use crate::capture::DccCapture;
use crate::error::CaptureResult;
use crate::types::{CaptureBackendKind, CaptureConfig, CaptureFrame};

// ── CaptureStats ───────────────────────────────────────────────────────────

/// Running statistics for a [`Capturer`] instance.
#[derive(Debug, Default)]
pub struct CaptureStats {
    /// Total number of successful captures.
    pub capture_count: AtomicU64,
    /// Total bytes produced across all successful captures.
    pub total_bytes: AtomicU64,
    /// Total number of capture errors.
    pub error_count: AtomicU64,
}

impl CaptureStats {
    fn record_success(&self, byte_len: usize) {
        self.capture_count.fetch_add(1, Ordering::Relaxed);
        self.total_bytes
            .fetch_add(byte_len as u64, Ordering::Relaxed);
    }

    fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Return a snapshot `(capture_count, total_bytes, error_count)`.
    pub fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.capture_count.load(Ordering::Relaxed),
            self.total_bytes.load(Ordering::Relaxed),
            self.error_count.load(Ordering::Relaxed),
        )
    }
}

// ── Capturer ───────────────────────────────────────────────────────────────

/// High-level screenshot / frame-capture entry point.
///
/// # Example
/// ```rust,no_run
/// use dcc_mcp_capture::{Capturer, CaptureConfig, CaptureFormat};
///
/// let capturer = Capturer::new_auto();
/// let frame = capturer.capture(&CaptureConfig::default()).unwrap();
/// println!("Captured {}×{} ({} bytes)", frame.width, frame.height, frame.byte_len());
/// ```
pub struct Capturer {
    backend: Box<dyn DccCapture>,
    backend_kind: CaptureBackendKind,
    stats: Arc<CaptureStats>,
}

impl std::fmt::Debug for Capturer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capturer")
            .field("backend_kind", &self.backend_kind)
            .finish_non_exhaustive()
    }
}

impl Capturer {
    /// Create a new `Capturer` using the best available backend for the
    /// current platform.
    pub fn new_auto() -> Self {
        let (backend, backend_kind) = backend::best_available();
        Capturer {
            backend,
            backend_kind,
            stats: Arc::new(CaptureStats::default()),
        }
    }

    /// Create a `Capturer` from an explicit backend.
    pub fn with_backend(backend: Box<dyn DccCapture>) -> Self {
        let kind = backend.backend_kind();
        Capturer {
            backend,
            backend_kind: kind,
            stats: Arc::new(CaptureStats::default()),
        }
    }

    /// Capture a single frame.
    pub fn capture(&self, config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        match self.backend.capture(config) {
            Ok(frame) => {
                self.stats.record_success(frame.byte_len());
                Ok(frame)
            }
            Err(e) => {
                self.stats.record_error();
                Err(e)
            }
        }
    }

    /// Returns the active backend kind.
    pub fn backend_kind(&self) -> CaptureBackendKind {
        self.backend_kind
    }

    /// Returns a shared reference to the running statistics.
    pub fn stats(&self) -> Arc<CaptureStats> {
        Arc::clone(&self.stats)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::mock::MockBackend;
    use crate::types::CaptureFormat;

    fn mock_capturer(w: u32, h: u32) -> Capturer {
        Capturer::with_backend(Box::new(MockBackend::new(w, h)))
    }

    #[test]
    fn test_capturer_auto_is_available() {
        let c = Capturer::new_auto();
        assert!(c.backend.is_available());
    }

    #[test]
    fn test_capturer_with_mock_backend() {
        let c = mock_capturer(320, 240);
        assert_eq!(c.backend_kind(), CaptureBackendKind::Mock);
        let frame = c.capture(&CaptureConfig::default()).unwrap();
        assert_eq!(frame.width, 320);
        assert_eq!(frame.height, 240);
    }

    #[test]
    fn test_capturer_stats_success() {
        let c = mock_capturer(64, 64);
        let _ = c.capture(&CaptureConfig::default()).unwrap();
        let _ = c.capture(&CaptureConfig::default()).unwrap();
        let (count, bytes, errs) = c.stats().snapshot();
        assert_eq!(count, 2);
        assert!(bytes > 0);
        assert_eq!(errs, 0);
    }

    #[test]
    fn test_capturer_stats_error_incremented() {
        // Zero-size mock triggers an error.
        let c = Capturer::with_backend(Box::new(MockBackend::new(0, 0)));
        let _ = c.capture(&CaptureConfig::default());
        let (count, _, errs) = c.stats().snapshot();
        assert_eq!(count, 0);
        assert_eq!(errs, 1);
    }

    #[test]
    fn test_capturer_jpeg_format() {
        let c = mock_capturer(100, 100);
        let cfg = CaptureConfig::builder().format(CaptureFormat::Jpeg).build();
        let frame = c.capture(&cfg).unwrap();
        assert_eq!(frame.format, CaptureFormat::Jpeg);
        // JPEG magic bytes
        assert!(frame.data.starts_with(&[0xFF, 0xD8]));
    }

    #[test]
    fn test_capturer_stats_bytes_accumulate() {
        let c = mock_capturer(128, 128);
        let cfg = CaptureConfig::builder()
            .format(CaptureFormat::RawBgra)
            .build();
        for _ in 0..5 {
            let _ = c.capture(&cfg).unwrap();
        }
        let (count, bytes, _) = c.stats().snapshot();
        assert_eq!(count, 5);
        // Raw BGRA: 128 * 128 * 4 = 65536 bytes per frame.
        assert_eq!(bytes, 5 * 128 * 128 * 4);
    }
}
