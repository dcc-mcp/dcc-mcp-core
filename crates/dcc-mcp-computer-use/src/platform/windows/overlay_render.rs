use windows::Win32::Foundation::{
    COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CombineRgn, CreateEllipticRgn, CreateFontW,
    CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER,
    DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FW_SEMIBOLD,
    GetStockObject, HBRUSH, HGDIOBJ, NULL_BRUSH, OUT_DEFAULT_PRECIS, PAINTSTRUCT, RGN_DIFF, RGN_ERROR,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClassInfoW, GetClassNameW,
    GetClientRect, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GWL_USERDATA, HWND_TOPMOST, IsWindowVisible, LWA_ALPHA, MSG, PM_REMOVE,
    PeekMessageW, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_PAINT,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP, WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT,
    WTS_REMOTE_DISCONNECT, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::core::{PCWSTR, w};

use crate::{ComputerUseError, ComputerUseErrorCode, ComputerUseResult};
use super::geometry::scaled_pixels;
use super::{
    CONTROL_ACCENT_COLOR, CONTROL_CAPSULE_FONT_SIZE, CONTROL_CURSOR_COLOR, CONTROL_GLOW_COLOR,
    CONTROL_OVERLAY_CLASS, CONTROL_GLOW_CLASS, CONTROL_CURSOR_CLASS, LAST_ACTION_DOT_CLASS,
    OverlayGeometry,
};

#[derive(Clone, Copy)]
pub(super) enum OverlayTone {
    Accent,
    Glow,
    Cursor,
}

pub(super) fn register_color_overlay_classes() -> ComputerUseResult<()> {
    static REGISTRATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
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
            let _ = unsafe { windows::Win32::Graphics::Gdi::FillRect(device, &bounds, brush) };
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
    let caption = super::wide(caption);
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
    let hwnd = create_color_overlay("", geometry, alpha, false, OverlayTone::Cursor, session_color)?;
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

pub(super) fn session_event_blocked(event: u32) -> Option<bool> {
    match event {
        WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => Some(true),
        WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => Some(false),
        _ => None,
    }
}
