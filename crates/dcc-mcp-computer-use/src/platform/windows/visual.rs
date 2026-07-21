use std::sync::Mutex;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CombineRgn, CreateEllipticRgn, CreateFontW,
    CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER,
    DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect,
    FW_SEMIBOLD, GetStockObject, HBRUSH, HGDIOBJ, NULL_BRUSH, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, RGN_DIFF, RGN_ERROR, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClassInfoW, GetClassNameW, GetClientRect,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GWL_USERDATA,
    HWND_TOPMOST,
    IsWindowVisible, LWA_ALPHA, MSG, PM_REMOVE, PeekMessageW, RegisterClassW,
    SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, TranslateMessage,
    WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_PAINT, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::{ComputerUseError, ComputerUseErrorCode, ComputerUseResult};

use super::geometry::{scaled_pixels, wide};
use super::OverlayGeometry;

// ---------------------------------------------------------------------------
// Visual geometry / display constants
// ---------------------------------------------------------------------------

pub(super) const CORNER_GLOW_THICKNESS: i32 = 42;
pub(super) const CORNER_MID_THICKNESS: i32 = 28;
pub(super) const CORNER_ACCENT_THICKNESS: i32 = 12;
pub(super) const CORNER_GLOW_LENGTH: i32 = 232;
pub(super) const CORNER_MID_LENGTH: i32 = 208;
pub(super) const CORNER_ACCENT_LENGTH: i32 = 180;
pub(super) const POINTER_EFFECT_SIZE: i32 = 72;
pub(super) const POINTER_RING_SIZE: i32 = 52;
pub(super) const CONTROL_OVERLAY_ALPHA: u8 = 185;
pub(super) const CONTROL_BORDER_ALPHA: u8 = 232;
pub(super) const CONTROL_CAPSULE_ALPHA: u8 = 244;
pub(super) const CONTROL_CAPSULE_GLOW_ALPHAS: [u8; 3] = [44, 78, 118];
pub(super) const CONTROL_CURSOR_ALPHA: u8 = 226;
pub(super) const CONTROL_CAPSULE_FONT_SIZE: i32 = 16;
pub(super) const CONTROL_PULSE_PERIOD_MS: u64 = 3_200;
pub(super) const CONTROL_BORDER_PULSE_FLOOR_PERCENT: u8 = 88;
pub(super) const CONTROL_CAPSULE_PULSE_FLOOR_PERCENT: u8 = 94;
pub(super) const CONTROL_CURSOR_PULSE_FLOOR_PERCENT: u8 = 90;
pub(super) const CONTROL_ACCENT_COLOR: COLORREF = COLORREF(0x00FF_840A);
pub(super) const CONTROL_GLOW_COLOR: COLORREF = COLORREF(0x00FA_A560);
pub(super) const CONTROL_CURSOR_COLOR: COLORREF = COLORREF(0x0043_9FFF);
pub(super) const CONTROL_OVERLAY_CLASS: PCWSTR = w!("DccMcpComputerUseOverlay");
pub(super) const CONTROL_GLOW_CLASS: PCWSTR = w!("DccMcpComputerUseGlowOverlay");
pub(super) const CONTROL_CURSOR_CLASS: PCWSTR = w!("DccMcpComputerUseCursorOverlay");
pub(super) const LAST_ACTION_DOT_CLASS: PCWSTR = w!("DccMcpComputerUseLastActionDot");
pub(super) const LAST_ACTION_DOT_SIZE: i32 = 16;
pub(super) const LAST_ACTION_DOT_FADE_MS: u64 = 2_000;
pub(super) const CONTROL_SCOPE_ANIMATION_MS: u64 = 1_500;
pub(super) const DEFAULT_POINTER_EFFECT_DWELL_MS: u64 = 350;

// ---------------------------------------------------------------------------
// Session color palette
// ---------------------------------------------------------------------------

/// 16-color palette for session color coding.
/// Index is selected deterministically from the session_id hash.
const SESSION_PALETTE: [COLORREF; 16] = [
    COLORREF(0x00FF_840A), // orange (original accent)
    COLORREF(0x0043_9FFF), // blue
    COLORREF(0x0016_A34A), // green
    COLORREF(0x00D9_3F3F), // red
    COLORREF(0x00C0_5BF3), // purple
    COLORREF(0x0000_BCD4), // teal
    COLORREF(0x00FF_9800), // amber
    COLORREF(0x00C6_28A8), // pink
    COLORREF(0x008B_C34A), // light green
    COLORREF(0x00FF_5722), // deep orange
    COLORREF(0x0079_55B0), // deep purple
    COLORREF(0x0000_8B8B), // dark cyan
    COLORREF(0x00B8_860B), // dark goldenrod
    COLORREF(0x00E9_1E63), // magenta-pink
    COLORREF(0x000D_47A1), // indigo
    COLORREF(0x00F4_43A5), // rose
];

/// Deterministic color from a session_id string.
pub(super) fn session_color(session_id: &str) -> COLORREF {
    let hash = session_id
        .bytes()
        .fold(0_u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    SESSION_PALETTE[(hash % SESSION_PALETTE.len() as u64) as usize]
}

/// Derive a lighter glow color from an accent color by blending with white.
pub(super) fn glow_from_accent(accent: COLORREF) -> COLORREF {
    let r = ((accent.0 & 0xFF) as u32 + 102).min(255);
    let g = (((accent.0 >> 8) & 0xFF) as u32 + 102).min(255);
    let b = (((accent.0 >> 16) & 0xFF) as u32 + 102).min(255);
    COLORREF((b << 16) | (g << 8) | r)
}

/// Derive a cursor ring color from an accent color by rotating channels.
pub(super) fn cursor_from_accent(accent: COLORREF) -> COLORREF {
    let r = accent.0 & 0xFF;
    let g = (accent.0 >> 8) & 0xFF;
    let b = (accent.0 >> 16) & 0xFF;
    // Rotate RGB -> BRG
    COLORREF((g << 16) | (r << 8) | b)
}

// ---------------------------------------------------------------------------
// Overlay tone
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(super) enum OverlayTone {
    Accent,
    Glow,
    Cursor,
}

// ---------------------------------------------------------------------------
// Overlay window class registration
// ---------------------------------------------------------------------------

pub(super) fn register_color_overlay_classes() -> ComputerUseResult<()> {
    static REGISTRATION_LOCK: Mutex<()> = Mutex::new(());
    let _registration_guard = REGISTRATION_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|error| overlay_backend_error("resolve the module handle for", error))?;
    let null_brush = HBRUSH(unsafe { GetStockObject(NULL_BRUSH) }.0);
    for class_name in [
        CONTROL_OVERLAY_CLASS,
        CONTROL_GLOW_CLASS,
        CONTROL_CURSOR_CLASS,
        LAST_ACTION_DOT_CLASS,
    ] {
        let mut existing = WNDCLASSW::default();
        if unsafe { GetClassInfoW(Some(instance.into()), class_name, &raw mut existing) }.is_ok() {
            continue;
        }
        let class = WNDCLASSW {
            lpfnWndProc: Some(overlay_window_proc),
            hInstance: instance.into(),
            hbrBackground: null_brush,
            lpszClassName: class_name,
            ..Default::default()
        };
        let atom = unsafe { RegisterClassW(&class) };
        if atom == 0 {
            return Err(overlay_backend_error(
                "register",
                format!(
                    "overlay window class: {}",
                    windows::core::Error::from_thread()
                ),
            ));
        }
    }
    Ok(())
}

unsafe extern "system" fn overlay_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_PAINT {
        let mut paint = PAINTSTRUCT::default();
        let device = unsafe { BeginPaint(hwnd, &raw mut paint) };
        if !device.0.is_null() {
            let mut bounds = RECT::default();
            let _ = unsafe { GetClientRect(hwnd, &raw mut bounds) };
            // Read per-window color from GWL_USERDATA; fall back to class-name
            // dispatch for windows created before this change (no userdata).
            let stored_color = unsafe { GetWindowLongPtrW(hwnd, GWL_USERDATA) } as u32;
            let color = if stored_color != 0 {
                COLORREF(stored_color)
            } else {
                let mut class_name = [0_u16; 64];
                let class_length =
                    unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
                match String::from_utf16_lossy(&class_name[..class_length]).as_ref() {
                    "DccMcpComputerUseGlowOverlay" => CONTROL_GLOW_COLOR,
                    "DccMcpComputerUseCursorOverlay" => CONTROL_CURSOR_COLOR,
                    _ => CONTROL_ACCENT_COLOR,
                }
            };
            let brush = unsafe { CreateSolidBrush(color) };
            let _ = unsafe { FillRect(device, &bounds, brush) };
            let _ = unsafe { DeleteObject(HGDIOBJ(brush.0)) };

            let text_length = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
            if text_length > 0 {
                let mut text = vec![0_u16; text_length + 1];
                let copied = unsafe { GetWindowTextW(hwnd, &mut text) }.max(0) as usize;
                text.truncate(copied);
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                let font = unsafe {
                    CreateFontW(
                        -scaled_pixels(CONTROL_CAPSULE_FONT_SIZE, dpi),
                        0,
                        0,
                        0,
                        FW_SEMIBOLD.0 as i32,
                        0,
                        0,
                        0,
                        DEFAULT_CHARSET,
                        OUT_DEFAULT_PRECIS,
                        CLIP_DEFAULT_PRECIS,
                        CLEARTYPE_QUALITY,
                        u32::from(DEFAULT_PITCH.0),
                        w!("Segoe UI Semibold"),
                    )
                };
                if !font.0.is_null() {
                    let old_font = unsafe { SelectObject(device, HGDIOBJ(font.0)) };
                    let _ = unsafe { SetBkMode(device, TRANSPARENT) };
                    let _ = unsafe { SetTextColor(device, COLORREF(0x00FF_FFFF)) };
                    let format = windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(
                        DT_CENTER.0 | DT_VCENTER.0 | DT_SINGLELINE.0 | DT_END_ELLIPSIS.0,
                    );
                    let _ = unsafe { DrawTextW(device, &mut text, &raw mut bounds, format) };
                    let _ = unsafe { SelectObject(device, old_font) };
                    let _ = unsafe { DeleteObject(HGDIOBJ(font.0)) };
                }
            }
        }
        let _ = unsafe { EndPaint(hwnd, &paint) };
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

// ---------------------------------------------------------------------------
// Overlay window creation
// ---------------------------------------------------------------------------

pub(super) fn create_color_overlay(
    caption: &str,
    (x, y, width, height): OverlayGeometry,
    alpha: u8,
    show: bool,
    tone: OverlayTone,
    session_color: Option<COLORREF>,
) -> ComputerUseResult<HWND> {
    register_color_overlay_classes()?;
    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|error| overlay_backend_error("resolve the module handle for", error))?;
    let caption = wide(caption);
    let style = WINDOW_STYLE(WS_POPUP.0);
    let ex_style = WINDOW_EX_STYLE(
        WS_EX_TOPMOST.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_NOACTIVATE.0
            | WS_EX_TRANSPARENT.0
            | WS_EX_LAYERED.0,
    );
    let class_name = match tone {
        OverlayTone::Accent => CONTROL_OVERLAY_CLASS,
        OverlayTone::Glow => CONTROL_GLOW_CLASS,
        OverlayTone::Cursor => CONTROL_CURSOR_CLASS,
    };
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            class_name,
            PCWSTR(caption.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .map_err(|error| overlay_backend_error("create", error.to_string()))?;
    // Store the per-window color so the WM_PAINT handler can read it.
    // When session_color is None, GWL_USERDATA stays 0 and the handler
    // falls back to the class-name-based default color.
    if let Some(color) = session_color {
        unsafe { SetWindowLongPtrW(hwnd, GWL_USERDATA, color.0 as isize) };
    }
    if let Err(error) = exclude_overlay_from_capture(hwnd) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    if let Err(error) = set_overlay_alpha(hwnd, alpha) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    let radius = width.min(height).max(1);
    let region = unsafe { CreateRoundRectRgn(0, 0, width, height, radius, radius) };
    if region.0.is_null() || unsafe { SetWindowRgn(hwnd, Some(region), true) } == 0 {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(overlay_backend_error(
            "round",
            "Windows did not accept the overlay region",
        ));
    }
    if let Err(error) = position_overlay(hwnd, (x, y, width, height), show) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    pump_overlay_messages(hwnd);
    Ok(hwnd)
}

pub(super) fn create_cursor_ring_overlay(
    geometry: OverlayGeometry,
    alpha: u8,
    session_color: Option<COLORREF>,
) -> ComputerUseResult<HWND> {
    let hwnd =
        create_color_overlay("", geometry, alpha, false, OverlayTone::Cursor, session_color)?;
    if let Err(error) = set_pointer_ring_region(hwnd, geometry.2, geometry.3) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    Ok(hwnd)
}

pub(super) fn set_pointer_ring_region(hwnd: HWND, width: i32, height: i32) -> ComputerUseResult<()> {
    let thickness = (width.min(height) / 12).max(3);
    let outer = unsafe { CreateEllipticRgn(0, 0, width, height) };
    let inner =
        unsafe { CreateEllipticRgn(thickness, thickness, width - thickness, height - thickness) };
    if outer.0.is_null() || inner.0.is_null() {
        let _ = unsafe { DeleteObject(HGDIOBJ(outer.0)) };
        let _ = unsafe { DeleteObject(HGDIOBJ(inner.0)) };
        return Err(overlay_backend_error(
            "shape",
            "Windows could not create the pointer ring",
        ));
    }
    let combined = unsafe { CombineRgn(Some(outer), Some(outer), Some(inner), RGN_DIFF) };
    let _ = unsafe { DeleteObject(HGDIOBJ(inner.0)) };
    if combined == RGN_ERROR || unsafe { SetWindowRgn(hwnd, Some(outer), true) } == 0 {
        let _ = unsafe { DeleteObject(HGDIOBJ(outer.0)) };
        return Err(overlay_backend_error(
            "shape",
            "Windows did not accept the pointer ring",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Overlay manipulation helpers
// ---------------------------------------------------------------------------

pub(super) fn set_overlay_alpha(hwnd: HWND, alpha: u8) -> ComputerUseResult<()> {
    unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA) }
        .map_err(|error| overlay_backend_error("configure transparency for", error.to_string()))
}

pub(super) fn exclude_overlay_from_capture(hwnd: HWND) -> ComputerUseResult<()> {
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|error| overlay_backend_error("exclude from capture", error.to_string()))
}

pub(super) fn position_overlay(
    hwnd: HWND,
    (x, y, width, height): OverlayGeometry,
    show: bool,
) -> ComputerUseResult<()> {
    let flags = if show {
        SWP_NOACTIVATE | SWP_SHOWWINDOW
    } else {
        SWP_NOACTIVATE
    };
    unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, flags) }
        .map_err(|error| overlay_backend_error("position", error.to_string()))?;
    let mut actual = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut actual) }
        .map_err(|error| overlay_backend_error("verify the position of", error.to_string()))?;
    if [
        actual.left,
        actual.top,
        actual.right - actual.left,
        actual.bottom - actual.top,
    ] != [x, y, width, height]
    {
        return Err(overlay_backend_error(
            "verify the position of",
            "Windows reported unexpected overlay bounds",
        ));
    }
    if show && !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return Err(overlay_backend_error(
            "show",
            "Windows did not make the overlay visible",
        ));
    }
    Ok(())
}

pub(super) fn set_overlay_visible(hwnd: HWND, visible: bool) -> ComputerUseResult<()> {
    let command = if visible { SW_SHOWNOACTIVATE } else { SW_HIDE };
    let _ = unsafe { ShowWindow(hwnd, command) };
    pump_overlay_messages(hwnd);
    if unsafe { IsWindowVisible(hwnd) }.as_bool() != visible {
        return Err(overlay_backend_error(
            if visible { "show" } else { "hide" },
            "Windows did not apply the requested visibility",
        ));
    }
    Ok(())
}

pub(super) fn overlay_backend_error(
    operation: &str,
    detail: impl std::fmt::Display,
) -> ComputerUseError {
    ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        format!("failed to {operation} the DCC UI Control visual overlay: {detail}"),
    )
}

pub(super) fn pump_overlay_messages(hwnd: HWND) {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
