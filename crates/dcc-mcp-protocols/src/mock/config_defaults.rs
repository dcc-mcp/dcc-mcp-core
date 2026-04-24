use std::collections::HashMap;

use crate::adapters::{
    BoundingBox, ObjectTransform, SceneInfo, SceneNode, SceneObject, SceneStatistics,
    ScriptLanguage,
};

use super::MockConfig;
use crate::mock::helpers::{current_platform, minimal_png};

impl Default for MockConfig {
    fn default() -> Self {
        let mesh = SceneObject {
            name: "pCube1".to_string(),
            long_name: "|pCube1".to_string(),
            object_type: "mesh".to_string(),
            parent: None,
            visible: true,
            metadata: HashMap::new(),
        };
        let camera = SceneObject {
            name: "persp".to_string(),
            long_name: "|persp".to_string(),
            object_type: "camera".to_string(),
            parent: None,
            visible: true,
            metadata: HashMap::new(),
        };

        let mut transforms = HashMap::new();
        transforms.insert("pCube1".to_string(), ObjectTransform::identity());
        transforms.insert("|pCube1".to_string(), ObjectTransform::identity());

        let mut bounding_boxes = HashMap::new();
        bounding_boxes.insert(
            "pCube1".to_string(),
            BoundingBox {
                min: [-1.0, 0.0, -1.0],
                max: [1.0, 2.0, 1.0],
            },
        );

        let mut render_settings = HashMap::new();
        render_settings.insert("width".to_string(), "1920".to_string());
        render_settings.insert("height".to_string(), "1080".to_string());
        render_settings.insert("renderer".to_string(), "default".to_string());
        render_settings.insert("samples".to_string(), "64".to_string());

        let mesh_node = SceneNode {
            object: mesh.clone(),
            children: vec![],
        };
        let camera_node = SceneNode {
            object: camera.clone(),
            children: vec![],
        };

        Self {
            dcc_type: "mock".to_string(),
            version: "1.0.0".to_string(),
            python_version: Some("3.11.0".to_string()),
            platform: current_platform().to_string(),
            pid: std::process::id(),
            metadata: HashMap::new(),
            supported_languages: vec![ScriptLanguage::Python],
            scene: SceneInfo {
                file_path: String::new(),
                name: "untitled".to_string(),
                modified: false,
                format: ".mock".to_string(),
                frame_range: Some((1.0, 100.0)),
                current_frame: Some(1.0),
                fps: Some(24.0),
                up_axis: Some("y".to_string()),
                units: Some("cm".to_string()),
                statistics: SceneStatistics::default(),
                metadata: HashMap::new(),
            },
            snapshot_enabled: true,
            snapshot_data: minimal_png(),
            script_handler: None,
            health_check_latency_ms: 1,
            connect_should_fail: false,
            connect_error_message: "Simulated connection failure".to_string(),
            objects: vec![mesh, camera],
            selection: vec![],
            hierarchy: vec![mesh_node, camera_node],
            transforms,
            bounding_boxes,
            render_time_ms: 100,
            render_settings,
        }
    }
}
