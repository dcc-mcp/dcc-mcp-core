use std::sync::atomic::Ordering;

use crate::adapters::{DccError, DccErrorCode, DccResult, DccSceneManager, SceneInfo, SceneObject};

use super::MockDccAdapter;

impl DccSceneManager for MockDccAdapter {
    fn get_scene_info(&self) -> DccResult<SceneInfo> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_scene_info")?;
        Ok(self.scene.read().clone())
    }

    fn list_objects(&self, object_type: Option<&str>) -> DccResult<Vec<SceneObject>> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("list_objects")?;

        let objects = self.objects.read();
        let filtered = match object_type {
            None => objects.clone(),
            Some(ty) => objects
                .iter()
                .filter(|o| o.object_type == ty)
                .cloned()
                .collect(),
        };
        Ok(filtered)
    }

    fn new_scene(&self, _save_prompt: bool) -> DccResult<SceneInfo> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("new_scene")?;

        let new = SceneInfo {
            name: "untitled".to_string(),
            modified: false,
            ..Default::default()
        };
        *self.scene.write() = new.clone();
        self.objects.write().clear();
        self.selection.write().clear();
        Ok(new)
    }

    fn open_file(&self, file_path: &str, _force: bool) -> DccResult<SceneInfo> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("open_file")?;

        let name = file_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(file_path)
            .to_string();
        let info = SceneInfo {
            file_path: file_path.to_string(),
            name,
            modified: false,
            ..Default::default()
        };
        *self.scene.write() = info.clone();
        Ok(info)
    }

    fn save_file(&self, file_path: Option<&str>) -> DccResult<String> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("save_file")?;

        let path = file_path
            .map(String::from)
            .unwrap_or_else(|| self.scene.read().file_path.clone());
        self.scene.write().modified = false;
        Ok(path)
    }

    fn export_file(
        &self,
        file_path: &str,
        _format: &str,
        _selection_only: bool,
    ) -> DccResult<String> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("export_file")?;
        Ok(file_path.to_string())
    }

    fn get_selection(&self) -> DccResult<Vec<String>> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_selection")?;
        Ok(self.selection.read().clone())
    }

    fn set_selection(&self, object_names: &[&str]) -> DccResult<Vec<String>> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("set_selection")?;

        let names: Vec<String> = object_names.iter().map(|s| s.to_string()).collect();
        *self.selection.write() = names.clone();
        Ok(names)
    }

    fn select_by_type(&self, object_type: &str) -> DccResult<Vec<String>> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("select_by_type")?;

        let names: Vec<String> = self
            .objects
            .read()
            .iter()
            .filter(|o| o.object_type == object_type)
            .map(|o| o.name.clone())
            .collect();
        *self.selection.write() = names.clone();
        Ok(names)
    }

    fn set_visibility(&self, object_name: &str, visible: bool) -> DccResult<bool> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("set_visibility")?;

        let mut objects = self.objects.write();
        for obj in objects.iter_mut() {
            if obj.name == object_name || obj.long_name == object_name {
                obj.visible = visible;
                return Ok(visible);
            }
        }
        Err(DccError {
            code: DccErrorCode::InvalidInput,
            message: format!("Object not found: {object_name}"),
            details: None,
            recoverable: false,
        })
    }
}
