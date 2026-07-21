use std::cell::Cell;
use std::time::Instant;

use super::*;

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

    /// Apply pulse with an optional alpha boost (e.g. for scope animation).
    /// When boost is 0 this is equivalent to the normal breathing pulse.
    fn apply_pulse_with_boost(
        &self,
        floor_percent: u8,
        elapsed_ms: u64,
        boost: u8,
    ) -> ComputerUseResult<()> {
        let base = breathing_alpha(self.base_alpha, floor_percent, elapsed_ms);
        let alpha = base.saturating_add(boost);
        if alpha != self.applied_alpha.get() {
            set_overlay_alpha(self.hwnd, alpha)?;
            self.applied_alpha.set(alpha);
        }
        Ok(())
    }
}

pub(super) struct ControlOverlay {
    capsule_glows: Vec<OverlayLayer>,
    capsule: OverlayLayer,
    corners: Vec<OverlayLayer>,
    cursor_ring: OverlayLayer,
    cursor_ring_size: Cell<i32>,
    cursor_visible: Cell<bool>,
    pulse_started: Instant,
    scope_started: Instant,
    last_action_dot: Option<OverlayLayer>,
    last_action_started: Option<Instant>,
}

impl ControlOverlay {
    pub(super) fn new(
        target: HWND,
        target_rect: &RECT,
        caption: &str,
        session_id: Option<&str>,
    ) -> ComputerUseResult<Self> {
        // Compute session colors: accent for capsule/corners, glow for glow layers,
        // cursor for cursor ring. When session_id is None, use the original hardcoded
        // defaults.
        let (accent_color, glow_color, cursor_color) = if let Some(id) = session_id {
            let accent = session_color(id);
            let glow = glow_from_accent(accent);
            let cursor = cursor_from_accent(accent);
            (Some(accent), Some(glow), Some(cursor))
        } else {
            (None, None, None)
        };

        let (capsule_geometry, corner_geometries) = overlay_geometries(target, target_rect)?;
        let mut capsule_glows = Vec::with_capacity(CONTROL_CAPSULE_GLOW_ALPHAS.len());
        for (geometry, alpha) in capsule_glow_geometries(capsule_geometry) {
            let initial_alpha = breathing_alpha(alpha, CONTROL_BORDER_PULSE_FLOOR_PERCENT, 0);
            match create_color_overlay(
                "",
                geometry,
                initial_alpha,
                false,
                OverlayTone::Glow,
                glow_color,
            ) {
                Ok(hwnd) => capsule_glows.push(OverlayLayer::new(hwnd, alpha, initial_alpha)),
                Err(error) => {
                    for layer in capsule_glows {
                        let _ = unsafe { DestroyWindow(layer.hwnd) };
                    }
                    return Err(error);
                }
            }
        }
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
            accent_color,
        ) {
            Ok(hwnd) => OverlayLayer::new(hwnd, CONTROL_CAPSULE_ALPHA, capsule_alpha),
            Err(error) => {
                for layer in capsule_glows {
                    let _ = unsafe { DestroyWindow(layer.hwnd) };
                }
                return Err(error);
            }
        };
        let mut corners = Vec::with_capacity(corner_geometries.len());
        for (geometry, alpha, focus) in corner_geometries {
            let initial_alpha = breathing_alpha(alpha, CONTROL_BORDER_PULSE_FLOOR_PERCENT, 0);
            let (tone, color) = if focus {
                (OverlayTone::Glow, glow_color)
            } else {
                (OverlayTone::Accent, accent_color)
            };
            match create_color_overlay("", geometry, initial_alpha, false, tone, color) {
                Ok(hwnd) => corners.push(OverlayLayer::new(hwnd, alpha, initial_alpha)),
                Err(error) => {
                    for layer in corners {
                        let _ = unsafe { DestroyWindow(layer.hwnd) };
                    }
                    let _ = unsafe { DestroyWindow(capsule.hwnd) };
                    for layer in capsule_glows {
                        let _ = unsafe { DestroyWindow(layer.hwnd) };
                    }
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
            for layer in capsule_glows {
                let _ = unsafe { DestroyWindow(layer.hwnd) };
            }
            return Err(overlay_backend_error(
                "locate the pointer for",
                error.to_string(),
            ));
        }
        let cursor_alpha =
            breathing_alpha(CONTROL_CURSOR_ALPHA, CONTROL_CURSOR_PULSE_FLOOR_PERCENT, 0);
        let cursor_geometry = pointer_ring_geometry(cursor.x, cursor.y);
        let cursor_ring =
            match create_cursor_ring_overlay(cursor_geometry, cursor_alpha, cursor_color) {
                Ok(hwnd) => OverlayLayer::new(hwnd, CONTROL_CURSOR_ALPHA, cursor_alpha),
                Err(error) => {
                    for layer in corners {
                        let _ = unsafe { DestroyWindow(layer.hwnd) };
                    }
                    let _ = unsafe { DestroyWindow(capsule.hwnd) };
                    for layer in capsule_glows {
                        let _ = unsafe { DestroyWindow(layer.hwnd) };
                    }
                    return Err(error);
                }
            };
        let cursor_visible = point_in_rect(cursor, target_rect);
        let overlay = Self {
            capsule_glows,
            capsule,
            corners,
            cursor_ring,
            cursor_ring_size: Cell::new(cursor_geometry.2),
            cursor_visible: Cell::new(cursor_visible),
            pulse_started: Instant::now(),
            scope_started: Instant::now(),
            last_action_dot: None,
            last_action_started: None,
        };
        overlay.set_visible(true)?;
        Ok(overlay)
    }

    pub(super) fn window_handle(&self) -> HWND {
        self.capsule.hwnd
    }

    /// Record a pointer action point so the fading dot can be shown.
    pub(super) fn record_last_action(&mut self, screen_x: i32, screen_y: i32) {
        // Destroy previous dot if any
        if let Some(ref old) = self.last_action_dot {
            let _ = unsafe { DestroyWindow(old.hwnd) };
        }
        self.last_action_dot = None;
        self.last_action_started = Some(Instant::now());

        // Create new dot at the action point
        let x = screen_x - LAST_ACTION_DOT_SIZE / 2;
        let y = screen_y - LAST_ACTION_DOT_SIZE / 2;
        let geometry = (x, y, LAST_ACTION_DOT_SIZE, LAST_ACTION_DOT_SIZE);
        if let Ok(hwnd) = create_color_overlay(
            "",
            geometry,
            255,
            true,
            OverlayTone::Accent,
            Some(CONTROL_ACCENT_COLOR),
        ) {
            // Make it circular
            let region =
                unsafe { CreateEllipticRgn(0, 0, LAST_ACTION_DOT_SIZE, LAST_ACTION_DOT_SIZE) };
            if !region.0.is_null() {
                let _ = unsafe { SetWindowRgn(hwnd, Some(region), true) };
            }
            self.last_action_dot = Some(OverlayLayer::new(hwnd, 255, 255));
        }
    }

    fn update_last_action_dot(&mut self) {
        let Some(ref dot) = self.last_action_dot else {
            return;
        };
        let Some(start) = self.last_action_started else {
            return;
        };
        let elapsed_ms = start.elapsed().as_millis() as u64;
        if elapsed_ms >= LAST_ACTION_DOT_FADE_MS {
            // Fully faded — hide and destroy
            let _ = unsafe { DestroyWindow(dot.hwnd) };
            self.last_action_dot = None;
            self.last_action_started = None;
            return;
        }
        // Ease-out cubic: (1 - t)^3 for smooth fade
        let progress = elapsed_ms as f64 / LAST_ACTION_DOT_FADE_MS as f64;
        let alpha = ((1.0 - progress).powi(3) * 255.0) as u8;
        if alpha != dot.applied_alpha.get() {
            let _ = set_overlay_alpha(dot.hwnd, alpha);
            dot.applied_alpha.set(alpha);
        }
    }

    pub(super) fn reposition(&mut self, target: HWND, target_rect: &RECT) -> ComputerUseResult<()> {
        let (capsule_geometry, corner_geometries) = overlay_geometries(target, target_rect)?;
        for (layer, (geometry, _alpha)) in self
            .capsule_glows
            .iter()
            .zip(capsule_glow_geometries(capsule_geometry))
        {
            position_overlay(layer.hwnd, geometry, false)?;
        }
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

        // Compute scope animation boost: extra alpha during first ~1.5 seconds
        let scope_elapsed = self.scope_started.elapsed().as_millis() as u64;
        let scope_boost = if scope_elapsed < CONTROL_SCOPE_ANIMATION_MS {
            let progress = scope_elapsed as f64 / CONTROL_SCOPE_ANIMATION_MS as f64;
            // Ease-out: 1 - (1-t)^2, capped at ~25% boost
            ((1.0 - (1.0 - progress).powi(2)) * 0.25 * 255.0) as u8
        } else {
            0
        };

        let elapsed_ms = self.pulse_started.elapsed().as_millis() as u64;
        for layer in &self.capsule_glows {
            layer.apply_pulse_with_boost(
                CONTROL_BORDER_PULSE_FLOOR_PERCENT,
                elapsed_ms,
                scope_boost,
            )?;
        }
        self.capsule.apply_pulse_with_boost(
            CONTROL_CAPSULE_PULSE_FLOOR_PERCENT,
            elapsed_ms,
            scope_boost,
        )?;
        for layer in &self.corners {
            layer.apply_pulse_with_boost(
                CONTROL_BORDER_PULSE_FLOOR_PERCENT,
                elapsed_ms,
                scope_boost,
            )?;
        }
        self.cursor_ring.apply_pulse_with_boost(
            CONTROL_CURSOR_PULSE_FLOOR_PERCENT,
            elapsed_ms,
            scope_boost,
        )?;

        // Update the last-action fading dot
        self.update_last_action_dot();

        Ok(())
    }

    pub(super) fn set_visible(&self, visible: bool) -> ComputerUseResult<()> {
        for layer in &self.capsule_glows {
            set_overlay_visible(layer.hwnd, visible)?;
        }
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
        if let Some(ref dot) = self.last_action_dot {
            let _ = unsafe { DestroyWindow(dot.hwnd) };
        }
        self.last_action_dot = None;
        let _ = unsafe { DestroyWindow(self.cursor_ring.hwnd) };
        let _ = unsafe { DestroyWindow(self.capsule.hwnd) };
        for layer in self.capsule_glows.drain(..) {
            let _ = unsafe { DestroyWindow(layer.hwnd) };
        }
    }
}

// ---------------------------------------------------------------------------
// Overlay window class registration and primitive creation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(super) enum OverlayTone {
    Accent,
    Glow,
    Cursor,
}

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
                let class_length = unsafe { GetClassNameW(hwnd, &mut class_name) }.max(0) as usize;
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
    let hwnd = create_color_overlay(
        "",
        geometry,
        alpha,
        false,
        OverlayTone::Cursor,
        session_color,
    )?;
    if let Err(error) = set_pointer_ring_region(hwnd, geometry.2, geometry.3) {
        let _ = unsafe { DestroyWindow(hwnd) };
        return Err(error);
    }
    Ok(hwnd)
}

pub(super) fn set_pointer_ring_region(
    hwnd: HWND,
    width: i32,
    height: i32,
) -> ComputerUseResult<()> {
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
