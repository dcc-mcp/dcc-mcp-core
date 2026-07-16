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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{RecvTimeoutError, sync_channel};
    use std::thread;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageBuffer, ImageFormat, Rgba};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits, GetWindowDC, HBITMAP, HDC, HGDIOBJ,
        ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
        SetThreadDpiAwarenessContext,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsHungAppWindow};

    use crate::error::{CaptureError, CaptureResult};
    use crate::types::{CaptureConfig, CaptureFormat, CaptureFrame, CaptureTarget};
    use crate::window::WindowFinder;

    // PW_RENDERFULLCONTENT — ensures DWM-composed content (UWP, DX surfaces) is captured.
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(0x00000002);
    const MAX_SOURCE_PIXELS: usize = 16_777_216;
    static CAPTURE_WORKER_ACTIVE: AtomicBool = AtomicBool::new(false);
    static CAPTURE_WORKER_POISONED: AtomicBool = AtomicBool::new(false);

    struct WindowDc {
        hwnd: HWND,
        dc: HDC,
    }

    impl Drop for WindowDc {
        fn drop(&mut self) {
            unsafe { ReleaseDC(Some(self.hwnd), self.dc) };
        }
    }

    struct MemoryDc(HDC);

    impl Drop for MemoryDc {
        fn drop(&mut self) {
            let _ = unsafe { DeleteDC(self.0) };
        }
    }

    struct OwnedBitmap(HBITMAP);

    impl Drop for OwnedBitmap {
        fn drop(&mut self) {
            let _ = unsafe { DeleteObject(self.0.into()) };
        }
    }

    struct BitmapSelection {
        dc: HDC,
        previous: HGDIOBJ,
        active: bool,
    }

    impl BitmapSelection {
        unsafe fn restore(&mut self) -> CaptureResult<()> {
            if self.active {
                let restored = unsafe { SelectObject(self.dc, self.previous) };
                if restored.is_invalid() {
                    return Err(CaptureError::Platform(
                        "failed to restore the previous GDI bitmap".to_string(),
                    ));
                }
                self.active = false;
            }
            Ok(())
        }
    }

    impl Drop for BitmapSelection {
        fn drop(&mut self) {
            let _ = unsafe { self.restore() };
        }
    }

    fn restore_then_read<T>(
        restore: impl FnOnce() -> CaptureResult<()>,
        read: impl FnOnce() -> T,
    ) -> CaptureResult<T> {
        restore()?;
        Ok(read())
    }

    struct ThreadDpiAwareness {
        previous: DPI_AWARENESS_CONTEXT,
    }

    impl ThreadDpiAwareness {
        fn enter() -> CaptureResult<Self> {
            let previous =
                unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            if previous.0.is_null() {
                return Err(CaptureError::Platform(
                    "Windows refused per-monitor-v2 DPI awareness for the HWND capture worker"
                        .to_string(),
                ));
            }
            Ok(Self { previous })
        }
    }

    impl Drop for ThreadDpiAwareness {
        fn drop(&mut self) {
            let _ = unsafe { SetThreadDpiAwarenessContext(self.previous) };
        }
    }

    pub(super) fn capture_hwnd(config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        let owned_config = config.clone();
        run_bounded_capture(
            &CAPTURE_WORKER_ACTIVE,
            &CAPTURE_WORKER_POISONED,
            config.timeout_ms,
            move || capture_hwnd_inner(&owned_config),
        )
    }

    fn run_bounded_capture<F>(
        active: &'static AtomicBool,
        poisoned: &'static AtomicBool,
        timeout_ms: u64,
        capture: F,
    ) -> CaptureResult<CaptureFrame>
    where
        F: FnOnce() -> CaptureResult<CaptureFrame> + Send + 'static,
    {
        let timeout_ms = timeout_ms.max(1);
        if active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            if poisoned.load(Ordering::Acquire) {
                return Err(CaptureError::Platform(
                    "a previous HWND capture timed out and its Windows worker has not returned; restart the adapter before retrying Computer Use capture"
                        .to_string(),
                ));
            }
            return Err(CaptureError::Timeout(timeout_ms));
        }
        poisoned.store(false, Ordering::Release);

        let (sender, receiver) = sync_channel(1);
        let spawn_result = thread::Builder::new()
            .name("dcc-mcp-hwnd-capture".to_string())
            .spawn(move || {
                struct ActiveGuard {
                    active: &'static AtomicBool,
                    poisoned: &'static AtomicBool,
                }
                impl Drop for ActiveGuard {
                    fn drop(&mut self) {
                        self.poisoned.store(false, Ordering::Release);
                        self.active.store(false, Ordering::Release);
                    }
                }

                let _active_guard = ActiveGuard { active, poisoned };
                let result = capture();
                let _ = sender.send(result);
            });
        if let Err(error) = spawn_result {
            poisoned.store(false, Ordering::Release);
            active.store(false, Ordering::Release);
            return Err(CaptureError::Internal(format!(
                "failed to start bounded HWND capture worker: {error}"
            )));
        }

        match receiver.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                poisoned.store(true, Ordering::Release);
                Err(CaptureError::Timeout(timeout_ms))
            }
            Err(RecvTimeoutError::Disconnected) => Err(CaptureError::Internal(
                "HWND capture worker exited without a result".to_string(),
            )),
        }
    }

    fn capture_hwnd_inner(config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        // Capture runs on a bounded worker thread, so the caller's DPI context
        // does not propagate. Enter PMv2 here to keep GetWindowRect and the
        // physical SendInput coordinate space consistent on mixed-DPI desktops.
        let _dpi_awareness = ThreadDpiAwareness::enter()?;
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

        let raw_bgra = unsafe { grab_bgra(hwnd, w, h)? };

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // BGRA → RGBA, then build ImageBuffer, apply scale, encode.
        let mut rgba = raw_bgra.clone();
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

    /// Pull the window's pixels as a top-down BGRA buffer.
    ///
    /// Prefers `PrintWindow(PW_RENDERFULLCONTENT)` which works for DWM-composed
    /// surfaces; falls back to `BitBlt(SRCCOPY)` if `PrintWindow` refuses.
    unsafe fn grab_bgra(hwnd: HWND, w: i32, h: i32) -> CaptureResult<Vec<u8>> {
        unsafe { grab_bgra_impl(hwnd, w, h, true) }
    }

    unsafe fn grab_bgra_impl(
        hwnd: HWND,
        w: i32,
        h: i32,
        allow_print_window: bool,
    ) -> CaptureResult<Vec<u8>> {
        unsafe {
            let buffer_len = source_buffer_len(w, h)?;
            // The buffer and observation use GetWindowRect coordinates, so the
            // fallback source must include the non-client area and share the
            // same (0, 0) window origin. GetDC(hwnd) is client-relative.
            let src_dc = GetWindowDC(Some(hwnd));
            if src_dc.is_invalid() {
                return Err(CaptureError::Platform(
                    "GetWindowDC returned null".to_string(),
                ));
            }
            let src_dc = WindowDc { hwnd, dc: src_dc };
            let mem_dc = CreateCompatibleDC(Some(src_dc.dc));
            if mem_dc.is_invalid() {
                return Err(CaptureError::Platform(
                    "CreateCompatibleDC returned null".to_string(),
                ));
            }
            let mem_dc = MemoryDc(mem_dc);
            let bmp = CreateCompatibleBitmap(src_dc.dc, w, h);
            if bmp.is_invalid() {
                return Err(CaptureError::Platform(
                    "CreateCompatibleBitmap returned null".to_string(),
                ));
            }
            let bmp = OwnedBitmap(bmp);
            let old = SelectObject(mem_dc.0, bmp.0.into());
            if old.is_invalid() {
                return Err(CaptureError::Platform(
                    "SelectObject failed for the capture bitmap".to_string(),
                ));
            }
            let mut selection = BitmapSelection {
                dc: mem_dc.0,
                previous: old,
                active: true,
            };

            // PrintWindow may block indefinitely inside a hung target process.
            // Skip it when User32 already considers the window unresponsive and
            // use the bounded BitBlt fallback instead.
            let printed = if !allow_print_window || IsHungAppWindow(hwnd).as_bool() {
                false
            } else {
                PrintWindow(hwnd, mem_dc.0, PW_RENDERFULLCONTENT).as_bool()
            };
            let fallback_error = (!printed)
                .then(|| BitBlt(mem_dc.0, 0, 0, w, h, Some(src_dc.dc), 0, 0, SRCCOPY))
                .and_then(Result::err);

            if let Some(error) = fallback_error {
                selection.restore()?;
                return Err(CaptureError::Platform(format!(
                    "PrintWindow and BitBlt both failed: {error}"
                )));
            }

            // Negative biHeight → top-down DIB, matching most image crates.
            let mut buf = vec![0u8; buffer_len];
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
            // GetDIBits requires the queried bitmap not to be selected into a DC.
            let rows = restore_then_read(
                || selection.restore(),
                || {
                    GetDIBits(
                        mem_dc.0,
                        bmp.0,
                        0,
                        h as u32,
                        Some(buf.as_mut_ptr() as *mut _),
                        &mut bi,
                        DIB_RGB_COLORS,
                    )
                },
            )?;
            if rows == 0 {
                return Err(CaptureError::Platform(
                    "GetDIBits returned 0 scanlines".to_string(),
                ));
            }
            Ok(buf)
        }
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

        static TEST_CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
        static TEST_CAPTURE_POISONED: AtomicBool = AtomicBool::new(false);

        #[test]
        fn bounded_worker_returns_timeout_without_starting_unbounded_retries() {
            TEST_CAPTURE_ACTIVE.store(false, Ordering::Release);
            TEST_CAPTURE_POISONED.store(false, Ordering::Release);
            let result =
                run_bounded_capture(&TEST_CAPTURE_ACTIVE, &TEST_CAPTURE_POISONED, 10, || {
                    thread::sleep(Duration::from_millis(50));
                    Err(CaptureError::Internal("late result".to_string()))
                });
            assert!(matches!(result, Err(CaptureError::Timeout(10))));
            assert!(TEST_CAPTURE_POISONED.load(Ordering::Acquire));

            let second =
                run_bounded_capture(&TEST_CAPTURE_ACTIVE, &TEST_CAPTURE_POISONED, 10, || {
                    Err(CaptureError::Internal("must not run".to_string()))
                });
            assert!(
                matches!(second, Err(CaptureError::Platform(message)) if message.contains("restart the adapter"))
            );

            thread::sleep(Duration::from_millis(60));
            assert!(!TEST_CAPTURE_ACTIVE.load(Ordering::Acquire));
            assert!(!TEST_CAPTURE_POISONED.load(Ordering::Acquire));
        }

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

        #[test]
        fn bitmap_is_restored_before_readback() {
            use std::cell::RefCell;

            let calls = RefCell::new(Vec::new());
            let value = restore_then_read(
                || {
                    calls.borrow_mut().push("restore");
                    Ok(())
                },
                || {
                    calls.borrow_mut().push("read");
                    7
                },
            )
            .unwrap();

            assert_eq!(value, 7);
            assert_eq!(*calls.borrow(), ["restore", "read"]);
        }

        #[test]
        fn failed_bitmap_restore_skips_readback() {
            let mut read = false;
            let result = restore_then_read(
                || Err(CaptureError::Platform("restore failed".to_string())),
                || {
                    read = true;
                },
            );

            assert!(result.is_err());
            assert!(!read);
        }

        #[test]
        fn bitblt_fallback_uses_full_window_coordinates() {
            use windows::Win32::Foundation::COLORREF;
            use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, SetPixel};
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WS_BORDER, WS_POPUP,
                WS_VISIBLE,
            };
            use windows::core::PCWSTR;

            struct TestWindow(HWND);
            impl Drop for TestWindow {
                fn drop(&mut self) {
                    let _ = unsafe { DestroyWindow(self.0) };
                }
            }

            let class = "STATIC"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let title = "dcc-mcp-capture-test"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let window = TestWindow(
                unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        PCWSTR(class.as_ptr()),
                        PCWSTR(title.as_ptr()),
                        WINDOW_STYLE(WS_POPUP.0 | WS_BORDER.0 | WS_VISIBLE.0),
                        100,
                        100,
                        120,
                        80,
                        None,
                        None,
                        None,
                        None,
                    )
                }
                .unwrap(),
            );
            let mut rect = RECT::default();
            unsafe { GetWindowRect(window.0, &mut rect) }.unwrap();
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;

            let window_dc = unsafe { GetWindowDC(Some(window.0)) };
            let client_dc = unsafe { GetDC(Some(window.0)) };
            assert!(!window_dc.is_invalid());
            assert!(!client_dc.is_invalid());
            let window_marker = COLORREF(0x0000_00E1);
            let client_marker = COLORREF(0x00D2_0000);
            assert_ne!(
                unsafe { SetPixel(window_dc, 0, 0, window_marker) }.0,
                u32::MAX
            );
            assert_ne!(
                unsafe { SetPixel(client_dc, 0, 0, client_marker) }.0,
                u32::MAX
            );
            assert_eq!(unsafe { GetPixel(window_dc, 0, 0) }, window_marker);
            assert_eq!(unsafe { GetPixel(client_dc, 0, 0) }, client_marker);
            unsafe {
                ReleaseDC(Some(window.0), client_dc);
                ReleaseDC(Some(window.0), window_dc);
            }

            let bgra = unsafe { grab_bgra_impl(window.0, width, height, false) }.unwrap();
            assert_eq!(bgra.len(), (width * height * 4) as usize);
            assert_eq!(&bgra[..3], &[0, 0, 0xE1]);
        }
    }
}
