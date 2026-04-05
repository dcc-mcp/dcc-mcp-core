//! Mock capture backend — generates synthetic frames for testing and CI.
//!
//! Produces a deterministic checkerboard pattern (BGRA32) and encodes it
//! in whichever format [`CaptureConfig`] requests.

use std::io::Cursor;
use std::time::{SystemTime, UNIX_EPOCH};

use image::{ImageBuffer, ImageFormat, Rgba};

use crate::capture::DccCapture;
use crate::error::{CaptureError, CaptureResult};
use crate::types::{CaptureBackendKind, CaptureConfig, CaptureFormat, CaptureFrame};

// ── MockBackend ────────────────────────────────────────────────────────────

/// Synthetic capture backend for testing and headless environments.
///
/// Generates a deterministic RGBA checkerboard at the configured resolution.
#[derive(Debug)]
pub struct MockBackend {
    width: u32,
    height: u32,
}

impl MockBackend {
    /// Create a new mock backend with the given native resolution.
    pub fn new(width: u32, height: u32) -> Self {
        MockBackend { width, height }
    }

    /// Generate a checkerboard `ImageBuffer` at native resolution.
    fn make_image(&self) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        let tile = 16u32;
        ImageBuffer::from_fn(self.width, self.height, |x, y| {
            let is_light = ((x / tile) + (y / tile)) % 2 == 0;
            if is_light {
                Rgba([200u8, 200, 200, 255])
            } else {
                Rgba([80u8, 80, 80, 255])
            }
        })
    }
}

impl DccCapture for MockBackend {
    fn backend_kind(&self) -> CaptureBackendKind {
        CaptureBackendKind::Mock
    }

    fn is_available(&self) -> bool {
        true
    }

    fn capture(&self, config: &CaptureConfig) -> CaptureResult<CaptureFrame> {
        if self.width == 0 || self.height == 0 {
            return Err(CaptureError::InvalidConfig(
                "width and height must be > 0".to_string(),
            ));
        }

        let img = self.make_image();

        // Apply scaling
        let (out_w, out_h) = if (config.scale - 1.0).abs() > 1e-4 {
            let nw = ((self.width as f32) * config.scale).round() as u32;
            let nh = ((self.height as f32) * config.scale).round() as u32;
            (nw.max(1), nh.max(1))
        } else {
            (self.width, self.height)
        };

        let img = if (out_w, out_h) != (self.width, self.height) {
            image::imageops::resize(&img, out_w, out_h, image::imageops::FilterType::Nearest)
        } else {
            img
        };

        // Apply crop
        let img = if let Some([cx, cy, cw, ch]) = config.crop {
            let cx = cx.min(img.width().saturating_sub(1));
            let cy = cy.min(img.height().saturating_sub(1));
            let cw = cw.min(img.width() - cx);
            let ch = ch.min(img.height() - cy);
            image::imageops::crop_imm(&img, cx, cy, cw, ch).to_image()
        } else {
            img
        };

        let final_w = img.width();
        let final_h = img.height();

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let (data, format) = match config.format {
            CaptureFormat::Png => {
                let mut buf = Cursor::new(Vec::new());
                img.write_to(&mut buf, ImageFormat::Png)
                    .map_err(|e| CaptureError::Image(e.to_string()))?;
                (buf.into_inner(), CaptureFormat::Png)
            }
            CaptureFormat::Jpeg => {
                // JPEG encoder does not support RGBA; convert to RGB first.
                let rgb = image::DynamicImage::ImageRgba8(img).into_rgb8();
                let mut buf = Cursor::new(Vec::new());
                rgb.write_to(&mut buf, ImageFormat::Jpeg)
                    .map_err(|e| CaptureError::Image(e.to_string()))?;
                (buf.into_inner(), CaptureFormat::Jpeg)
            }
            CaptureFormat::RawBgra => {
                // Convert RGBA → BGRA in-place
                let mut raw: Vec<u8> = img.into_raw();
                for chunk in raw.chunks_exact_mut(4) {
                    chunk.swap(0, 2); // R ↔ B
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
            dpi_scale: 1.0,
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CaptureFormat, CaptureTarget};

    fn make_cfg(fmt: CaptureFormat) -> CaptureConfig {
        CaptureConfig::builder().format(fmt).build()
    }

    #[test]
    fn test_mock_png_magic_bytes() {
        let b = MockBackend::new(64, 64);
        let frame = b.capture(&make_cfg(CaptureFormat::Png)).unwrap();
        assert!(frame.data.starts_with(b"\x89PNG"));
        assert_eq!(frame.width, 64);
        assert_eq!(frame.height, 64);
    }

    #[test]
    fn test_mock_jpeg_magic_bytes() {
        let b = MockBackend::new(64, 64);
        let frame = b.capture(&make_cfg(CaptureFormat::Jpeg)).unwrap();
        assert!(frame.data.starts_with(&[0xFF, 0xD8]));
    }

    #[test]
    fn test_mock_raw_bgra_size() {
        let b = MockBackend::new(16, 8);
        let frame = b.capture(&make_cfg(CaptureFormat::RawBgra)).unwrap();
        assert_eq!(frame.data.len(), 16 * 8 * 4);
        assert_eq!(frame.format, CaptureFormat::RawBgra);
    }

    #[test]
    fn test_mock_scale_half() {
        let b = MockBackend::new(100, 100);
        let cfg = CaptureConfig::builder()
            .format(CaptureFormat::RawBgra)
            .scale(0.5)
            .build();
        let frame = b.capture(&cfg).unwrap();
        assert_eq!(frame.width, 50);
        assert_eq!(frame.height, 50);
        assert_eq!(frame.data.len(), 50 * 50 * 4);
    }

    #[test]
    fn test_mock_crop() {
        let b = MockBackend::new(200, 150);
        let cfg = CaptureConfig::builder()
            .format(CaptureFormat::RawBgra)
            .crop(10, 20, 80, 60)
            .build();
        let frame = b.capture(&cfg).unwrap();
        assert_eq!(frame.width, 80);
        assert_eq!(frame.height, 60);
        assert_eq!(frame.data.len(), 80 * 60 * 4);
    }

    #[test]
    fn test_mock_zero_size_returns_error() {
        let b = MockBackend::new(0, 100);
        let result = b.capture(&CaptureConfig::default());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CaptureError::InvalidConfig(_)
        ));
    }

    #[test]
    fn test_mock_timestamp_nonzero() {
        let b = MockBackend::new(4, 4);
        let frame = b.capture(&CaptureConfig::default()).unwrap();
        assert!(frame.timestamp_ms > 0);
    }

    #[test]
    fn test_mock_target_ignored() {
        // MockBackend ignores the target — any target should succeed.
        let b = MockBackend::new(64, 64);
        let cfg = CaptureConfig::builder()
            .target(CaptureTarget::ProcessId(99999))
            .build();
        let frame = b.capture(&cfg).unwrap();
        assert_eq!(frame.width, 64);
    }

    #[test]
    fn test_mock_checkerboard_deterministic() {
        // Two captures of the same mock should produce identical raw bytes
        // (timestamp will differ, but raw pixel data should not).
        let b = MockBackend::new(32, 32);
        let cfg = CaptureConfig::builder()
            .format(CaptureFormat::RawBgra)
            .build();
        let f1 = b.capture(&cfg).unwrap();
        let f2 = b.capture(&cfg).unwrap();
        // Pixel data is generated by from_fn which is deterministic.
        assert_eq!(f1.data, f2.data);
    }
}
