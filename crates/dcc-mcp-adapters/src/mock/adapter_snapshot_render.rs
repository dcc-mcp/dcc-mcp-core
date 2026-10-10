use std::collections::HashMap;
use std::sync::atomic::Ordering;

use crate::adapters::{
    CaptureResult, DccError, DccErrorCode, DccRenderCapture, DccResult, DccSnapshot, RenderOutput,
};

use super::MockDccAdapter;

impl DccSnapshot for MockDccAdapter {
    fn capture_viewport(
        &self,
        viewport: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        format: &str,
    ) -> DccResult<CaptureResult> {
        self.snapshot_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("capture_viewport")?;

        if !self.snapshot_enabled {
            return Err(DccError {
                code: DccErrorCode::Unsupported,
                message: "Snapshot not supported by this mock adapter".to_string(),
                details: None,
                recoverable: false,
            });
        }

        Ok(CaptureResult {
            data: self.snapshot_data.clone(),
            width: width.unwrap_or(1920),
            height: height.unwrap_or(1080),
            format: format.to_string(),
            viewport: viewport.map(String::from),
        })
    }
}

impl DccRenderCapture for MockDccAdapter {
    fn capture_viewport(
        &self,
        viewport: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        format: &str,
    ) -> DccResult<CaptureResult> {
        self.render_capture_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("capture_viewport")?;

        if !self.snapshot_enabled {
            return Err(DccError {
                code: DccErrorCode::Unsupported,
                message: "Viewport capture not supported by this mock adapter".to_string(),
                details: None,
                recoverable: false,
            });
        }

        Ok(CaptureResult {
            data: self.snapshot_data.clone(),
            width: width.unwrap_or(1920),
            height: height.unwrap_or(1080),
            format: format.to_string(),
            viewport: viewport.map(String::from),
        })
    }

    fn render_scene(
        &self,
        output_path: &str,
        width: Option<u32>,
        height: Option<u32>,
        renderer: Option<&str>,
    ) -> DccResult<RenderOutput> {
        self.render_capture_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("render_scene")?;

        let settings = self.render_settings.read();
        let w = width.unwrap_or_else(|| {
            settings
                .get("width")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1920)
        });
        let h = height.unwrap_or_else(|| {
            settings
                .get("height")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1080)
        });
        let fmt = output_path.rsplit('.').next().unwrap_or("png").to_string();

        let _ = renderer;

        Ok(RenderOutput {
            file_path: output_path.to_string(),
            width: w,
            height: h,
            format: fmt,
            render_time_ms: self.render_time_ms,
        })
    }

    fn get_render_settings(&self) -> DccResult<HashMap<String, String>> {
        self.render_capture_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_render_settings")?;
        Ok(self.render_settings.read().clone())
    }

    fn set_render_settings(&self, settings: HashMap<String, String>) -> DccResult<()> {
        self.render_capture_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("set_render_settings")?;

        let mut current = self.render_settings.write();
        for (k, v) in settings {
            current.insert(k, v);
        }
        Ok(())
    }
}
