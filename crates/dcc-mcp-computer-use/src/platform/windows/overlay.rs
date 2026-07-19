use std::cell::Cell;
use std::sync::OnceLock;
use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CombineRgn, CreateEllipticRgn, CreateFontW,
    CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER,
    DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint,
    FW_SEMIBOLD, HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, RGN_DIFF, RGN_ERROR, SelectObject,
    SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClassNameW, GetClientRect,
    GetCursorPos, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible, LWA_ALPHA,
    MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SetLayeredWindowAttributes,
    SetWindowDisplayAffinity, SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    SW_HIDE, SW_SHOWNOACTIVATE, TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_PAINT, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, HWND_TOPMOST,
};
use windows::core::{PCWSTR, w};

use crate::{
    ComputerUseError, ComputerUseErrorCode, ComputerUseResult,
};

use super::{
    CONTROL_CAPSULE_ALPHA, CONTROL_CAPSULE_GLOW_ALPHA, CONTROL_CURSOR_ALPHA,
    CONTROL_CAPSULE_FONT_SIZE, CONTROL_BORDER_PULSE_FLOOR_PERCENT,
    CONTROL_CAPSULE_PULSE_FLOOR_PERCENT, CONTROL_CURSOR_PULSE_FLOOR_PERCENT,
    CONTROL_ACCENT_COLOR, CONTROL_GLOW_COLOR, CONTROL_CURSOR_COLOR, CONTROL_OVERLAY_CLASS,
    CONTROL_GLOW_CLASS, CONTROL_CURSOR_CLASS, OverlayGeometry,
};
use super::geometry::{
    breathing_alpha, capsule_glow_geometry, overlay_geometries, point_in_rect,
    pointer_ring_geometry, scaled_pixels, wide,
};

struct OverlayLayer {
    hwnd: HWND,
    base_alpha: u8,
    applied_alpha: Cell<u8>,
}

impl OverlayLayer {
    fn new(hwnd: HWND, base_alpha: u8, applied_alpha: u8) -> Self {
        Self {
            hwnd,
            base_alpha,
            applied_alpha: Cell::new(applied_alpha),
        }
    }

    fn apply_pulse(&self, floor_percent: u8, elapsed_ms: u64) -> ComputerUseResult<()> {
        let alpha = breathing_alpha(self.base_alpha, floor_percent, elapsed_ms);
        if alpha != self.applied_alpha.get() {
            set_overlay_alpha(self.hwnd, alpha)?;
            self.applied_alpha.set(alpha);
        }
        Ok(())
    }
}

pub(super) struct ControlOverlay {
    capsule_glow: OverlayLayer,
    capsule: OverlayLayer,
    corners: Vec<OverlayLayer>,
    cursor_ring: OverlayLayer,
    cursor_ring_size: Cell<i32>,
    cursor_visible: Cell<bool>,
    pulse_started: Instant,
}

impl ControlOverlay {
    pub(super) fn new(
        target: HWND,
        target_rect: &windows::Win32::Foundation::RECT,
        caption: &str,
    ) -> ComputerUseResult<Self> {
        let (capsule_geometry, corner_geometries) = overlay_geometries(target, target_rect)?;
        let capsule_glow_geometry = capsule_glow_geometry(capsule_geometry);
        let capsule_glow_alpha = breathing_alpha(
            CONTROL_CAPSULE_GLOW_ALPHA,
            CONTROL_BORDER_PULSE_FLOOR_PERCENT,
            0,
        );
        let capsule_glow = OverlayLayer::new(
            create_color_overlay(
                "",
                capsule_glow_geometry,
                capsule_glow_alpha,
                false,
                OverlayTone::Glow,
            )?,
            CONTROL_CAPSULE_GLOW_ALPHA,
            capsule_glow_alpha,
        );
        let capsule_alpha = breathing_alpha(
            CONTROL_CAPSULE_ALPHA,
            CONTROL_CAPSULE_PULSE_FLOOR_PERCENT,
            0,
        );
        let capsule = match create_color_overlay(
            caption,
            capsule_geometry,
            capsule_alpha,
            false,
            OverlayTone::Accent,
        ) {
            Ok(hwnd) => OverlayLayer::new(hwnd, CONTROL_CAPSULE_ALPHA, capsule_alpha),
            Err(error) => {
                let _ = unsafe { DestroyWindow(capsule_glow.hwnd) };
                return Err(error);
            }
        };
        let mut corners = Vec::with_capacity(corner_geometries.len());
        for (geometry, alpha, focus) in corner_geometries {
            let initial_alpha = breathing_alpha(alpha, CONTROL_BORDER_PULSE_FLOOR_PERCENT, 0);
            let tone = if focus {
                OverlayTone::Glow
            } else {
                OverlayTone::Accent
            };
            match create_color_overlay("", geometry, initial_alpha, false, tone) {
                Ok(hwnd) => corners.push(OverlayLayer::new(hwnd, alpha, initial_alpha)),
                Err(error) => {
                    for layer in corners {
                        let _ = unsafe { DestroyWindow(layer.hwnd) };
                    }
                    let _ = unsafe { DestroyWindow(capsule.hwnd) };
                    let _ = unsafe { DestroyWindow(capsule_glow.hwnd) };
                    return Err(error);
                }
            }
        }
        let mut cursor = POINT::default();
        if let Err(error) = unsafe { GetCursorPos(&mut cursor) } {
            for layer in corners {
                let _ = unsafe { DestroyWindow(layer.hwnd) };
            }
            let _ = unsafe { DestroyWindow(capsule.hwnd) };
            let _ = unsafe { DestroyWindow(capsule_glow.hwnd) };
            return Err(overlay_backend_error(
                "locate the pointer for",
                error.to_string(),
            ));
        }
        let cursor_alpha =
            breathing_alpha(CONTROL_CURSOR_ALPHA, CONTROL_CURSOR_PULSE_FLOOR_PERCENT, 0);
        let cursor_geometry = pointer_ring_geometry(cursor.x, cursor.y);
        let cursor_ring = match create_cursor_ring_overlay(cursor_geometry, cursor_alpha) {
            Ok(hwnd) => OverlayLayer::new(hwnd, CONTROL_CURSOR_ALPHA, cursor_alpha),
            Err(error) => {
                for layer in corners {
                    let _ = unsafe { DestroyWindow(layer.hwnd) };
                }
                let _ = unsafe { DestroyWindow(capsule.hwnd) };
                let _ = unsafe { DestroyWindow(capsule_glow.hwnd) };
                return Err(error);
            }
        };
        let cursor_visible = point_in_rect(cursor, target_rect);
        let overlay = Self {
            capsule_glow,
            capsule,
            corners,
            cursor_ring,
            cursor_ring_size: Cell::new(cursor_geometry.2),
            cursor_visible: Cell::new(cursor_visible),
            pulse_started: Instant::now(),
        };
        overlay.set_visible(true)?;
        Ok(overlay)
    }

    pub(super) fn message_hwnd(&self) -> HWND {
        self.capsule.hwnd
    }

    pub(super) fn reposition(
        &self,
        target: HWND,
        target_rect: &windows::Win32::Foundation::RECT,
    ) -> ComputerUseResult<()> {
        let (capsule_geometry, corner_geometries) = overlay_geometries(target, target_rect)?;
        position_overlay(
            self.capsule_glow.hwnd,
            capsule_glow_geometry(capsule_geometry),
            false,
        )?;
        position_overlay(self.capsule.hwnd, capsule_geometry, false)?;
        for (layer, (geometry, _alpha, _focus)) in self.corners.iter().zip(corner_geometries) {
            position_overlay(layer.hwnd, geometry, false)?;
        }
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor) }
            .map_err(|error| overlay_backend_error("locate the pointer for", error.to_string()))?;
        let cursor_visible = point_in_rect(cursor, target_rect);
        if cursor_visible {
            let geometry = pointer_ring_geometry(cursor.x, cursor.y);
            position_overlay(self.cursor_ring.hwnd, geometry, false)?;
            if geometry.2 != self.cursor_ring_size.get() {
                set_pointer_ring_region(self.cursor_ring.hwnd, geometry.2, geometry.3)?;
                self.cursor_ring_size.set(geometry.2);
            }
        }
        if cursor_visible != self.cursor_visible.get() {
            set_overlay_visible(self.cursor_ring.hwnd, cursor_visible)?;
            self.cursor_visible.set(cursor_visible);
        }
        let elapsed_ms = self.pulse_started.elapsed().as_millis() as u64;
        self.capsule_glow
            .apply_pulse(CONTROL_BORDER_PULSE_FLOOR_PERCENT, elapsed_ms)?;
        self.capsule
            .apply_pulse(CONTROL_CAPSULE_PULSE_FLOOR_PERCENT, elapsed_ms)?;
        for layer in &self.corners {
            layer.apply_pulse(CONTROL_BORDER_PULSE_FLOOR_PERCENT, elapsed_ms)?;
        }
        self.cursor_ring
            .apply_pulse(CONTROL_CURSOR_PULSE_FLOOR_PERCENT, elapsed_ms)?;
        Ok(())
    }

    pub(super) fn set_visible(&self, visible: bool) -> ComputerUseResult<()> {
        set_overlay_visible(self.capsule_glow.hwnd, visible)?;
        set_overlay_visible(self.capsule.hwnd, visible)?;
        for layer in &self.corners {
            set_overlay_visible(layer.hwnd, visible)?;
        }
        if visible {
            if self.cursor_visible.get() {
                set_overlay_visible(self.cursor_ring.hwnd, true)?;
            }
        } else if self.cursor_visible.replace(false) {
            set_overlay_visible(self.cursor_ring.hwnd, false)?;
        }
        Ok(())
    }
}

impl Drop for ControlOverlay {
    fn drop(&mut self) {
        for layer in self.corners.drain(..) {
            let _ = unsafe { DestroyWindow(layer.hwnd) };
        }
        let _ = unsafe { DestroyWindow(self.cursor_ring.hwnd) };
        let _ = unsafe { DestroyWindow(self.capsule.hwnd) };
        let _ = unsafe { DestroyWindow(self.capsule_glow.hwnd) };
    }
}

#[derive(Clone, Copy)]
pub(crate) enum OverlayTone {
    Accent,
    Glow,
    Cursor,
}

fn register_color_overlay_classes() -> ComputerUseResult<()> {
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            let instance = unsafe { GetModuleHandleW(None) }
                .map_err(|error| format!("resolve module handle: {error}"))?;
            for (class_name, color) in [
                (CONTROL_OVERLAY_CLASS, CONTROL_ACCENT_COLOR),
                (CONTROL_GLOW_CLASS, CONTROL_GLOW_COLOR),
                (CONTROL_CURSOR_CLASS, CONTROL_CURSOR_COLOR),
            ] {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(overlay_window_proc),
                    hInstance: instance.into(),
                    hbrBackground: unsafe { CreateSolidBrush(color) },
                    lpszClassName: class_name,
                    ..Default::default()
                };
                let atom = unsafe { RegisterClassW(&class) };
                if atom == 0 {
                    return Err(format!(
                        "register overlay window class: {}",
                        windows::core::Error::from_thread()
                    ));
                }
            }
            Ok(())
        })
        .clone()
        .map_err(|detail| overlay_backend_error("register", detail))
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
            let mut class_name = [0_u16; 64];
            let class_length = unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
            let color = match String::from_utf16_lossy(&class_name[..class_length]).as_ref() {
                "DccMcpComputerUseGlowOverlay" => CONTROL_GLOW_COLOR,
                "DccMcpComputerUseCursorOverlay" => CONTROL_CURSOR_COLOR,
                _ => CONTROL_ACCENT_COLOR,
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

pub(crate) fn create_color_overlay(
    caption: &str,
    (x, y, width, height): OverlayGeometry,
    alpha: u8,
    show: bool,
    tone: OverlayTone,
) -> ComputerUseResult<HWND> {
    register_color_overlay_classes()?;
    let caption = wide(caption);
    let style = WINDOW_STYLE(WS_POPUP.0);
    let ex_style = WINDOW_EX_STYLE(
        WS_EX_TOPMOST.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_NOACTIVATE.0
            | WS_EX_TRANSPARENT.0
            | WS_EX_LAYERED.0,
    );
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            match tone {
                OverlayTone::Accent => CONTROL_OVERLAY_CLASS,
                OverlayTone::Glow => CONTROL_GLOW_CLASS,
                OverlayTone::Cursor => CONTROL_CURSOR_CLASS,
            },
            PCWSTR(caption.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            None,
            None,
            None,
            None,
        )
    }
    .map_err(|error| overlay_backend_error("create", error.to_string()))?;
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

pub(crate) fn create_cursor_ring_overlay(geometry: OverlayGeometry, alpha: u8) -> ComputerUseResult<HWND> {
    let hwnd = create_color_overlay("", geometry, alpha, false, OverlayTone::Cursor)?;
    if let Err(error) = set_pointer_ring_region(hwnd, geometry.2, geometry.3) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    Ok(hwnd)
}

fn set_pointer_ring_region(hwnd: HWND, width: i32, height: i32) -> ComputerUseResult<()> {
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

fn set_overlay_alpha(hwnd: HWND, alpha: u8) -> ComputerUseResult<()> {
    unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA) }
        .map_err(|error| overlay_backend_error("configure transparency for", error.to_string()))
}

fn exclude_overlay_from_capture(hwnd: HWND) -> ComputerUseResult<()> {
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
        .map_err(|error| overlay_backend_error("exclude from capture", error.to_string()))
}

pub(crate) fn position_overlay(
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

pub(crate) fn set_overlay_visible(hwnd: HWND, visible: bool) -> ComputerUseResult<()> {
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

fn overlay_backend_error(operation: &str, detail: impl std::fmt::Display) -> ComputerUseError {
    ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        format!("failed to {operation} the DCC UI Control visual overlay: {detail}"),
    )
}

pub(crate) fn pump_overlay_messages(hwnd: HWND) {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
