//! Core data types for the capture subsystem.

use serde::{Deserialize, Serialize};

// ── CaptureFormat ──────────────────────────────────────────────────────────

/// Output image format for a captured frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CaptureFormat {
    /// PNG lossless (default).
    #[default]
    Png,
    /// JPEG with configurable quality (0-100).
    Jpeg,
    /// Raw BGRA32 bytes (fastest, no encoding overhead).
    RawBgra,
}

impl CaptureFormat {
    /// Returns the MIME type string for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            CaptureFormat::Png => "image/png",
            CaptureFormat::Jpeg => "image/jpeg",
            CaptureFormat::RawBgra => "application/octet-stream",
        }
    }

    /// Returns the typical file extension (without the leading dot).
    pub fn extension(&self) -> &'static str {
        match self {
            CaptureFormat::Png => "png",
            CaptureFormat::Jpeg => "jpg",
            CaptureFormat::RawBgra => "raw",
        }
    }
}

// ── CaptureTarget ──────────────────────────────────────────────────────────

/// Specifies which window / screen surface to capture.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum CaptureTarget {
    /// Capture the full primary display.
    #[default]
    PrimaryDisplay,
    /// Capture the window belonging to a specific process ID.
    ProcessId(u32),
    /// Capture the window whose title contains the given substring (case-insensitive).
    WindowTitle(String),
    /// Capture a specific monitor by zero-based index.
    MonitorIndex(usize),
}

// ── CaptureConfig ──────────────────────────────────────────────────────────

/// Configuration for a single capture operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    /// Which surface to capture.
    pub target: CaptureTarget,
    /// Output image format.
    pub format: CaptureFormat,
    /// JPEG quality (0-100); ignored for PNG / RawBgra.
    pub jpeg_quality: u8,
    /// Optional scale factor (0.0 < scale ≤ 1.0).  1.0 = native resolution.
    pub scale: f32,
    /// Maximum time to wait for the first frame (milliseconds).
    pub timeout_ms: u64,
    /// Crop rectangle `[x, y, width, height]` in logical pixels.
    /// `None` means capture the full surface.
    pub crop: Option<[u32; 4]>,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        CaptureConfig {
            target: CaptureTarget::default(),
            format: CaptureFormat::Png,
            jpeg_quality: 85,
            scale: 1.0,
            timeout_ms: 5000,
            crop: None,
        }
    }
}

impl CaptureConfig {
    /// Create a builder for this config.
    pub fn builder() -> CaptureConfigBuilder {
        CaptureConfigBuilder::default()
    }
}

// ── CaptureConfigBuilder ───────────────────────────────────────────────────

/// Fluent builder for [`CaptureConfig`].
#[derive(Debug, Default)]
pub struct CaptureConfigBuilder {
    inner: CaptureConfig,
}

impl CaptureConfigBuilder {
    pub fn target(mut self, t: CaptureTarget) -> Self {
        self.inner.target = t;
        self
    }

    pub fn format(mut self, f: CaptureFormat) -> Self {
        self.inner.format = f;
        self
    }

    pub fn jpeg_quality(mut self, q: u8) -> Self {
        self.inner.jpeg_quality = q.min(100);
        self
    }

    pub fn scale(mut self, s: f32) -> Self {
        self.inner.scale = s.clamp(0.01, 1.0);
        self
    }

    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.inner.timeout_ms = ms;
        self
    }

    pub fn crop(mut self, x: u32, y: u32, w: u32, h: u32) -> Self {
        self.inner.crop = Some([x, y, w, h]);
        self
    }

    pub fn build(self) -> CaptureConfig {
        self.inner
    }
}

// ── CaptureFrame ───────────────────────────────────────────────────────────

/// A single captured frame, ready for consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureFrame {
    /// Encoded image bytes (PNG / JPEG) or raw BGRA32 data.
    pub data: Vec<u8>,
    /// Width in pixels (after any scaling/crop).
    pub width: u32,
    /// Height in pixels (after any scaling/crop).
    pub height: u32,
    /// Format of [`data`].
    pub format: CaptureFormat,
    /// Capture timestamp (milliseconds since Unix epoch).
    pub timestamp_ms: u64,
    /// Source display scale factor (e.g. 2.0 for HiDPI).
    pub dpi_scale: f32,
}

impl CaptureFrame {
    /// Returns the MIME type for the encoded bytes.
    pub fn mime_type(&self) -> &'static str {
        self.format.mime_type()
    }

    /// Returns the byte length of the encoded image.
    pub fn byte_len(&self) -> usize {
        self.data.len()
    }
}

// ── CaptureBackendKind ─────────────────────────────────────────────────────

/// Identifies the active capture backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureBackendKind {
    /// Windows DXGI Desktop Duplication API.
    DxgiDesktopDuplication,
    /// macOS ScreenCaptureKit.
    ScreenCaptureKit,
    /// Linux X11 XShmGetImage.
    X11Xshm,
    /// Linux Wayland / PipeWire.
    PipeWire,
    /// In-process mock backend (testing / headless).
    Mock,
}

impl std::fmt::Display for CaptureBackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            CaptureBackendKind::DxgiDesktopDuplication => "DXGI Desktop Duplication",
            CaptureBackendKind::ScreenCaptureKit => "ScreenCaptureKit",
            CaptureBackendKind::X11Xshm => "X11 XShmGetImage",
            CaptureBackendKind::PipeWire => "PipeWire",
            CaptureBackendKind::Mock => "Mock",
        };
        write!(f, "{s}")
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_format_defaults() {
        let fmt = CaptureFormat::default();
        assert_eq!(fmt, CaptureFormat::Png);
        assert_eq!(fmt.mime_type(), "image/png");
        assert_eq!(fmt.extension(), "png");
    }

    #[test]
    fn test_capture_format_jpeg() {
        let fmt = CaptureFormat::Jpeg;
        assert_eq!(fmt.mime_type(), "image/jpeg");
        assert_eq!(fmt.extension(), "jpg");
    }

    #[test]
    fn test_capture_format_raw_bgra() {
        let fmt = CaptureFormat::RawBgra;
        assert_eq!(fmt.mime_type(), "application/octet-stream");
        assert_eq!(fmt.extension(), "raw");
    }

    #[test]
    fn test_capture_config_default() {
        let cfg = CaptureConfig::default();
        assert_eq!(cfg.jpeg_quality, 85);
        assert!((cfg.scale - 1.0).abs() < f32::EPSILON);
        assert_eq!(cfg.timeout_ms, 5000);
        assert!(cfg.crop.is_none());
    }

    #[test]
    fn test_capture_config_builder() {
        let cfg = CaptureConfig::builder()
            .target(CaptureTarget::ProcessId(1234))
            .format(CaptureFormat::Jpeg)
            .jpeg_quality(90)
            .scale(0.5)
            .timeout_ms(1000)
            .crop(10, 20, 800, 600)
            .build();

        assert_eq!(cfg.jpeg_quality, 90);
        assert!((cfg.scale - 0.5).abs() < f32::EPSILON);
        assert_eq!(cfg.timeout_ms, 1000);
        assert_eq!(cfg.crop, Some([10, 20, 800, 600]));
        assert!(matches!(cfg.target, CaptureTarget::ProcessId(1234)));
    }

    #[test]
    fn test_capture_config_builder_clamps_quality() {
        let cfg = CaptureConfig::builder().jpeg_quality(200).build();
        assert_eq!(cfg.jpeg_quality, 100);
    }

    #[test]
    fn test_capture_config_builder_clamps_scale() {
        let cfg = CaptureConfig::builder().scale(5.0).build();
        assert!((cfg.scale - 1.0).abs() < f32::EPSILON);

        let cfg2 = CaptureConfig::builder().scale(0.0).build();
        assert!(cfg2.scale >= 0.01);
    }

    #[test]
    fn test_capture_frame_helpers() {
        let frame = CaptureFrame {
            data: vec![0u8; 128],
            width: 320,
            height: 240,
            format: CaptureFormat::Png,
            timestamp_ms: 1_700_000_000_000,
            dpi_scale: 1.0,
        };
        assert_eq!(frame.byte_len(), 128);
        assert_eq!(frame.mime_type(), "image/png");
    }

    #[test]
    fn test_capture_backend_kind_display() {
        assert_eq!(
            CaptureBackendKind::DxgiDesktopDuplication.to_string(),
            "DXGI Desktop Duplication"
        );
        assert_eq!(CaptureBackendKind::Mock.to_string(), "Mock");
    }

    #[test]
    fn test_capture_config_serialize_roundtrip() {
        let cfg = CaptureConfig::builder()
            .format(CaptureFormat::Jpeg)
            .jpeg_quality(75)
            .build();
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: CaptureConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.jpeg_quality, 75);
        assert_eq!(deserialized.format, CaptureFormat::Jpeg);
    }
}
