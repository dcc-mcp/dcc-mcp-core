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
    capsule_glows: Vec<OverlayLayer>,
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
        target_rect: &RECT,
        caption: &str,
        initially_visible: bool,
    ) -> ComputerUseResult<Self> {
        let (capsule_geometry, corner_geometries) = overlay_geometries(target, target_rect)?;
        let mut capsule_glows = Vec::with_capacity(CONTROL_CAPSULE_GLOW_ALPHAS.len());
        for (geometry, alpha) in capsule_glow_geometries(capsule_geometry) {
            let initial_alpha = breathing_alpha(alpha, CONTROL_BORDER_PULSE_FLOOR_PERCENT, 0);
            match create_color_overlay("", geometry, initial_alpha, false, OverlayTone::Glow) {
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
        let cursor_ring = match create_cursor_ring_overlay(cursor_geometry, cursor_alpha) {
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
        };
        if initially_visible {
            overlay.set_visible(true)?;
        }
        Ok(overlay)
    }

    pub(super) fn window_handle(&self) -> HWND {
        self.capsule.hwnd
    }

    pub(super) fn reposition(&self, target: HWND, target_rect: &RECT) -> ComputerUseResult<()> {
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
        let elapsed_ms = self.pulse_started.elapsed().as_millis() as u64;
        for layer in &self.capsule_glows {
            layer.apply_pulse(CONTROL_BORDER_PULSE_FLOOR_PERCENT, elapsed_ms)?;
        }
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
        let _ = unsafe { DestroyWindow(self.cursor_ring.hwnd) };
        let _ = unsafe { DestroyWindow(self.capsule.hwnd) };
        for layer in self.capsule_glows.drain(..) {
            let _ = unsafe { DestroyWindow(layer.hwnd) };
        }
    }
}
