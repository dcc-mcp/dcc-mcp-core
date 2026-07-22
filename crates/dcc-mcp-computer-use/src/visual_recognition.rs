//! Bounded grayscale recognizers used by exact-window visual replay providers.

use dcc_mcp_ui_control::{UiBounds, UiVisualCalibration};

/// One accepted visual match in exact-window pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisualMatch {
    /// Similarity confidence in 0..=1.
    pub confidence: f64,
    /// Match bounds within the exact target image.
    pub bounds: UiBounds,
}

/// Fail-closed recognition error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VisualRecognitionError {
    /// Buffer dimensions do not match their payloads.
    #[error("invalid grayscale image dimensions")]
    InvalidDimensions,
    /// Replay target geometry, DPI, or topology differs from calibration.
    #[error("visual calibration drifted")]
    CalibrationDrift,
    /// No candidate reached the required threshold.
    #[error("visual confidence is below threshold")]
    LowConfidence,
}

/// Compare a reviewed grayscale reference with a same-sized current region.
pub fn image_difference_confidence(
    current: &[u8],
    reference: &[u8],
) -> Result<f64, VisualRecognitionError> {
    if current.is_empty() || current.len() != reference.len() {
        return Err(VisualRecognitionError::InvalidDimensions);
    }
    let absolute_error = current
        .iter()
        .zip(reference)
        .map(|(left, right)| u64::from(left.abs_diff(*right)))
        .sum::<u64>();
    let maximum = 255_u64.saturating_mul(current.len() as u64);
    Ok(1.0 - absolute_error as f64 / maximum as f64)
}

/// Find the best grayscale template inside a bounded exact-window region.
#[allow(clippy::too_many_arguments)]
pub fn match_template(
    frame: &[u8],
    frame_width: u32,
    frame_height: u32,
    template: &[u8],
    template_width: u32,
    template_height: u32,
    search_region: UiBounds,
    threshold: f64,
    recorded_calibration: UiVisualCalibration,
    current_calibration: UiVisualCalibration,
) -> Result<VisualMatch, VisualRecognitionError> {
    if !recorded_calibration.matches(current_calibration) {
        return Err(VisualRecognitionError::CalibrationDrift);
    }
    let frame_len = usize::try_from(frame_width).ok().and_then(|width| {
        usize::try_from(frame_height)
            .ok()
            .map(|height| width * height)
    });
    let template_len = usize::try_from(template_width).ok().and_then(|width| {
        usize::try_from(template_height)
            .ok()
            .map(|height| width * height)
    });
    if frame_len != Some(frame.len())
        || template_len != Some(template.len())
        || template.is_empty()
        || template_width > frame_width
        || template_height > frame_height
    {
        return Err(VisualRecognitionError::InvalidDimensions);
    }

    let left = search_region.x.max(0.0).floor() as u32;
    let top = search_region.y.max(0.0).floor() as u32;
    let right = (search_region.x + search_region.width)
        .ceil()
        .min(f64::from(frame_width)) as u32;
    let bottom = (search_region.y + search_region.height)
        .ceil()
        .min(f64::from(frame_height)) as u32;
    if right < left + template_width || bottom < top + template_height {
        return Err(VisualRecognitionError::InvalidDimensions);
    }

    let mut best = None;
    for y in top..=bottom - template_height {
        for x in left..=right - template_width {
            let mut error = 0_u64;
            for template_y in 0..template_height {
                let frame_start = ((y + template_y) * frame_width + x) as usize;
                let template_start = (template_y * template_width) as usize;
                for offset in 0..template_width as usize {
                    error += u64::from(
                        frame[frame_start + offset].abs_diff(template[template_start + offset]),
                    );
                }
            }
            let maximum = 255_u64 * u64::from(template_width) * u64::from(template_height);
            let confidence = 1.0 - error as f64 / maximum as f64;
            if best.is_none_or(|candidate: VisualMatch| confidence > candidate.confidence) {
                best = Some(VisualMatch {
                    confidence,
                    bounds: UiBounds {
                        x: f64::from(x),
                        y: f64::from(y),
                        width: f64::from(template_width),
                        height: f64::from(template_height),
                    },
                });
            }
        }
    }
    best.filter(|candidate| candidate.confidence >= threshold)
        .ok_or(VisualRecognitionError::LowConfidence)
}

/// Consecutive-frame gate used to prevent one noisy match from authorizing input.
#[derive(Debug, Clone)]
pub struct StableFrameGate {
    required: u32,
    consecutive: u32,
}

impl StableFrameGate {
    /// Create a gate requiring at least one stable frame.
    #[must_use]
    pub fn new(required: u32) -> Self {
        Self {
            required: required.max(1),
            consecutive: 0,
        }
    }

    /// Observe one confidence decision and return whether the gate is satisfied.
    pub fn observe(&mut self, matched: bool) -> bool {
        self.consecutive = if matched {
            self.consecutive.saturating_add(1)
        } else {
            0
        };
        self.consecutive >= self.required
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calibration() -> UiVisualCalibration {
        UiVisualCalibration {
            width: 4,
            height: 3,
            dpi_x: 96,
            dpi_y: 96,
            topology_generation: 1,
        }
    }

    #[test]
    fn template_matching_is_bounded_and_rejects_calibration_drift() {
        let frame = [0, 0, 0, 0, 0, 10, 20, 0, 0, 30, 40, 0];
        let matched = match_template(
            &frame,
            4,
            3,
            &[10, 20, 30, 40],
            2,
            2,
            UiBounds {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 3.0,
            },
            0.99,
            calibration(),
            calibration(),
        )
        .unwrap();
        assert_eq!(matched.bounds.x, 1.0);
        assert_eq!(matched.bounds.y, 1.0);
        assert_eq!(matched.confidence, 1.0);

        let drifted = UiVisualCalibration {
            dpi_x: 144,
            ..calibration()
        };
        assert_eq!(
            match_template(
                &frame,
                4,
                3,
                &[10, 20, 30, 40],
                2,
                2,
                UiBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 4.0,
                    height: 3.0,
                },
                0.9,
                calibration(),
                drifted,
            ),
            Err(VisualRecognitionError::CalibrationDrift)
        );
    }

    #[test]
    fn stable_frame_gate_resets_after_a_miss() {
        let mut gate = StableFrameGate::new(2);
        assert!(!gate.observe(true));
        assert!(!gate.observe(false));
        assert!(!gate.observe(true));
        assert!(gate.observe(true));
    }
}
