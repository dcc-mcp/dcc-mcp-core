//! PyO3 bindings for `dcc-mcp-capture`.
//!
//! Exposes [`PyCapturer`] and [`PyCaptureFrame`] to Python, enabling DCC
//! plugins to capture screenshots without any Python-native dependencies.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::backend::mock::MockBackend;
use crate::capturer::Capturer;
use crate::types::{CaptureConfig, CaptureFormat, CaptureTarget};

// ── Error conversion ───────────────────────────────────────────────────────

fn to_py_err(e: crate::error::CaptureError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

// ── PyCaptureFrame ─────────────────────────────────────────────────────────

/// A single captured frame (Python-visible).
#[pyclass(name = "CaptureFrame")]
pub struct PyCaptureFrame {
    inner: crate::types::CaptureFrame,
}

#[pymethods]
impl PyCaptureFrame {
    /// Raw image bytes (PNG, JPEG, or BGRA32 depending on format).
    #[getter]
    fn data(&self) -> &[u8] {
        &self.inner.data
    }

    /// Frame width in pixels.
    #[getter]
    fn width(&self) -> u32 {
        self.inner.width
    }

    /// Frame height in pixels.
    #[getter]
    fn height(&self) -> u32 {
        self.inner.height
    }

    /// Format string: ``"png"``, ``"jpeg"``, or ``"raw_bgra"``.
    #[getter]
    fn format(&self) -> &str {
        match self.inner.format {
            CaptureFormat::Png => "png",
            CaptureFormat::Jpeg => "jpeg",
            CaptureFormat::RawBgra => "raw_bgra",
        }
    }

    /// MIME type for the encoded bytes.
    #[getter]
    fn mime_type(&self) -> &str {
        self.inner.mime_type()
    }

    /// Milliseconds since Unix epoch at capture time.
    #[getter]
    fn timestamp_ms(&self) -> u64 {
        self.inner.timestamp_ms
    }

    /// Display scale factor (1.0 for standard, 2.0 for HiDPI).
    #[getter]
    fn dpi_scale(&self) -> f32 {
        self.inner.dpi_scale
    }

    /// Byte length of the encoded image data.
    fn byte_len(&self) -> usize {
        self.inner.byte_len()
    }

    fn __repr__(&self) -> String {
        format!(
            "CaptureFrame({}x{}, format='{}', {} bytes)",
            self.inner.width,
            self.inner.height,
            self.format(),
            self.inner.byte_len()
        )
    }
}

// ── PyCapturer ─────────────────────────────────────────────────────────────

/// DCC screenshot / frame-capture entry point (Python-visible).
///
/// # Example (Python)
/// ```python
/// from dcc_mcp_core import PyCapturer
/// capturer = PyCapturer.new_auto()
/// frame = capturer.capture()
/// print(f"Captured {frame.width}×{frame.height}, {frame.byte_len()} bytes")
/// ```
#[pyclass(name = "Capturer")]
pub struct PyCapturer {
    inner: Capturer,
}

#[pymethods]
impl PyCapturer {
    /// Create a capturer using the best available backend on this platform.
    #[staticmethod]
    fn new_auto() -> Self {
        PyCapturer {
            inner: Capturer::new_auto(),
        }
    }

    /// Create a capturer backed by the mock (synthetic checkerboard) backend.
    ///
    /// Useful in headless CI and testing without a GPU or display.
    #[staticmethod]
    #[pyo3(signature = (width=1920, height=1080))]
    fn new_mock(width: u32, height: u32) -> Self {
        PyCapturer {
            inner: Capturer::with_backend(Box::new(MockBackend::new(width, height))),
        }
    }

    /// Capture a single frame.
    ///
    /// Parameters
    /// ----------
    /// format : str, optional
    ///     ``"png"`` (default), ``"jpeg"``, or ``"raw_bgra"``.
    /// jpeg_quality : int, optional
    ///     JPEG quality 0-100 (default 85). Ignored for PNG / raw_bgra.
    /// scale : float, optional
    ///     Scale factor 0.0-1.0 (default 1.0 = native resolution).
    /// timeout_ms : int, optional
    ///     Max milliseconds to wait for a frame (default 5000).
    /// process_id : int, optional
    ///     Capture the window belonging to this PID.
    /// window_title : str, optional
    ///     Capture the window whose title contains this substring.
    #[pyo3(signature = (
        format="png",
        jpeg_quality=85,
        scale=1.0,
        timeout_ms=5000,
        process_id=None,
        window_title=None
    ))]
    fn capture(
        &self,
        format: &str,
        jpeg_quality: u8,
        scale: f32,
        timeout_ms: u64,
        process_id: Option<u32>,
        window_title: Option<String>,
    ) -> PyResult<PyCaptureFrame> {
        let fmt = match format {
            "jpeg" | "jpg" => CaptureFormat::Jpeg,
            "raw_bgra" | "raw" => CaptureFormat::RawBgra,
            _ => CaptureFormat::Png,
        };

        let target = if let Some(pid) = process_id {
            CaptureTarget::ProcessId(pid)
        } else if let Some(title) = window_title {
            CaptureTarget::WindowTitle(title)
        } else {
            CaptureTarget::PrimaryDisplay
        };

        let cfg = CaptureConfig::builder()
            .format(fmt)
            .jpeg_quality(jpeg_quality)
            .scale(scale)
            .timeout_ms(timeout_ms)
            .target(target)
            .build();

        let frame = self.inner.capture(&cfg).map_err(to_py_err)?;
        Ok(PyCaptureFrame { inner: frame })
    }

    /// Return the active backend name.
    fn backend_name(&self) -> String {
        self.inner.backend_kind().to_string()
    }

    /// Return capture statistics as ``(count, total_bytes, errors)``.
    fn stats(&self) -> (u64, u64, u64) {
        self.inner.stats().snapshot()
    }

    fn __repr__(&self) -> String {
        format!("Capturer(backend='{}')", self.backend_name())
    }
}

// ── Module registration ────────────────────────────────────────────────────

/// Register all PyO3 classes and functions exposed by this crate.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCaptureFrame>()?;
    m.add_class::<PyCapturer>()?;
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_py_capturer_new_mock() {
        let c = PyCapturer::new_mock(640, 480);
        assert!(c.backend_name().contains("Mock"));
    }

    #[test]
    fn test_py_capturer_capture_png() {
        let c = PyCapturer::new_mock(100, 100);
        let frame = c.capture("png", 85, 1.0, 5000, None, None).unwrap();
        assert_eq!(frame.format(), "png");
        assert!(frame.data().starts_with(b"\x89PNG"));
        assert_eq!(frame.width(), 100);
        assert_eq!(frame.height(), 100);
        assert!(frame.byte_len() > 0);
    }

    #[test]
    fn test_py_capturer_capture_jpeg() {
        let c = PyCapturer::new_mock(64, 64);
        let frame = c.capture("jpeg", 90, 1.0, 5000, None, None).unwrap();
        assert_eq!(frame.format(), "jpeg");
        assert_eq!(frame.mime_type(), "image/jpeg");
    }

    #[test]
    fn test_py_capturer_capture_raw() {
        let c = PyCapturer::new_mock(16, 16);
        let frame = c.capture("raw_bgra", 85, 1.0, 5000, None, None).unwrap();
        assert_eq!(frame.format(), "raw_bgra");
        assert_eq!(frame.byte_len(), 16 * 16 * 4);
    }

    #[test]
    fn test_py_capturer_stats_accumulate() {
        let c = PyCapturer::new_mock(32, 32);
        for _ in 0..3 {
            let _ = c.capture("png", 85, 1.0, 5000, None, None).unwrap();
        }
        let (count, bytes, errs) = c.stats();
        assert_eq!(count, 3);
        assert!(bytes > 0);
        assert_eq!(errs, 0);
    }

    #[test]
    fn test_py_capturer_new_auto_backend_name_nonempty() {
        let c = PyCapturer::new_auto();
        assert!(!c.backend_name().is_empty());
    }

    #[test]
    fn test_py_capturer_repr() {
        let c = PyCapturer::new_mock(1, 1);
        assert!(c.__repr__().contains("Capturer"));
    }

    #[test]
    fn test_py_capture_frame_repr() {
        let c = PyCapturer::new_mock(10, 10);
        let frame = c.capture("png", 85, 1.0, 5000, None, None).unwrap();
        assert!(frame.__repr__().contains("10x10"));
    }

    #[test]
    fn test_py_capturer_scale_half() {
        let c = PyCapturer::new_mock(200, 100);
        let frame = c.capture("raw_bgra", 85, 0.5, 5000, None, None).unwrap();
        assert_eq!(frame.width(), 100);
        assert_eq!(frame.height(), 50);
    }
}
