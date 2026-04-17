//! Windows DXGI Desktop Duplication backend.
//!
//! Uses the `windows` crate to call the Desktop Duplication API
//! (`IDXGIOutputDuplication`), which provides direct access to the GPU
//! framebuffer with minimal overhead.
//!
//! # References
//! - <https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api>

use crate::capture::DccCapture;
use crate::error::{CaptureError, CaptureResult};
#[cfg(target_os = "windows")]
use crate::types::CaptureFormat;
use crate::types::{CaptureBackendKind, CaptureConfig, CaptureFrame};

// ── DxgiBackend ────────────────────────────────────────────────────────────

/// Windows DXGI Desktop Duplication capture backend.
///
/// On non-Windows platforms this struct exists but [`DccCapture::is_available`]
/// returns `false` and [`DccCapture::capture`] returns
/// [`CaptureError::BackendNotSupported`].
#[derive(Debug)]
pub struct DxgiBackend;

impl DxgiBackend {
    /// Create a new DXGI backend instance.
    pub fn new() -> Self {
        DxgiBackend
    }
}

impl Default for DxgiBackend {
    fn default() -> Self {
        DxgiBackend::new()
    }
}

// ── DccCapture impl — Windows ──────────────────────────────────────────────

#[cfg(target_os = "windows")]
impl DccCapture for DxgiBackend {
    fn backend_kind(&self) -> CaptureBackendKind {
        CaptureBackendKind::DxgiDesktopDuplication
    }

    fn is_available(&self) -> bool {
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
        use windows::Win32::Graphics::Direct3D11::{D3D11_SDK_VERSION, D3D11CreateDevice};
        use windows::Win32::Graphics::Dxgi::CreateDXGIFactory1;
        use windows::Win32::Graphics::Dxgi::IDXGIFactory1;

        unsafe {
            // Step 1: check D3D11 is available
            let d3d_ok = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                Default::default(),
                None,
                D3D11_SDK_VERSION,
                None,
                None,
                None,
            )
            .is_ok();

            if !d3d_ok {
                return false;
            }

            // Step 2: check at least one adapter has an output (display connected)
            let factory_res: Result<IDXGIFactory1, _> = CreateDXGIFactory1();
            let Ok(factory) = factory_res else {
                return false;
            };
            let Ok(adapter) = factory.EnumAdapters1(0) else {
                return false;
            };
            adapter.EnumOutputs(0).is_ok()
        }
    }

    fn capture(&self, config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        capture_dxgi(config)
    }
}

/// Perform the actual DXGI Desktop Duplication capture.
#[cfg(target_os = "windows")]
fn capture_dxgi(config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{ImageBuffer, Rgba};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device,
        ID3D11DeviceContext, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_OUTDUPL_FRAME_INFO, IDXGIFactory1, IDXGIOutput1,
    };
    use windows::core::Interface;

    // -- Step 1: Create D3D11 device ----------------------------------------
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;

    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            Default::default(),
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .map_err(|e| CaptureError::Platform(format!("D3D11CreateDevice: {e}")))?;
    }

    let device = device.ok_or_else(|| {
        CaptureError::Platform("D3D11CreateDevice returned null device".to_string())
    })?;

    // -- Step 2: Get DXGI output --------------------------------------------
    let factory: IDXGIFactory1 = unsafe {
        CreateDXGIFactory1()
            .map_err(|e| CaptureError::Platform(format!("CreateDXGIFactory1: {e}")))?
    };

    let monitor_idx = match &config.target {
        crate::types::CaptureTarget::MonitorIndex(i) => *i,
        crate::types::CaptureTarget::PrimaryDisplay => 0,
        crate::types::CaptureTarget::ProcessId(_)
        | crate::types::CaptureTarget::WindowTitle(_)
        | crate::types::CaptureTarget::WindowHandle(_) => 0,
    };

    let adapter = unsafe {
        factory
            .EnumAdapters1(0)
            .map_err(|e| CaptureError::Platform(format!("EnumAdapters1: {e}")))?
    };

    let output = unsafe {
        adapter
            .EnumOutputs(monitor_idx as u32)
            .map_err(|e| CaptureError::Platform(format!("EnumOutputs({monitor_idx}): {e}")))?
    };

    let output1: IDXGIOutput1 = output
        .cast()
        .map_err(|e| CaptureError::Platform(format!("cast IDXGIOutput → IDXGIOutput1: {e}")))?;

    // -- Step 3: Create output duplication ----------------------------------
    let duplication = unsafe {
        output1
            .DuplicateOutput(&device)
            .map_err(|e| CaptureError::Platform(format!("DuplicateOutput: {e}")))?
    };

    // Get display mode dimensions.
    let desc = unsafe { duplication.GetDesc() };
    let src_w = desc.ModeDesc.Width;
    let src_h = desc.ModeDesc.Height;

    // -- Step 4: Acquire next frame -----------------------------------------
    let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
    let mut desktop_resource = None;
    let timeout_ms = config.timeout_ms as u32;

    unsafe {
        duplication
            .AcquireNextFrame(timeout_ms, &mut frame_info, &mut desktop_resource)
            .map_err(|e| {
                // DXGI_ERROR_WAIT_TIMEOUT = 0x887A0027
                if e.code().0 == 0x887A0027u32 as i32 {
                    CaptureError::Timeout(config.timeout_ms)
                } else {
                    CaptureError::Platform(format!("AcquireNextFrame: {e}"))
                }
            })?;
    }

    let desktop_resource = desktop_resource.ok_or_else(|| {
        CaptureError::Platform("AcquireNextFrame returned null resource".to_string())
    })?;

    let desktop_tex: ID3D11Texture2D = desktop_resource
        .cast()
        .map_err(|e| CaptureError::Platform(format!("cast desktop resource: {e}")))?;

    // -- Step 5: Copy to CPU-readable staging texture -----------------------
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: src_w,
        Height: src_h,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: Default::default(),
    };

    let mut staging_tex: Option<ID3D11Texture2D> = None;
    unsafe {
        device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging_tex))
            .map_err(|e| CaptureError::Platform(format!("CreateTexture2D (staging): {e}")))?;
    }

    let staging_tex = staging_tex
        .ok_or_else(|| CaptureError::Platform("CreateTexture2D returned null".to_string()))?;

    let ctx = context.ok_or_else(|| CaptureError::Platform("D3D11 context is null".to_string()))?;

    unsafe {
        ctx.CopyResource(&staging_tex, &desktop_tex);
    }

    // -- Step 6: Map & read pixels ------------------------------------------
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        ctx.Map(&staging_tex, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| CaptureError::Platform(format!("Map: {e}")))?;
    }

    let row_pitch = mapped.RowPitch as usize;
    let mut raw_bgra: Vec<u8> = Vec::with_capacity((src_h * src_w * 4) as usize);
    let data_ptr = mapped.pData as *const u8;
    for row in 0..src_h {
        let offset = (row as usize) * row_pitch;
        let slice =
            unsafe { std::slice::from_raw_parts(data_ptr.add(offset), (src_w * 4) as usize) };
        raw_bgra.extend_from_slice(slice);
    }

    unsafe {
        ctx.Unmap(&staging_tex, 0);
        let _ = duplication.ReleaseFrame();
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // -- Step 7: Encode to requested format --------------------------------
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = {
        let mut rgba = raw_bgra.clone();
        for chunk in rgba.chunks_exact_mut(4) {
            chunk.swap(0, 2); // BGRA → RGBA
        }
        ImageBuffer::from_raw(src_w, src_h, rgba)
            .ok_or_else(|| CaptureError::Internal("from_raw failed".to_string()))?
    };

    let (data, fmt) = match config.format {
        CaptureFormat::Png => {
            let mut buf = Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png)
                .map_err(|e| CaptureError::Image(e.to_string()))?;
            (buf.into_inner(), CaptureFormat::Png)
        }
        CaptureFormat::Jpeg => {
            let mut buf = Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Jpeg)
                .map_err(|e| CaptureError::Image(e.to_string()))?;
            (buf.into_inner(), CaptureFormat::Jpeg)
        }
        CaptureFormat::RawBgra => (raw_bgra, CaptureFormat::RawBgra),
    };

    Ok(CaptureFrame {
        data,
        width: src_w,
        height: src_h,
        format: fmt,
        timestamp_ms,
        dpi_scale: 1.0,
        window_rect: None,
        window_title: None,
    })
}

// ── DccCapture impl — non-Windows stub ────────────────────────────────────

#[cfg(not(target_os = "windows"))]
impl DccCapture for DxgiBackend {
    fn backend_kind(&self) -> CaptureBackendKind {
        CaptureBackendKind::DxgiDesktopDuplication
    }

    fn is_available(&self) -> bool {
        false
    }

    fn capture(&self, _config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        Err(CaptureError::BackendNotSupported(
            "DXGI Desktop Duplication is only available on Windows".to_string(),
        ))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_dxgi_not_available_on_non_windows() {
        let b = DxgiBackend::new();
        assert!(!b.is_available());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_dxgi_capture_returns_not_supported_on_non_windows() {
        let b = DxgiBackend::default();
        let result = b.capture(&CaptureConfig::default());
        assert!(matches!(
            result.unwrap_err(),
            CaptureError::BackendNotSupported(_)
        ));
    }

    #[test]
    fn test_dxgi_backend_kind() {
        let b = DxgiBackend::new();
        assert_eq!(b.backend_kind(), CaptureBackendKind::DxgiDesktopDuplication);
    }
}
