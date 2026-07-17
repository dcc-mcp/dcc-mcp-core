//! Windows GDI `PrintWindow` / `BitBlt` window-target capture backend.
//!
//! Captures a single top-level window rather than the whole desktop.
//! Resolves the target via [`WindowFinder`] so either a `ProcessId`,
//! `WindowTitle`, or `WindowHandle` target is accepted.
//!
//! On non-Windows platforms this compiles to a stub that reports
//! unavailable.
//!
//! # References
//! - <https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow>
//! - <https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-bitblt>

use crate::capture::DccCapture;
#[allow(unused_imports)]
use crate::error::{CaptureError, CaptureResult};
use crate::types::{CaptureBackendKind, CaptureConfig, CaptureFrame};

// ── HwndBackend ────────────────────────────────────────────────────────────

/// GDI-based window-target capture backend (Windows only).
#[derive(Debug, Default)]
pub struct HwndBackend;

impl HwndBackend {
    /// Create a new HWND backend instance.
    pub fn new() -> Self {
        HwndBackend
    }
}

// ── DccCapture impl — Windows ──────────────────────────────────────────────

#[cfg(target_os = "windows")]
impl DccCapture for HwndBackend {
    fn backend_kind(&self) -> CaptureBackendKind {
        CaptureBackendKind::HwndPrintWindow
    }

    fn is_available(&self) -> bool {
        true
    }

    fn capture(&self, config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        if config.crop.is_some() {
            return Err(CaptureError::InvalidConfig(
                "crop is not supported by Windows window capture".to_string(),
            ));
        }
        imp::capture_hwnd(config)
    }
}

// ── DccCapture impl — non-Windows stub ─────────────────────────────────────

#[cfg(not(target_os = "windows"))]
impl DccCapture for HwndBackend {
    fn backend_kind(&self) -> CaptureBackendKind {
        CaptureBackendKind::HwndPrintWindow
    }

    fn is_available(&self) -> bool {
        false
    }

    fn capture(&self, _config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        Err(CaptureError::BackendNotSupported(
            "HwndBackend is only available on Windows".to_string(),
        ))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hwnd_backend_kind() {
        let b = HwndBackend::new();
        assert_eq!(b.backend_kind(), CaptureBackendKind::HwndPrintWindow);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_hwnd_not_available_on_non_windows() {
        let b = HwndBackend::new();
        assert!(!b.is_available());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_hwnd_capture_returns_not_supported_on_non_windows() {
        let b = HwndBackend::new();
        let result = b.capture(&CaptureConfig::default());
        assert!(matches!(
            result.unwrap_err(),
            CaptureError::BackendNotSupported(_)
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_hwnd_nonexistent_pid_returns_target_not_found() {
        use crate::types::CaptureTarget;

        let b = HwndBackend::new();
        let cfg = CaptureConfig::builder()
            .target(CaptureTarget::ProcessId(0x7FFF_FFFF))
            .build();
        let result = b.capture(&cfg);
        assert!(matches!(result, Err(CaptureError::TargetNotFound(_))));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_hwnd_rejects_unsupported_crop() {
        let result = HwndBackend::new().capture(&CaptureConfig::builder().crop(0, 0, 1, 1).build());
        assert!(matches!(result, Err(CaptureError::InvalidConfig(_))));
    }
}

// ── Windows implementation ─────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod imp {
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageBuffer, ImageFormat, Rgba};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    use crate::backend::win_dpi::ThreadDpiAwareness;
    use crate::error::{CaptureError, CaptureResult};
    use crate::helper::{capture_same_thread_bgra, capture_via_helper, window_is_same_thread};
    use crate::types::{CaptureConfig, CaptureFormat, CaptureFrame, CaptureTarget};
    use crate::window::WindowFinder;

    const MAX_SOURCE_PIXELS: usize = 16_777_216;

    pub(super) fn capture_hwnd(config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        capture_hwnd_inner(config)
    }

    fn capture_hwnd_inner(config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        // Keep GetWindowRect and the physical SendInput coordinate space
        // consistent on mixed-DPI desktops.
        let _dpi_awareness = ThreadDpiAwareness::enter("HWND capture")?;
        let finder = WindowFinder::new();
        let info = match &config.target {
            CaptureTarget::WindowHandle(_)
            | CaptureTarget::ProcessId(_)
            | CaptureTarget::WindowTitle(_) => finder.find(&config.target)?,
            CaptureTarget::PrimaryDisplay | CaptureTarget::MonitorIndex(_) => {
                return Err(CaptureError::BackendNotSupported(
                    "HwndBackend requires a window target (WindowHandle / ProcessId / WindowTitle)"
                        .to_string(),
                ));
            }
        };

        let hwnd = HWND(info.handle as *mut core::ffi::c_void);
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }
            .map_err(|e| CaptureError::Platform(format!("GetWindowRect: {e}")))?;
        let w = (rect.right - rect.left).max(1);
        let h = (rect.bottom - rect.top).max(1);
        source_buffer_len(w, h)?;

        // PrintWindow is synchronous and unbounded. Keep same-thread windows
        // on a local, non-message BitBlt path; every other target is captured
        // by the killable helper process so timeout_ms is enforceable.
        let raw_bgra = if window_is_same_thread(info.handle) {
            capture_same_thread_bgra(info.handle, w, h)?
        } else {
            capture_via_helper(info.handle, w, h, config.timeout_ms)?
        };

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // BGRA → RGBA, then build ImageBuffer, apply scale, encode.
        let mut rgba = raw_bgra;
        for chunk in rgba.chunks_exact_mut(4) {
            chunk.swap(0, 2);
            // GDI DIB sections expose BGRX for many windows. Treat the unused
            // byte as opaque so encoded screenshots cannot become transparent.
            chunk[3] = u8::MAX;
        }
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(w as u32, h as u32, rgba)
                .ok_or_else(|| CaptureError::Internal("from_raw failed".to_string()))?;

        // Apply scale.
        let (out_w, out_h) = if (config.scale - 1.0).abs() > 1e-4 {
            let nw = ((w as f32) * config.scale).round() as u32;
            let nh = ((h as f32) * config.scale).round() as u32;
            (nw.max(1), nh.max(1))
        } else {
            (w as u32, h as u32)
        };
        let img = if (out_w, out_h) != (w as u32, h as u32) {
            image::imageops::resize(&img, out_w, out_h, image::imageops::FilterType::Triangle)
        } else {
            img
        };

        let final_w = img.width();
        let final_h = img.height();
        let (data, format) = match config.format {
            CaptureFormat::Png => {
                let mut buf = Cursor::new(Vec::new());
                img.write_to(&mut buf, ImageFormat::Png)
                    .map_err(|e| CaptureError::Image(e.to_string()))?;
                (buf.into_inner(), CaptureFormat::Png)
            }
            CaptureFormat::Jpeg => {
                let rgb = image::DynamicImage::ImageRgba8(img).into_rgb8();
                let mut buf = Cursor::new(Vec::new());
                JpegEncoder::new_with_quality(&mut buf, config.jpeg_quality)
                    .encode_image(&rgb)
                    .map_err(|e| CaptureError::Image(e.to_string()))?;
                (buf.into_inner(), CaptureFormat::Jpeg)
            }
            CaptureFormat::RawBgra => {
                // Convert the (possibly-scaled) RGBA back to BGRA.
                let mut raw: Vec<u8> = img.into_raw();
                for chunk in raw.chunks_exact_mut(4) {
                    chunk.swap(0, 2);
                }
                (raw, CaptureFormat::RawBgra)
            }
        };

        Ok(CaptureFrame {
            data,
            width: final_w,
            height: final_h,
            format,
            timestamp_ms,
            dpi_scale: unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0,
            window_rect: Some([rect.left, rect.top, w, h]),
            window_title: Some(info.title),
        })
    }
    fn source_buffer_len(w: i32, h: i32) -> CaptureResult<usize> {
        let width = usize::try_from(w)
            .map_err(|_| CaptureError::InvalidConfig(format!("invalid source window width {w}")))?;
        let height = usize::try_from(h).map_err(|_| {
            CaptureError::InvalidConfig(format!("invalid source window height {h}"))
        })?;
        let pixels = width.checked_mul(height).ok_or_else(|| {
            CaptureError::InvalidConfig("source window dimensions overflow usize".to_string())
        })?;
        if pixels > MAX_SOURCE_PIXELS {
            return Err(CaptureError::InvalidConfig(format!(
                "source window has {pixels} pixels, exceeding the {MAX_SOURCE_PIXELS}-pixel safety limit"
            )));
        }
        pixels.checked_mul(4).ok_or_else(|| {
            CaptureError::InvalidConfig("source BGRA buffer size overflowed usize".to_string())
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn source_buffer_length_is_checked_and_bounded() {
            assert_eq!(source_buffer_len(1920, 1080).unwrap(), 1920 * 1080 * 4);
            assert!(matches!(
                source_buffer_len(100_000, 100_000),
                Err(CaptureError::InvalidConfig(_))
            ));
            assert!(matches!(
                source_buffer_len(-1, 100),
                Err(CaptureError::InvalidConfig(_))
            ));
        }
    }
}
