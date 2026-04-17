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
        let b = HwndBackend::default();
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
}

// ── Windows implementation ─────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod imp {
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{ImageBuffer, ImageFormat, Rgba};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    use crate::error::{CaptureError, CaptureResult};
    use crate::types::{CaptureConfig, CaptureFormat, CaptureFrame, CaptureTarget};
    use crate::window::WindowFinder;

    // PW_RENDERFULLCONTENT — ensures DWM-composed content (UWP, DX surfaces) is captured.
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(0x00000002);

    pub(super) fn capture_hwnd(config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
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

        let raw_bgra = unsafe { grab_bgra(hwnd, w, h)? };

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // BGRA → RGBA, then build ImageBuffer, apply scale, encode.
        let mut rgba = raw_bgra.clone();
        for chunk in rgba.chunks_exact_mut(4) {
            chunk.swap(0, 2);
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
                rgb.write_to(&mut buf, ImageFormat::Jpeg)
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
            dpi_scale: 1.0,
            window_rect: Some([rect.left, rect.top, w, h]),
            window_title: Some(info.title),
        })
    }

    /// Pull the window's pixels as a top-down BGRA buffer.
    ///
    /// Prefers `PrintWindow(PW_RENDERFULLCONTENT)` which works for DWM-composed
    /// surfaces; falls back to `BitBlt(SRCCOPY)` if `PrintWindow` refuses.
    unsafe fn grab_bgra(hwnd: HWND, w: i32, h: i32) -> CaptureResult<Vec<u8>> {
        unsafe {
            let src_dc = GetDC(Some(hwnd));
            if src_dc.is_invalid() {
                return Err(CaptureError::Platform("GetDC returned null".to_string()));
            }
            let mem_dc = CreateCompatibleDC(Some(src_dc));
            let bmp = CreateCompatibleBitmap(src_dc, w, h);
            let old = SelectObject(mem_dc, bmp.into());

            let printed = PrintWindow(hwnd, mem_dc, PW_RENDERFULLCONTENT).as_bool();
            if !printed {
                let _ = BitBlt(mem_dc, 0, 0, w, h, Some(src_dc), 0, 0, SRCCOPY);
            }

            // Negative biHeight → top-down DIB, matching most image crates.
            let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
            let mut bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let rows = GetDIBits(
                mem_dc,
                bmp,
                0,
                h as u32,
                Some(buf.as_mut_ptr() as *mut _),
                &mut bi,
                DIB_RGB_COLORS,
            );
            SelectObject(mem_dc, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem_dc);
            ReleaseDC(Some(hwnd), src_dc);
            if rows == 0 {
                return Err(CaptureError::Platform(
                    "GetDIBits returned 0 scanlines".to_string(),
                ));
            }
            Ok(buf)
        }
    }
}
