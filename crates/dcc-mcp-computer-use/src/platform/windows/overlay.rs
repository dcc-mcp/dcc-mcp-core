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
        initially_visible: bool,
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
        if initially_visible {
            overlay.set_visible(true)?;
        }
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
