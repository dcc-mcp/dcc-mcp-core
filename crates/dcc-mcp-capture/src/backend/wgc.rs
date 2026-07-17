//! Windows.Graphics.Capture single-window backend.
//!
//! WGC captures DWM-composed windows while they are occluded. If WGC is not
//! available, or a capture fails, [`WgcBackend`] delegates to the existing GDI
//! `PrintWindow` / `BitBlt` backend.

use crate::capture::DccCapture;
#[allow(unused_imports)]
use crate::error::{CaptureError, CaptureResult};
use crate::types::{CaptureBackendKind, CaptureConfig, CaptureFrame};

#[cfg(any(target_os = "windows", test))]
fn capture_with_fallback<T, Primary, Fallback, Elapsed>(
    config: &CaptureConfig,
    primary: Primary,
    fallback: Fallback,
    elapsed: Elapsed,
) -> CaptureResult<(T, bool)>
where
    Primary: FnOnce(&CaptureConfig) -> CaptureResult<T>,
    Fallback: FnOnce(&CaptureConfig) -> CaptureResult<T>,
    Elapsed: FnOnce() -> std::time::Duration,
{
    match primary(config) {
        Ok(value) => Ok((value, false)),
        Err(error) => {
            tracing::warn!(%error, "WGC window capture failed; falling back to GDI");
            let timeout_ms = config.timeout_ms.max(1);
            let remaining = std::time::Duration::from_millis(timeout_ms)
                .checked_sub(elapsed())
                .filter(|duration| duration.as_millis() > 0)
                .ok_or(CaptureError::Timeout(timeout_ms))?;
            let mut fallback_config = config.clone();
            fallback_config.timeout_ms = remaining.as_millis() as u64;
            fallback(&fallback_config).map(|value| (value, true))
        }
    }
}

/// Windows.Graphics.Capture window-target backend.
#[derive(Debug)]
pub struct WgcBackend {
    #[cfg(target_os = "windows")]
    last_capture_was_wgc: std::sync::atomic::AtomicBool,
}

impl WgcBackend {
    /// Create a new WGC backend instance.
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "windows")]
            last_capture_was_wgc: std::sync::atomic::AtomicBool::new(true),
        }
    }
}

impl Default for WgcBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
impl DccCapture for WgcBackend {
    fn backend_kind(&self) -> CaptureBackendKind {
        if self
            .last_capture_was_wgc
            .load(std::sync::atomic::Ordering::Acquire)
        {
            CaptureBackendKind::WindowsGraphicsCapture
        } else {
            CaptureBackendKind::HwndPrintWindow
        }
    }

    fn is_available(&self) -> bool {
        std::thread::Builder::new()
            .name("dcc-mcp-wgc-probe".to_string())
            .spawn(imp::is_supported)
            .and_then(|worker| {
                worker
                    .join()
                    .map_err(|_| std::io::Error::other("WGC probe panicked"))
            })
            .unwrap_or(false)
    }

    fn capture(&self, config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        if config.crop.is_some() {
            return Err(CaptureError::InvalidConfig(
                "crop is not supported by Windows window capture".to_string(),
            ));
        }
        let started = std::time::Instant::now();
        let (frame, used_fallback) = capture_with_fallback(
            config,
            imp::capture,
            |fallback_config| super::hwnd::HwndBackend::new().capture(fallback_config),
            || started.elapsed(),
        )?;
        self.last_capture_was_wgc
            .store(!used_fallback, std::sync::atomic::Ordering::Release);
        Ok(frame)
    }
}

#[cfg(not(target_os = "windows"))]
impl DccCapture for WgcBackend {
    fn backend_kind(&self) -> CaptureBackendKind {
        CaptureBackendKind::WindowsGraphicsCapture
    }

    fn is_available(&self) -> bool {
        false
    }

    fn capture(&self, _config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        Err(CaptureError::BackendNotSupported(
            "Windows.Graphics.Capture is only available on Windows".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn backend_kind_is_distinct_from_gdi_fallback() {
        let backend = WgcBackend::new();
        assert_eq!(
            backend.backend_kind(),
            CaptureBackendKind::WindowsGraphicsCapture
        );
        assert_ne!(backend.backend_kind(), CaptureBackendKind::HwndPrintWindow);
        backend
            .last_capture_was_wgc
            .store(false, std::sync::atomic::Ordering::Release);
        assert_eq!(backend.backend_kind(), CaptureBackendKind::HwndPrintWindow);
    }

    #[test]
    fn gdi_fallback_uses_only_the_remaining_timeout_budget() {
        let config = CaptureConfig::builder().timeout_ms(100).build();
        let mut observed_timeout = None;
        let result = capture_with_fallback(
            &config,
            |_| Err(CaptureError::Platform("primary failed".to_string())),
            |fallback_config| {
                observed_timeout = Some(fallback_config.timeout_ms);
                Ok(())
            },
            || std::time::Duration::from_millis(37),
        );

        assert_eq!(result.unwrap(), ((), true));
        assert_eq!(observed_timeout, Some(63));
    }

    #[test]
    fn gdi_fallback_is_skipped_when_the_shared_budget_is_exhausted() {
        let config = CaptureConfig::builder().timeout_ms(100).build();
        let mut fallback_started = false;
        let result = capture_with_fallback(
            &config,
            |_| Err(CaptureError::Platform("primary failed".to_string())),
            |_| {
                fallback_started = true;
                Ok(())
            },
            || std::time::Duration::from_millis(100),
        );

        assert!(matches!(result, Err(CaptureError::Timeout(100))));
        assert!(!fallback_started);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unavailable_off_windows() {
        assert!(!WgcBackend::new().is_available());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn rejects_unsupported_crop_before_capture() {
        let result = WgcBackend::new().capture(&CaptureConfig::builder().crop(0, 0, 1, 1).build());
        assert!(matches!(result, Err(CaptureError::InvalidConfig(_))));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn availability_probe_is_stable_across_capturer_instances() {
        let first = WgcBackend::new().is_available();
        let second = WgcBackend::new().is_available();
        assert_eq!(first, second);
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::io::Cursor;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{RecvTimeoutError, sync_channel};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use image::codecs::jpeg::JpegEncoder;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use windows::Graphics::Capture::{
        Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
    };
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::{HMODULE, HWND, RECT};
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::System::WinRT::Direct3D11::{
        CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
    };
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetThreadDpiAwarenessContext,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    use windows::core::{Interface, factory};

    use crate::error::{CaptureError, CaptureResult};
    use crate::types::{CaptureConfig, CaptureFormat, CaptureFrame, CaptureTarget};
    use crate::window::WindowFinder;

    const MAX_SOURCE_PIXELS: usize = 16_777_216;
    // ponytail: one process-wide worker keeps timed-out GPU captures bounded;
    // use per-target worker state only if concurrent multi-window capture is needed.
    static CAPTURE_WORKER_ACTIVE: AtomicBool = AtomicBool::new(false);
    static WGC_SUPPORTED: OnceLock<bool> = OnceLock::new();

    struct WinRtGuard;

    impl WinRtGuard {
        fn init() -> CaptureResult<Self> {
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
                .map_err(|error| CaptureError::Platform(format!("RoInitialize: {error}")))?;
            Ok(Self)
        }
    }

    impl Drop for WinRtGuard {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }

    use crate::backend::win_dpi::ThreadDpiAwareness;

    pub(super) fn is_supported() -> bool {
        *WGC_SUPPORTED.get_or_init(probe_supported)
    }

    fn probe_supported() -> bool {
        let Ok(_winrt) = WinRtGuard::init() else {
            return false;
        };
        GraphicsCaptureSession::IsSupported().unwrap_or(false)
    }

    pub(super) fn capture(config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        if CAPTURE_WORKER_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(CaptureError::Platform(
                "a previous WGC capture is still running".to_string(),
            ));
        }

        let owned_config = config.clone();
        let (sender, receiver) = sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("dcc-mcp-wgc-capture".to_string())
            .spawn(move || {
                struct ActiveGuard;
                impl Drop for ActiveGuard {
                    fn drop(&mut self) {
                        CAPTURE_WORKER_ACTIVE.store(false, Ordering::Release);
                    }
                }
                let _active = ActiveGuard;
                let _ = sender.send(capture_inner(&owned_config));
            });
        if let Err(error) = worker {
            CAPTURE_WORKER_ACTIVE.store(false, Ordering::Release);
            return Err(CaptureError::Internal(format!(
                "failed to start WGC capture worker: {error}"
            )));
        }

        let timeout_ms = config.timeout_ms.max(1);
        match receiver.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(CaptureError::Timeout(timeout_ms)),
            Err(RecvTimeoutError::Disconnected) => Err(CaptureError::Internal(
                "WGC capture worker exited without a result".to_string(),
            )),
        }
    }

    fn capture_inner(config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        let started = Instant::now();
        // Capture runs on a dedicated worker, so the caller's DPI context does
        // not propagate. PMv2 keeps WindowFinder, GetWindowRect, WGC pixels,
        // and SendInput observations in one physical coordinate space.
        let _dpi_awareness = ThreadDpiAwareness::enter("WGC capture worker")?;
        let _winrt = WinRtGuard::init()?;
        let info = match &config.target {
            CaptureTarget::WindowHandle(_)
            | CaptureTarget::ProcessId(_)
            | CaptureTarget::WindowTitle(_) => WindowFinder::new().find(&config.target)?,
            CaptureTarget::PrimaryDisplay | CaptureTarget::MonitorIndex(_) => {
                return Err(CaptureError::BackendNotSupported(
                    "WGC window capture requires WindowHandle / ProcessId / WindowTitle"
                        .to_string(),
                ));
            }
        };
        let hwnd = HWND(info.handle as *mut core::ffi::c_void);

        let item_factory: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|error| CaptureError::Platform(format!("WGC item factory: {error}")))?;
        let item: GraphicsCaptureItem = unsafe { item_factory.CreateForWindow(hwnd) }
            .map_err(|error| CaptureError::Platform(format!("CreateForWindow: {error}")))?;
        let size = item.Size().map_err(|error| {
            CaptureError::Platform(format!("GraphicsCaptureItem.Size: {error}"))
        })?;
        checked_buffer_len(size.Width, size.Height)?;

        let (device, context, winrt_device) = create_device()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )
        .map_err(|error| CaptureError::Platform(format!("CreateFreeThreaded: {error}")))?;
        let session = pool
            .CreateCaptureSession(&item)
            .map_err(|error| CaptureError::Platform(format!("CreateCaptureSession: {error}")))?;
        let _ = session.SetIsCursorCaptureEnabled(false);
        session
            .StartCapture()
            .map_err(|error| CaptureError::Platform(format!("StartCapture: {error}")))?;

        // RAII guard: ensures session and pool are closed on every exit path
        // (early error returns, timeout, and the normal success path) after
        // StartCapture() succeeds. COM Drop/Release() alone does not guarantee
        // the Windows.Graphics.Capture pipeline is torn down; Close() is the
        // documented shutdown path that releases GPU resources.
        struct CaptureSessionGuard<'a> {
            session: &'a GraphicsCaptureSession,
            pool: &'a Direct3D11CaptureFramePool,
        }
        impl Drop for CaptureSessionGuard<'_> {
            fn drop(&mut self) {
                let _ = self.session.Close();
                let _ = self.pool.Close();
            }
        }
        let _capture_guard = CaptureSessionGuard {
            session: &session,
            pool: &pool,
        };

        let timeout = Duration::from_millis(config.timeout_ms.max(1));
        let frame = loop {
            if let Ok(frame) = pool.TryGetNextFrame() {
                break frame;
            }
            if started.elapsed() >= timeout {
                return Err(CaptureError::Timeout(config.timeout_ms.max(1)));
            }
            std::thread::sleep(Duration::from_millis(2));
        };

        let content_size = frame
            .ContentSize()
            .map_err(|error| CaptureError::Platform(format!("ContentSize: {error}")))?;
        let surface = frame
            .Surface()
            .map_err(|error| CaptureError::Platform(format!("frame.Surface: {error}")))?;
        let access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|error| CaptureError::Platform(format!("surface DXGI access: {error}")))?;
        let source: ID3D11Texture2D = unsafe { access.GetInterface() }
            .map_err(|error| CaptureError::Platform(format!("surface texture: {error}")))?;

        let mut source_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut source_desc) };
        let width = content_size.Width.min(source_desc.Width as i32);
        let height = content_size.Height.min(source_desc.Height as i32);
        let buffer_len = checked_buffer_len(width, height)?;
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: source_desc.Width,
            Height: source_desc.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: source_desc.Format,
            SampleDesc: source_desc.SampleDesc,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: Default::default(),
        };
        let mut staging = None;
        unsafe {
            device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                .map_err(|error| {
                    CaptureError::Platform(format!("CreateTexture2D staging: {error}"))
                })?;
        }
        let staging = staging.ok_or_else(|| {
            CaptureError::Platform("CreateTexture2D returned null staging texture".to_string())
        })?;
        unsafe { context.CopyResource(&staging, &source) };

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|error| CaptureError::Platform(format!("Map staging texture: {error}")))?;
        }
        let row_bytes = width as usize * 4;
        let mut bgra = Vec::with_capacity(buffer_len);
        for row in 0..height as usize {
            let source = unsafe {
                std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add(row * mapped.RowPitch as usize),
                    row_bytes,
                )
            };
            bgra.extend_from_slice(source);
        }
        unsafe { context.Unmap(&staging, 0) };
        let _ = frame.Close();
        // _capture_guard drops here (or on any earlier error path), calling
        // session.Close() and pool.Close() to release GPU capture resources.

        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }
            .map_err(|error| CaptureError::Platform(format!("GetWindowRect: {error}")))?;
        encode_frame(
            config,
            bgra,
            width as u32,
            height as u32,
            hwnd,
            rect,
            info.title,
        )
    }

    fn create_device() -> CaptureResult<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice)> {
        hardware_then_warp(
            || create_device_for_driver(D3D_DRIVER_TYPE_HARDWARE, "hardware"),
            || create_device_for_driver(D3D_DRIVER_TYPE_WARP, "WARP"),
        )
    }

    fn hardware_then_warp<T>(
        hardware: impl FnOnce() -> CaptureResult<T>,
        warp: impl FnOnce() -> CaptureResult<T>,
    ) -> CaptureResult<T> {
        match hardware() {
            Ok(value) => Ok(value),
            Err(hardware_error) => warp().map_err(|warp_error| {
                CaptureError::Platform(format!(
                    "failed to create a WGC D3D11 device; hardware: {hardware_error}; WARP: {warp_error}"
                ))
            }),
        }
    }

    fn create_device_for_driver(
        driver_type: D3D_DRIVER_TYPE,
        driver_name: &str,
    ) -> CaptureResult<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice)> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                driver_type,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|error| {
                CaptureError::Platform(format!("D3D11CreateDevice ({driver_name}) failed: {error}"))
            })?;
        }
        let device = device.ok_or_else(|| {
            CaptureError::Platform(format!(
                "D3D11CreateDevice ({driver_name}) returned null device"
            ))
        })?;
        let context = context.ok_or_else(|| {
            CaptureError::Platform(format!(
                "D3D11CreateDevice ({driver_name}) returned null context"
            ))
        })?;
        let dxgi: IDXGIDevice = device
            .cast()
            .map_err(|error| CaptureError::Platform(format!("cast IDXGIDevice: {error}")))?;
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
            .map_err(|error| CaptureError::Platform(format!("create WinRT D3D device: {error}")))?;
        let winrt_device: IDirect3DDevice = inspectable
            .cast()
            .map_err(|error| CaptureError::Platform(format!("cast IDirect3DDevice: {error}")))?;
        Ok((device, context, winrt_device))
    }

    fn encode_frame(
        config: &CaptureConfig,
        mut bgra: Vec<u8>,
        width: u32,
        height: u32,
        hwnd: HWND,
        rect: RECT,
        title: String,
    ) -> CaptureResult<CaptureFrame> {
        let (data, out_width, out_height) =
            if config.format == CaptureFormat::RawBgra && (config.scale - 1.0).abs() <= 1e-4 {
                (bgra, width, height)
            } else {
                for pixel in bgra.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                    pixel[3] = u8::MAX;
                }
                let image: ImageBuffer<Rgba<u8>, Vec<u8>> =
                    ImageBuffer::from_raw(width, height, bgra).ok_or_else(|| {
                        CaptureError::Internal("failed to construct WGC image buffer".to_string())
                    })?;
                let out_width = ((width as f32) * config.scale).round().max(1.0) as u32;
                let out_height = ((height as f32) * config.scale).round().max(1.0) as u32;
                let image = if (out_width, out_height) == (width, height) {
                    image
                } else {
                    image::imageops::resize(
                        &image,
                        out_width,
                        out_height,
                        image::imageops::FilterType::Triangle,
                    )
                };
                let mut encoded = Cursor::new(Vec::new());
                match config.format {
                    CaptureFormat::Png => image.write_to(&mut encoded, ImageFormat::Png)?,
                    CaptureFormat::Jpeg => {
                        let rgb = DynamicImage::ImageRgba8(image).into_rgb8();
                        JpegEncoder::new_with_quality(&mut encoded, config.jpeg_quality)
                            .encode_image(&rgb)?;
                    }
                    CaptureFormat::RawBgra => {
                        let mut raw = image.into_raw();
                        for pixel in raw.chunks_exact_mut(4) {
                            pixel.swap(0, 2);
                        }
                        return finish_frame(
                            raw,
                            out_width,
                            out_height,
                            config.format,
                            hwnd,
                            rect,
                            title,
                        );
                    }
                }
                (encoded.into_inner(), out_width, out_height)
            };
        finish_frame(
            data,
            out_width,
            out_height,
            config.format,
            hwnd,
            rect,
            title,
        )
    }

    fn finish_frame(
        data: Vec<u8>,
        width: u32,
        height: u32,
        format: CaptureFormat,
        hwnd: HWND,
        rect: RECT,
        title: String,
    ) -> CaptureResult<CaptureFrame> {
        Ok(CaptureFrame {
            data,
            width,
            height,
            format,
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or(0),
            dpi_scale: unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0,
            window_rect: Some([
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ]),
            window_title: Some(title),
        })
    }

    fn checked_buffer_len(width: i32, height: i32) -> CaptureResult<usize> {
        let width = usize::try_from(width)
            .map_err(|_| CaptureError::InvalidConfig(format!("invalid WGC width {width}")))?;
        let height = usize::try_from(height)
            .map_err(|_| CaptureError::InvalidConfig(format!("invalid WGC height {height}")))?;
        let pixels = width.checked_mul(height).ok_or_else(|| {
            CaptureError::InvalidConfig("WGC dimensions overflow usize".to_string())
        })?;
        if pixels == 0 || pixels > MAX_SOURCE_PIXELS {
            return Err(CaptureError::InvalidConfig(format!(
                "WGC source has {pixels} pixels; expected 1..={MAX_SOURCE_PIXELS}"
            )));
        }
        pixels.checked_mul(4).ok_or_else(|| {
            CaptureError::InvalidConfig("WGC BGRA buffer size overflowed usize".to_string())
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn source_buffer_length_is_checked() {
            assert_eq!(checked_buffer_len(1920, 1080).unwrap(), 1920 * 1080 * 4);
            assert!(checked_buffer_len(0, 1080).is_err());
            assert!(checked_buffer_len(100_000, 100_000).is_err());
        }

        #[test]
        fn device_creation_reports_both_hardware_and_warp_failures() {
            let error = hardware_then_warp::<()>(
                || Err(CaptureError::Platform("hardware sentinel".to_string())),
                || Err(CaptureError::Platform("WARP sentinel".to_string())),
            )
            .unwrap_err()
            .to_string();

            assert!(error.contains("hardware sentinel"), "{error}");
            assert!(error.contains("WARP sentinel"), "{error}");
        }

        #[test]
        fn capture_worker_uses_per_monitor_v2_and_restores_its_dpi_context() {
            use windows::Win32::UI::HiDpi::{
                AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_UNAWARE,
                GetThreadDpiAwarenessContext,
            };

            std::thread::spawn(|| {
                let original =
                    unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE) };
                assert!(!original.0.is_null());
                assert!(
                    unsafe {
                        AreDpiAwarenessContextsEqual(
                            GetThreadDpiAwarenessContext(),
                            DPI_AWARENESS_CONTEXT_UNAWARE,
                        )
                    }
                    .as_bool()
                );

                {
                    let _awareness = ThreadDpiAwareness::enter("WGC test thread").unwrap();
                    assert!(
                        unsafe {
                            AreDpiAwarenessContextsEqual(
                                GetThreadDpiAwarenessContext(),
                                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                            )
                        }
                        .as_bool()
                    );
                }

                assert!(
                    unsafe {
                        AreDpiAwarenessContextsEqual(
                            GetThreadDpiAwarenessContext(),
                            DPI_AWARENESS_CONTEXT_UNAWARE,
                        )
                    }
                    .as_bool()
                );
                let restored = unsafe { SetThreadDpiAwarenessContext(original) };
                assert!(!restored.0.is_null());
            })
            .join()
            .unwrap();
        }
    }
}
