//! PyO3 bindings for `dcc-mcp-capture`.
//!
//! Exposes [`PyCapturer`], [`PyCaptureFrame`], [`PyCaptureTarget`],
//! [`PyCaptureBackendKind`] and [`PyWindowFinder`] to Python, enabling DCC
//! plugins to capture screenshots without any Python-native dependencies.

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

use crate::backend::mock::MockBackend;
use crate::capturer::Capturer;
use crate::types::{CaptureBackendKind, CaptureConfig, CaptureFormat, CaptureTarget};
use crate::window::WindowFinder;

// ── Error conversion ───────────────────────────────────────────────────────

fn to_py_err(e: crate::error::CaptureError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

fn parse_format(fmt: &str) -> CaptureFormat {
    match fmt {
        "jpeg" | "jpg" => CaptureFormat::Jpeg,
        "raw_bgra" | "raw" => CaptureFormat::RawBgra,
        _ => CaptureFormat::Png,
    }
}

// ── PyCaptureFrame ─────────────────────────────────────────────────────────

/// A single captured frame (Python-visible).
//
// NOTE (stub-gen PoC): we annotate the *class* (to derive `PyStubType`
// so other methods returning `PyCaptureFrame` can be typed), but skip
// the *methods* — `data()` returns `&[u8]` which `pyo3-stub-gen` can't
// auto-map. To ship the method stubs we'd add either an
// `impl_stub_type!(...)` entry or a `gen_methods_from_python!` block
// giving an explicit `bytes` return type.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
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

    /// Source window bounds ``(x, y, width, height)`` or ``None`` for
    /// full-screen / display captures.
    #[getter]
    fn window_rect(&self) -> Option<(i32, i32, i32, i32)> {
        self.inner.window_rect.map(|[x, y, w, h]| (x, y, w, h))
    }

    /// Source window title or ``None`` for full-screen / display captures.
    #[getter]
    fn window_title(&self) -> Option<String> {
        self.inner.window_title.clone()
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
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "Capturer")]
pub struct PyCapturer {
    inner: Capturer,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyCapturer {
    /// Create a capturer using the best available backend on this platform.
    #[staticmethod]
    fn new_auto() -> Self {
        PyCapturer {
            inner: Capturer::new_auto(),
        }
    }

    /// Create a capturer configured for single-window capture.
    ///
    /// Uses the GDI PrintWindow backend on Windows; falls back to Mock on
    /// other platforms until window-target backends are added.
    #[staticmethod]
    fn new_window_auto() -> Self {
        PyCapturer {
            inner: Capturer::new_window_auto(),
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

    /// Capture a single window.
    ///
    /// Parameters
    /// ----------
    /// process_id : int, optional
    /// window_handle : int, optional
    /// window_title : str, optional
    ///     At least one of the three must be provided.
    /// format, jpeg_quality, scale, timeout_ms, include_decorations :
    ///     Same semantics as :py:meth:`capture`.
    #[pyo3(signature = (
        *,
        process_id=None,
        window_handle=None,
        window_title=None,
        format="png",
        jpeg_quality=85,
        scale=1.0,
        timeout_ms=5000,
        include_decorations=true,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn capture_window(
        &self,
        process_id: Option<u32>,
        window_handle: Option<u64>,
        window_title: Option<String>,
        format: &str,
        jpeg_quality: u8,
        scale: f32,
        timeout_ms: u64,
        include_decorations: bool,
    ) -> PyResult<PyCaptureFrame> {
        let _ = include_decorations; // reserved for future client-area cropping
        let target = if let Some(h) = window_handle {
            CaptureTarget::WindowHandle(h)
        } else if let Some(pid) = process_id {
            CaptureTarget::ProcessId(pid)
        } else if let Some(title) = window_title {
            CaptureTarget::WindowTitle(title)
        } else {
            return Err(PyValueError::new_err(
                "capture_window requires one of process_id / window_handle / window_title",
            ));
        };
        let cfg = CaptureConfig::builder()
            .format(parse_format(format))
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

    /// Return the active backend kind enum.
    fn backend_kind(&self) -> PyCaptureBackendKind {
        PyCaptureBackendKind {
            inner: self.inner.backend_kind(),
        }
    }

    /// Return capture statistics as ``(count, total_bytes, errors)``.
    fn stats(&self) -> (u64, u64, u64) {
        self.inner.stats().snapshot()
    }

    /// Capture a single window as PNG bytes, looked up by process ID.
    ///
    /// This is a sugar wrapper intended for the common "grab a snapshot of a
    /// DCC window, attach it to a chat message" workflow — no ``Capturer``
    /// instance needed.  Internally creates a window-auto capturer, captures,
    /// and returns the PNG-encoded bytes.
    ///
    /// Parameters
    /// ----------
    /// pid : int
    ///     OS process ID of the DCC to capture.
    /// timeout_ms : int, optional
    ///     Max milliseconds to wait for the frame (default 1000).
    ///
    /// Returns
    /// -------
    /// bytes | None
    ///     PNG-encoded bytes on success; ``None`` when the process has no
    ///     visible top-level window or the backend is unavailable (the
    ///     function never raises for capture errors — use the instance
    ///     :py:meth:`capture_window` API when you need exceptions).
    #[staticmethod]
    #[pyo3(signature = (pid, *, timeout_ms=1000))]
    fn capture_window_png(py: Python<'_>, pid: u32, timeout_ms: u64) -> PyResult<Py<PyAny>> {
        let cfg = CaptureConfig::builder()
            .format(CaptureFormat::Png)
            .timeout_ms(timeout_ms)
            .target(CaptureTarget::ProcessId(pid))
            .build();
        let capturer = Capturer::new_window_auto();
        match capturer.capture(&cfg) {
            Ok(frame) => Ok(pyo3::types::PyBytes::new(py, &frame.data)
                .unbind()
                .into_any()),
            Err(e) => {
                tracing::debug!(?pid, error = %e, "capture_window_png returning None");
                Ok(py.None())
            }
        }
    }

    /// Capture a cropped rectangle of a window as PNG bytes, looked up by PID.
    ///
    /// The window is captured first (via the same backend as
    /// :py:meth:`capture_window_png`) and the ``(x, y, w, h)`` region is
    /// cropped in CPU before re-encoding.  Coordinates are in window-local
    /// pixels relative to the top-left of the window rectangle.
    ///
    /// Parameters
    /// ----------
    /// pid, x, y, w, h : int
    /// timeout_ms : int, optional (default 1000)
    ///
    /// Returns
    /// -------
    /// bytes | None
    ///     PNG-encoded cropped bytes on success; ``None`` on any failure
    ///     (window not found, crop out of bounds, decode error, ...).
    #[staticmethod]
    #[pyo3(signature = (pid, x, y, w, h, *, timeout_ms=1000))]
    #[allow(clippy::too_many_arguments)]
    fn capture_region_png(
        py: Python<'_>,
        pid: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        timeout_ms: u64,
    ) -> PyResult<Py<PyAny>> {
        if w == 0 || h == 0 {
            return Ok(py.None());
        }
        let cfg = CaptureConfig::builder()
            .format(CaptureFormat::Png)
            .timeout_ms(timeout_ms)
            .target(CaptureTarget::ProcessId(pid))
            .build();
        let capturer = Capturer::new_window_auto();
        let frame = match capturer.capture(&cfg) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!(?pid, error = %e, "capture_region_png capture failed");
                return Ok(py.None());
            }
        };
        match crop_png_bytes(&frame.data, x, y, w, h) {
            Ok(bytes) => Ok(pyo3::types::PyBytes::new(py, &bytes).unbind().into_any()),
            Err(e) => {
                tracing::debug!(?pid, ?x, ?y, ?w, ?h, error = %e, "capture_region_png crop failed");
                Ok(py.None())
            }
        }
    }

    fn __repr__(&self) -> String {
        format!("Capturer(backend='{}')", self.backend_name())
    }
}

/// Decode a PNG, crop to ``(x, y, w, h)`` and re-encode as PNG.
///
/// Returns the raw PNG bytes on success, or an error string describing the
/// failure (decode error, crop out of bounds, encode error).
#[cfg(feature = "python-bindings")]
fn crop_png_bytes(png: &[u8], x: u32, y: u32, w: u32, h: u32) -> Result<Vec<u8>, String> {
    use image::ImageFormat;
    let img = image::load_from_memory_with_format(png, ImageFormat::Png)
        .map_err(|e| format!("decode: {}", e))?;
    if x.saturating_add(w) > img.width() || y.saturating_add(h) > img.height() {
        return Err(format!(
            "crop ({},{},{},{}) out of bounds {}x{}",
            x,
            y,
            w,
            h,
            img.width(),
            img.height()
        ));
    }
    let cropped = img.crop_imm(x, y, w, h);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    cropped
        .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|e| format!("encode: {}", e))?;
    Ok(out)
}

// ── PyCaptureTarget ────────────────────────────────────────────────────────

/// Opaque Python wrapper for [`CaptureTarget`].
///
/// Construct via the ``primary_display``, ``monitor_index``, ``process_id``,
/// ``window_title`` or ``window_handle`` static factories.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "CaptureTarget", skip_from_py_object)]
#[derive(Clone)]
pub struct PyCaptureTarget {
    pub(crate) inner: CaptureTarget,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyCaptureTarget {
    #[staticmethod]
    fn primary_display() -> Self {
        Self {
            inner: CaptureTarget::PrimaryDisplay,
        }
    }

    #[staticmethod]
    fn monitor_index(index: usize) -> Self {
        Self {
            inner: CaptureTarget::MonitorIndex(index),
        }
    }

    #[staticmethod]
    fn process_id(pid: u32) -> Self {
        Self {
            inner: CaptureTarget::ProcessId(pid),
        }
    }

    #[staticmethod]
    fn window_title(title: String) -> Self {
        Self {
            inner: CaptureTarget::WindowTitle(title),
        }
    }

    #[staticmethod]
    fn window_handle(handle: u64) -> Self {
        Self {
            inner: CaptureTarget::WindowHandle(handle),
        }
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            CaptureTarget::PrimaryDisplay => "CaptureTarget.primary_display()".to_string(),
            CaptureTarget::MonitorIndex(i) => format!("CaptureTarget.monitor_index({i})"),
            CaptureTarget::ProcessId(p) => format!("CaptureTarget.process_id({p})"),
            CaptureTarget::WindowTitle(t) => format!("CaptureTarget.window_title({t:?})"),
            CaptureTarget::WindowHandle(h) => format!("CaptureTarget.window_handle(0x{h:x})"),
        }
    }
}

// ── PyCaptureBackendKind ───────────────────────────────────────────────────

/// Python wrapper for [`CaptureBackendKind`].
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "CaptureBackendKind", eq, frozen, skip_from_py_object)]
#[derive(Clone, PartialEq, Eq)]
pub struct PyCaptureBackendKind {
    pub(crate) inner: CaptureBackendKind,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyCaptureBackendKind {
    /// Human-readable backend name.
    #[getter]
    fn name(&self) -> String {
        self.inner.to_string()
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn DxgiDesktopDuplication() -> PyCaptureBackendKind {
        PyCaptureBackendKind {
            inner: CaptureBackendKind::DxgiDesktopDuplication,
        }
    }
    #[classattr]
    #[allow(non_snake_case)]
    fn ScreenCaptureKit() -> PyCaptureBackendKind {
        PyCaptureBackendKind {
            inner: CaptureBackendKind::ScreenCaptureKit,
        }
    }
    #[classattr]
    #[allow(non_snake_case)]
    fn X11Xshm() -> PyCaptureBackendKind {
        PyCaptureBackendKind {
            inner: CaptureBackendKind::X11Xshm,
        }
    }
    #[classattr]
    #[allow(non_snake_case)]
    fn PipeWire() -> PyCaptureBackendKind {
        PyCaptureBackendKind {
            inner: CaptureBackendKind::PipeWire,
        }
    }
    #[classattr]
    #[allow(non_snake_case)]
    fn HwndPrintWindow() -> PyCaptureBackendKind {
        PyCaptureBackendKind {
            inner: CaptureBackendKind::HwndPrintWindow,
        }
    }
    #[classattr]
    #[allow(non_snake_case)]
    fn Mock() -> PyCaptureBackendKind {
        PyCaptureBackendKind {
            inner: CaptureBackendKind::Mock,
        }
    }

    fn __repr__(&self) -> String {
        format!("CaptureBackendKind.{:?}", self.inner)
    }
}

// ── PyWindowFinder ─────────────────────────────────────────────────────────

/// Python wrapper for [`WindowFinder`].
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "WindowFinder")]
pub struct PyWindowFinder {
    inner: WindowFinder,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyWindowFinder {
    #[new]
    fn new() -> Self {
        Self {
            inner: WindowFinder::new(),
        }
    }

    /// Resolve a :class:`CaptureTarget` to a window.
    ///
    /// Returns ``None`` instead of raising when no matching window exists.
    fn find(&self, target: &PyCaptureTarget) -> Option<PyWindowInfo> {
        self.inner.find(&target.inner).ok().map(PyWindowInfo::from)
    }

    /// Return a list of all visible top-level windows.
    fn enumerate(&self) -> Vec<PyWindowInfo> {
        self.inner
            .enumerate()
            .into_iter()
            .map(PyWindowInfo::from)
            .collect()
    }
}

/// Python wrapper for [`crate::window::WindowInfo`].
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "WindowInfo", frozen)]
pub struct PyWindowInfo {
    #[pyo3(get)]
    handle: u64,
    #[pyo3(get)]
    pid: u32,
    #[pyo3(get)]
    title: String,
    #[pyo3(get)]
    rect: (i32, i32, i32, i32),
}

impl From<crate::window::WindowInfo> for PyWindowInfo {
    fn from(info: crate::window::WindowInfo) -> Self {
        Self {
            handle: info.handle,
            pid: info.pid,
            title: info.title,
            rect: (info.rect[0], info.rect[1], info.rect[2], info.rect[3]),
        }
    }
}

// ── Module registration ────────────────────────────────────────────────────

/// Register all PyO3 classes and functions exposed by this crate.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCaptureFrame>()?;
    m.add_class::<PyCapturer>()?;
    m.add_class::<PyCaptureTarget>()?;
    m.add_class::<PyCaptureBackendKind>()?;
    m.add_class::<PyWindowFinder>()?;
    m.add_class::<PyWindowInfo>()?;
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
