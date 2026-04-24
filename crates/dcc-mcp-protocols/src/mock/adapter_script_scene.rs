use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::adapters::{
    DccError, DccErrorCode, DccResult, DccSceneInfo, DccScriptEngine, SceneInfo, ScriptLanguage,
    ScriptResult,
};

use super::MockDccAdapter;

impl DccScriptEngine for MockDccAdapter {
    fn execute_script(
        &self,
        code: &str,
        language: ScriptLanguage,
        timeout_ms: Option<u64>,
    ) -> DccResult<ScriptResult> {
        self.script_count.fetch_add(1, Ordering::Relaxed);

        if !self.connected.load(Ordering::SeqCst) {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: "Not connected — call connect() first".to_string(),
                details: None,
                recoverable: true,
            });
        }

        if !self.capabilities.script_languages.contains(&language) {
            return Ok(ScriptResult::failure(
                format!("Unsupported language: {language}"),
                0,
            ));
        }

        let start = Instant::now();
        let result = if let Some(ref handler) = self.script_handler {
            handler(code, language, timeout_ms)
        } else {
            Ok(code.to_string())
        };
        let elapsed_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(output) => Ok(ScriptResult::success(output, elapsed_ms)),
            Err(error) => Ok(ScriptResult::failure(error, elapsed_ms)),
        }
    }

    fn supported_languages(&self) -> Vec<ScriptLanguage> {
        self.capabilities.script_languages.clone()
    }
}

impl DccSceneInfo for MockDccAdapter {
    fn get_scene_info(&self) -> DccResult<SceneInfo> {
        self.scene_query_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_scene_info")?;
        Ok(self.scene.read().clone())
    }

    fn list_objects(&self) -> DccResult<Vec<(String, String)>> {
        self.scene_query_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("list_objects")?;

        let stats = &self.scene.read().statistics;
        let mut objects = Vec::new();
        for i in 0..stats.object_count.min(100) {
            objects.push((format!("object_{i}"), "mesh".to_string()));
        }
        for i in 0..stats.light_count.min(10) {
            objects.push((format!("light_{i}"), "light".to_string()));
        }
        for i in 0..stats.camera_count.min(10) {
            objects.push((format!("camera_{i}"), "camera".to_string()));
        }
        Ok(objects)
    }

    fn get_selection(&self) -> DccResult<Vec<String>> {
        self.scene_query_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_selection")?;
        Ok(vec![])
    }
}
