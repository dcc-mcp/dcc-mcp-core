//! Implementation of the `DccSceneInfo` trait for `UsdStage`.
//!
//! This allows a `UsdStage` to act as a read-only DCC scene info provider,
//! which is useful when an Agent already has a USD representation of a scene
//! and wants to query it through the standard `DccSceneInfo` interface.

use dcc_mcp_protocols::adapters::{DccResult, DccSceneInfo, SceneInfo};

use crate::stage::UsdStage;

impl DccSceneInfo for UsdStage {
    /// Return a `SceneInfo` derived from the stage's root layer metadata and
    /// the `/World` prim's custom metadata.
    fn get_scene_info(&self) -> DccResult<SceneInfo> {
        Ok(crate::bridge::stage_to_scene_info(self))
    }

    /// Return all prim paths and their type names as (name, type) pairs.
    fn list_objects(&self) -> DccResult<Vec<(String, String)>> {
        let pairs = self
            .traverse()
            .into_iter()
            .map(|p| (p.path.to_string(), p.type_name.clone()))
            .collect();
        Ok(pairs)
    }

    /// USD stages do not track selection state; always returns empty.
    fn get_selection(&self) -> DccResult<Vec<String>> {
        Ok(Vec::new())
    }
}

/// A wrapper that allows a `UsdStage` to be used as a mock DCC adapter's
/// scene info source.
pub struct UsdSceneInfoAdapter {
    pub stage: UsdStage,
}

impl UsdSceneInfoAdapter {
    /// Create a new adapter wrapping the given stage.
    pub fn new(stage: UsdStage) -> Self {
        Self { stage }
    }
}

impl DccSceneInfo for UsdSceneInfoAdapter {
    fn get_scene_info(&self) -> DccResult<SceneInfo> {
        self.stage.get_scene_info()
    }

    fn list_objects(&self) -> DccResult<Vec<(String, String)>> {
        self.stage.list_objects()
    }

    fn get_selection(&self) -> DccResult<Vec<String>> {
        Ok(Vec::new())
    }
}

// Safety: UsdStage is Send + Sync by default (all fields are Send + Sync).
unsafe impl Send for UsdSceneInfoAdapter {}
unsafe impl Sync for UsdSceneInfoAdapter {}

/// Convert a `DccResult<T>` into a `crate::UsdResult<T>`.
fn _dcc_to_usd<T>(r: DccResult<T>) -> crate::UsdResult<T> {
    r.map_err(|e| crate::UsdError::ConversionError(e.message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::UsdStage;
    use crate::types::SdfPath;

    mod test_usd_stage_as_dcc_scene_info {
        use super::*;

        fn make_stage() -> UsdStage {
            let mut stage = UsdStage::new("test_scene");
            stage.root_layer.start_time_code = Some(1.0);
            stage.root_layer.end_time_code = Some(100.0);
            stage.root_layer.frames_per_second = Some(25.0);
            stage.define_prim(SdfPath::new("/World").unwrap(), "Xform");
            stage.define_prim(SdfPath::new("/World/Mesh1").unwrap(), "Mesh");
            stage.define_prim(SdfPath::new("/World/Cam").unwrap(), "Camera");
            stage
        }

        #[test]
        fn test_get_scene_info_frame_range() {
            let stage = make_stage();
            let info = stage.get_scene_info().unwrap();
            assert_eq!(info.frame_range, Some((1.0, 100.0)));
            assert_eq!(info.fps, Some(25.0));
        }

        #[test]
        fn test_list_objects_count() {
            let stage = make_stage();
            let objects = stage.list_objects().unwrap();
            // 3 prims defined
            assert_eq!(objects.len(), 3);
        }

        #[test]
        fn test_list_objects_contains_mesh() {
            let stage = make_stage();
            let objects = stage.list_objects().unwrap();
            let has_mesh = objects.iter().any(|(_, t)| t == "Mesh");
            assert!(has_mesh);
        }

        #[test]
        fn test_get_selection_empty() {
            let stage = make_stage();
            let sel = stage.get_selection().unwrap();
            assert!(sel.is_empty());
        }
    }

    mod test_usd_scene_info_adapter {
        use super::*;

        #[test]
        fn test_adapter_wraps_stage() {
            let stage = UsdStage::new("wrapped");
            let adapter = UsdSceneInfoAdapter::new(stage);
            let info = adapter.get_scene_info().unwrap();
            assert_eq!(info.name, "wrapped");
        }

        #[test]
        fn test_adapter_list_objects() {
            let mut stage = UsdStage::new("wrapped");
            stage.define_prim(SdfPath::new("/Root").unwrap(), "Xform");
            let adapter = UsdSceneInfoAdapter::new(stage);
            let objs = adapter.list_objects().unwrap();
            assert_eq!(objs.len(), 1);
            assert_eq!(objs[0].0, "/Root");
            assert_eq!(objs[0].1, "Xform");
        }
    }
}
