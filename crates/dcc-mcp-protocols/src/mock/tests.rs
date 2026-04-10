//! Tests for the mock DCC adapter.

use crate::adapters::{DccErrorCode, SceneInfo, SceneStatistics, ScriptLanguage};

use super::{MockConfig, MockDccAdapter};
use crate::adapters::{
    DccAdapter, DccConnection, DccHierarchy, DccRenderCapture, DccSceneInfo, DccSceneManager,
    DccScriptEngine, DccSnapshot,
};

// ── Construction tests ──

mod construction {
    use super::*;

    #[test]
    fn test_default_adapter() {
        let adapter = MockDccAdapter::new();
        assert_eq!(adapter.info().dcc_type, "mock");
        assert_eq!(adapter.info().version, "1.0.0");
        assert!(adapter.info().python_version.is_some());
        assert!(!adapter.is_connected());
    }

    #[test]
    fn test_custom_config() {
        let config = MockConfig::builder()
            .dcc_type("test_dcc")
            .version("2.0.0")
            .python_version("3.9.0")
            .platform("linux")
            .pid(42)
            .metadata("renderer", "cycles")
            .build();

        let adapter = MockDccAdapter::with_config(config);
        assert_eq!(adapter.info().dcc_type, "test_dcc");
        assert_eq!(adapter.info().version, "2.0.0");
        assert_eq!(adapter.info().python_version.as_deref(), Some("3.9.0"));
        assert_eq!(adapter.info().platform, "linux");
        assert_eq!(adapter.info().pid, 42);
        assert_eq!(adapter.info().metadata["renderer"], "cycles");
    }

    #[test]
    fn test_no_python_config() {
        let config = MockConfig::builder().no_python().build();
        let adapter = MockDccAdapter::with_config(config);
        assert!(adapter.info().python_version.is_none());
    }
}

// ── DCC preset tests ──

mod presets {
    use super::*;

    #[test]
    fn test_maya_preset() {
        let config = MockConfig::maya("2024.2");
        let adapter = MockDccAdapter::with_config(config);
        assert_eq!(adapter.info().dcc_type, "maya");
        assert_eq!(adapter.info().version, "2024.2");
        let caps = adapter.capabilities();
        assert!(caps.script_languages.contains(&ScriptLanguage::Python));
        assert!(caps.script_languages.contains(&ScriptLanguage::Mel));
    }

    #[test]
    fn test_blender_preset() {
        let config = MockConfig::blender("4.1.0");
        let adapter = MockDccAdapter::with_config(config);
        assert_eq!(adapter.info().dcc_type, "blender");
        assert_eq!(adapter.capabilities().script_languages.len(), 1);
    }

    #[test]
    fn test_houdini_preset() {
        let config = MockConfig::houdini("20.0.547");
        let adapter = MockDccAdapter::with_config(config);
        assert_eq!(adapter.info().dcc_type, "houdini");
        let caps = adapter.capabilities();
        assert_eq!(caps.script_languages.len(), 3);
        assert!(caps.script_languages.contains(&ScriptLanguage::Vex));
    }

    #[test]
    fn test_max_3ds_preset() {
        let config = MockConfig::max_3ds("2025");
        let adapter = MockDccAdapter::with_config(config);
        assert_eq!(adapter.info().dcc_type, "3dsmax");
        assert!(
            adapter
                .capabilities()
                .script_languages
                .contains(&ScriptLanguage::MaxScript)
        );
    }

    #[test]
    fn test_unreal_preset() {
        let config = MockConfig::unreal("5.4");
        let adapter = MockDccAdapter::with_config(config);
        assert_eq!(adapter.info().dcc_type, "unreal");
        assert!(
            adapter
                .capabilities()
                .script_languages
                .contains(&ScriptLanguage::Blueprint)
        );
    }

    #[test]
    fn test_unity_preset() {
        let config = MockConfig::unity("2022.3");
        let adapter = MockDccAdapter::with_config(config);
        assert_eq!(adapter.info().dcc_type, "unity");
        assert!(adapter.info().python_version.is_none());
        assert!(
            adapter
                .capabilities()
                .script_languages
                .contains(&ScriptLanguage::CSharp)
        );
    }
}

// ── Connection tests ──

mod connection {
    use super::*;

    #[test]
    fn test_connect_disconnect() {
        let mut adapter = MockDccAdapter::new();
        assert!(!adapter.is_connected());

        adapter.connect().unwrap();
        assert!(adapter.is_connected());
        assert_eq!(adapter.connect_count(), 1);

        adapter.disconnect().unwrap();
        assert!(!adapter.is_connected());
        assert_eq!(adapter.disconnect_count(), 1);
    }

    #[test]
    fn test_connect_failure() {
        let config = MockConfig::builder()
            .connect_should_fail("Test failure")
            .build();
        let mut adapter = MockDccAdapter::with_config(config);

        let result = adapter.connect();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, DccErrorCode::ConnectionFailed);
        assert!(err.message.contains("Test failure"));
        assert!(err.recoverable);
        assert!(!adapter.is_connected());
    }

    #[test]
    fn test_health_check_connected() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        let rtt = adapter.health_check().unwrap();
        assert_eq!(rtt, 1); // default latency
        assert_eq!(adapter.health_check_count(), 1);
    }

    #[test]
    fn test_health_check_disconnected() {
        let adapter = MockDccAdapter::new();
        let result = adapter.health_check();
        assert!(result.is_err());
    }

    #[test]
    fn test_custom_health_check_latency() {
        let config = MockConfig::builder().health_check_latency_ms(42).build();
        let mut adapter = MockDccAdapter::with_config(config);
        adapter.connect().unwrap();

        assert_eq!(adapter.health_check().unwrap(), 42);
    }
}

// ── Script execution tests ──

mod scripts {
    use super::*;

    #[test]
    fn test_execute_script_echo() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        let result = adapter
            .execute_script("print('hello')", ScriptLanguage::Python, None)
            .unwrap();

        assert!(result.success);
        assert_eq!(result.output.as_deref(), Some("print('hello')"));
        assert_eq!(adapter.script_count(), 1);
    }

    #[test]
    fn test_execute_script_unsupported_language() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        // Default mock only supports Python
        let result = adapter
            .execute_script("some mel code", ScriptLanguage::Mel, None)
            .unwrap();

        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("Unsupported"));
    }

    #[test]
    fn test_execute_script_not_connected() {
        let adapter = MockDccAdapter::new();
        let result = adapter.execute_script("code", ScriptLanguage::Python, None);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, DccErrorCode::ConnectionFailed);
    }

    #[test]
    fn test_custom_script_handler() {
        let config = MockConfig::builder()
            .script_handler(|code, _lang, _timeout| {
                if code.contains("error") {
                    Err("Simulated error".to_string())
                } else {
                    Ok(format!("result: {code}"))
                }
            })
            .build();
        let mut adapter = MockDccAdapter::with_config(config);
        adapter.connect().unwrap();

        let ok_result = adapter
            .execute_script("hello", ScriptLanguage::Python, None)
            .unwrap();
        assert!(ok_result.success);
        assert_eq!(ok_result.output.as_deref(), Some("result: hello"));

        let err_result = adapter
            .execute_script("trigger error", ScriptLanguage::Python, None)
            .unwrap();
        assert!(!err_result.success);
        assert!(err_result.error.as_deref().unwrap().contains("Simulated"));
    }

    #[test]
    fn test_supported_languages() {
        let config = MockConfig::maya("2024");
        let adapter = MockDccAdapter::with_config(config);

        let langs = adapter.supported_languages();
        assert_eq!(langs.len(), 2);
        assert!(langs.contains(&ScriptLanguage::Python));
        assert!(langs.contains(&ScriptLanguage::Mel));
    }
}

// ── Scene info tests ──

mod scene {
    use super::*;

    #[test]
    fn test_get_scene_info() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        let info = DccSceneInfo::get_scene_info(&adapter).unwrap();
        assert_eq!(info.name, "untitled");
        assert!(!info.modified);
        assert_eq!(adapter.scene_query_count(), 1);
    }

    #[test]
    fn test_set_scene() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        adapter.set_scene(SceneInfo {
            file_path: "/projects/shot_010.ma".to_string(),
            name: "shot_010".to_string(),
            modified: true,
            format: ".ma".to_string(),
            statistics: SceneStatistics {
                object_count: 100,
                vertex_count: 50000,
                ..Default::default()
            },
            ..Default::default()
        });

        let info = DccSceneInfo::get_scene_info(&adapter).unwrap();
        assert_eq!(info.name, "shot_010");
        assert!(info.modified);
        assert_eq!(info.statistics.object_count, 100);
    }

    #[test]
    fn test_set_modified() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        adapter.set_modified(true);
        assert!(DccSceneInfo::get_scene_info(&adapter).unwrap().modified);

        adapter.set_modified(false);
        assert!(!DccSceneInfo::get_scene_info(&adapter).unwrap().modified);
    }

    #[test]
    fn test_list_objects() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        adapter.set_statistics(SceneStatistics {
            object_count: 3,
            light_count: 2,
            camera_count: 1,
            ..Default::default()
        });

        let objects = DccSceneInfo::list_objects(&adapter).unwrap();
        assert_eq!(objects.len(), 6); // 3 mesh + 2 light + 1 camera
        assert_eq!(objects[0].1, "mesh");
        assert_eq!(objects[3].1, "light");
        assert_eq!(objects[5].1, "camera");
    }

    #[test]
    fn test_get_selection() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        let selection = DccSceneInfo::get_selection(&adapter).unwrap();
        assert!(selection.is_empty());
    }

    #[test]
    fn test_scene_query_not_connected() {
        let adapter = MockDccAdapter::new();
        assert!(DccSceneInfo::get_scene_info(&adapter).is_err());
        assert!(DccSceneInfo::list_objects(&adapter).is_err());
        assert!(DccSceneInfo::get_selection(&adapter).is_err());
    }
}

// ── Snapshot tests ──

mod snapshot {
    use super::*;

    #[test]
    fn test_capture_viewport() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        let result =
            DccSnapshot::capture_viewport(&adapter, Some("persp"), Some(800), Some(600), "png")
                .unwrap();

        assert_eq!(result.width, 800);
        assert_eq!(result.height, 600);
        assert_eq!(result.format, "png");
        assert_eq!(result.viewport.as_deref(), Some("persp"));
        assert!(!result.data.is_empty());
        assert_eq!(adapter.snapshot_count(), 1);
    }

    #[test]
    fn test_capture_default_resolution() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        let result = DccSnapshot::capture_viewport(&adapter, None, None, None, "png").unwrap();
        assert_eq!(result.width, 1920);
        assert_eq!(result.height, 1080);
    }

    #[test]
    fn test_snapshot_disabled() {
        let config = MockConfig::builder().snapshot_enabled(false).build();
        let mut adapter = MockDccAdapter::with_config(config);
        adapter.connect().unwrap();

        let result = DccSnapshot::capture_viewport(&adapter, None, None, None, "png");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, DccErrorCode::Unsupported);
    }

    #[test]
    fn test_snapshot_not_connected() {
        let adapter = MockDccAdapter::new();
        let result = DccSnapshot::capture_viewport(&adapter, None, None, None, "png");
        assert!(result.is_err());
    }
}

// ── DccAdapter trait tests ──

mod adapter_trait {
    use super::*;

    #[test]
    fn test_capabilities() {
        let adapter = MockDccAdapter::new();
        let caps = adapter.capabilities();
        assert!(caps.scene_info);
        assert!(caps.snapshot);
        assert!(caps.selection);
        assert!(caps.file_operations);
    }

    #[test]
    fn test_as_connection() {
        let mut adapter = MockDccAdapter::new();
        assert!(adapter.as_connection().is_some());
    }

    #[test]
    fn test_as_script_engine() {
        let adapter = MockDccAdapter::new();
        assert!(adapter.as_script_engine().is_some());
    }

    #[test]
    fn test_as_scene_info() {
        let adapter = MockDccAdapter::new();
        assert!(adapter.as_scene_info().is_some());
    }

    #[test]
    fn test_as_snapshot_enabled() {
        let adapter = MockDccAdapter::new();
        assert!(adapter.as_snapshot().is_some());
    }

    #[test]
    fn test_as_snapshot_disabled() {
        let config = MockConfig::builder().snapshot_enabled(false).build();
        let adapter = MockDccAdapter::with_config(config);
        assert!(adapter.as_snapshot().is_none());
    }
}

// ── Counter tests ──

mod counters {
    use super::*;

    #[test]
    fn test_invocation_counters() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();
        adapter.connect().unwrap(); // connect twice

        adapter
            .execute_script("a", ScriptLanguage::Python, None)
            .unwrap();
        adapter
            .execute_script("b", ScriptLanguage::Python, None)
            .unwrap();
        adapter
            .execute_script("c", ScriptLanguage::Python, None)
            .unwrap();

        DccSceneInfo::get_scene_info(&adapter).unwrap();
        DccSnapshot::capture_viewport(&adapter, None, None, None, "png").unwrap();
        adapter.health_check().unwrap();

        assert_eq!(adapter.connect_count(), 2);
        assert_eq!(adapter.script_count(), 3);
        assert_eq!(adapter.scene_query_count(), 1);
        assert_eq!(adapter.snapshot_count(), 1);
        assert_eq!(adapter.health_check_count(), 1);
    }

    #[test]
    fn test_reset_counters() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();
        adapter
            .execute_script("x", ScriptLanguage::Python, None)
            .unwrap();

        adapter.reset_counters();

        assert_eq!(adapter.connect_count(), 0);
        assert_eq!(adapter.script_count(), 0);
        assert_eq!(adapter.scene_query_count(), 0);
        assert_eq!(adapter.snapshot_count(), 0);
        assert_eq!(adapter.health_check_count(), 0);
        assert_eq!(adapter.disconnect_count(), 0);
    }

    #[test]
    fn test_cross_protocol_counters() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        // DccSceneManager calls
        let sm = adapter.as_scene_manager().unwrap();
        let _ = sm.list_objects(None);
        let _ = sm.get_selection();
        assert_eq!(adapter.scene_manager_count(), 2);

        // DccTransform calls
        let tf = adapter.as_transform().unwrap();
        let _ = tf.get_transform("pCube1");
        assert_eq!(adapter.transform_count(), 1);

        // DccRenderCapture calls
        let rc = adapter.as_render_capture().unwrap();
        let _ = rc.get_render_settings();
        assert_eq!(adapter.render_capture_count(), 1);

        // DccHierarchy calls
        let hier = adapter.as_hierarchy().unwrap();
        let _ = hier.get_hierarchy();
        assert_eq!(adapter.hierarchy_count(), 1);
    }
}

// ── DccSceneManager tests ──

mod scene_manager {
    use super::*;
    use crate::adapters::SceneObject;
    use std::collections::HashMap;

    fn connected_adapter() -> MockDccAdapter {
        let mut a = MockDccAdapter::new();
        a.connect().unwrap();
        a
    }

    #[test]
    fn test_list_objects_all() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();
        let objects = sm.list_objects(None).unwrap();
        // Default config has pCube1 (mesh) + persp (camera)
        assert_eq!(objects.len(), 2);
    }

    #[test]
    fn test_list_objects_filtered_by_type() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();

        let meshes = sm.list_objects(Some("mesh")).unwrap();
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].name, "pCube1");

        let cameras = sm.list_objects(Some("camera")).unwrap();
        assert_eq!(cameras.len(), 1);
        assert_eq!(cameras[0].name, "persp");

        let lights = sm.list_objects(Some("light")).unwrap();
        assert!(lights.is_empty());
    }

    #[test]
    fn test_list_objects_not_connected() {
        let adapter = MockDccAdapter::new();
        let sm = adapter.as_scene_manager().unwrap();
        assert!(sm.list_objects(None).is_err());
    }

    #[test]
    fn test_get_set_selection() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();

        // Initially empty
        assert!(sm.get_selection().unwrap().is_empty());

        // Set selection
        let selected = sm.set_selection(&["pCube1", "persp"]).unwrap();
        assert_eq!(selected.len(), 2);
        assert!(selected.contains(&"pCube1".to_string()));

        // Read back
        let current = sm.get_selection().unwrap();
        assert_eq!(current, vec!["pCube1", "persp"]);
    }

    #[test]
    fn test_select_by_type() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();

        let meshes = sm.select_by_type("mesh").unwrap();
        assert_eq!(meshes, vec!["pCube1"]);

        // Selection should be updated
        assert_eq!(sm.get_selection().unwrap(), vec!["pCube1"]);
    }

    #[test]
    fn test_set_visibility() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();

        // Hide the cube
        let result = sm.set_visibility("pCube1", false).unwrap();
        assert!(!result);

        // Verify via list_objects
        let objects = sm.list_objects(Some("mesh")).unwrap();
        assert!(!objects[0].visible);

        // Show it again
        sm.set_visibility("pCube1", true).unwrap();
        let objects = sm.list_objects(Some("mesh")).unwrap();
        assert!(objects[0].visible);
    }

    #[test]
    fn test_set_visibility_not_found() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();
        let err = sm.set_visibility("nonexistent", true).unwrap_err();
        assert_eq!(err.code, DccErrorCode::InvalidInput);
    }

    #[test]
    fn test_new_scene() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();

        let info = sm.new_scene(false).unwrap();
        assert_eq!(info.name, "untitled");
        assert!(!info.modified);

        // Objects should be cleared
        let objects = sm.list_objects(None).unwrap();
        assert!(objects.is_empty());
    }

    #[test]
    fn test_open_file() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();

        let info = sm.open_file("/projects/shot_010.ma", false).unwrap();
        assert_eq!(info.file_path, "/projects/shot_010.ma");
        assert_eq!(info.name, "shot_010.ma");
        assert!(!info.modified);
    }

    #[test]
    fn test_save_file_inplace() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();

        // Open a file first to set a path
        sm.open_file("/projects/shot.ma", false).unwrap();
        let saved = sm.save_file(None).unwrap();
        assert_eq!(saved, "/projects/shot.ma");
    }

    #[test]
    fn test_save_file_new_path() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();
        let saved = sm.save_file(Some("/export/final.ma")).unwrap();
        assert_eq!(saved, "/export/final.ma");
    }

    #[test]
    fn test_export_file() {
        let adapter = connected_adapter();
        let sm = adapter.as_scene_manager().unwrap();
        let path = sm.export_file("/export/model.fbx", "fbx", false).unwrap();
        assert_eq!(path, "/export/model.fbx");
    }

    #[test]
    fn test_set_objects_via_helper() {
        let adapter = connected_adapter();
        adapter.set_objects(vec![
            SceneObject {
                name: "cube".to_string(),
                long_name: "|cube".to_string(),
                object_type: "mesh".to_string(),
                parent: None,
                visible: true,
                metadata: HashMap::new(),
            },
            SceneObject {
                name: "sphere".to_string(),
                long_name: "|sphere".to_string(),
                object_type: "mesh".to_string(),
                parent: None,
                visible: true,
                metadata: HashMap::new(),
            },
        ]);
        let sm = adapter.as_scene_manager().unwrap();
        let objects = sm.list_objects(None).unwrap();
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0].name, "cube");
        assert_eq!(objects[1].name, "sphere");
    }
}

// ── DccTransform tests ──

mod transform {
    use super::*;
    use crate::adapters::BoundingBox;

    fn connected_adapter() -> MockDccAdapter {
        let mut a = MockDccAdapter::new();
        a.connect().unwrap();
        a
    }

    #[test]
    fn test_get_transform_default_object() {
        let adapter = connected_adapter();
        let tf = adapter.as_transform().unwrap();

        let t = tf.get_transform("pCube1").unwrap();
        // Default config registers identity transform for pCube1
        assert_eq!(t.translate, [0.0, 0.0, 0.0]);
        assert_eq!(t.rotate, [0.0, 0.0, 0.0]);
        assert_eq!(t.scale, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn test_get_transform_not_found() {
        let adapter = connected_adapter();
        let tf = adapter.as_transform().unwrap();
        let err = tf.get_transform("nonexistent").unwrap_err();
        assert_eq!(err.code, DccErrorCode::InvalidInput);
    }

    #[test]
    fn test_set_transform_translate_only() {
        let adapter = connected_adapter();
        let tf = adapter.as_transform().unwrap();

        let result = tf
            .set_transform("pCube1", Some([10.0, 20.0, 30.0]), None, None)
            .unwrap();
        assert_eq!(result.translate, [10.0, 20.0, 30.0]);
        assert_eq!(result.rotate, [0.0, 0.0, 0.0]); // unchanged
        assert_eq!(result.scale, [1.0, 1.0, 1.0]); // unchanged
    }

    #[test]
    fn test_set_transform_all_components() {
        let adapter = connected_adapter();
        let tf = adapter.as_transform().unwrap();

        let result = tf
            .set_transform(
                "pCube1",
                Some([5.0, 0.0, -5.0]),
                Some([0.0, 45.0, 0.0]),
                Some([2.0, 2.0, 2.0]),
            )
            .unwrap();
        assert_eq!(result.translate, [5.0, 0.0, -5.0]);
        assert_eq!(result.rotate, [0.0, 45.0, 0.0]);
        assert_eq!(result.scale, [2.0, 2.0, 2.0]);

        // Confirm persisted
        let fetched = tf.get_transform("pCube1").unwrap();
        assert_eq!(fetched.translate, [5.0, 0.0, -5.0]);
    }

    #[test]
    fn test_set_transform_creates_new_object() {
        let adapter = connected_adapter();
        let tf = adapter.as_transform().unwrap();

        // "newObj" doesn't exist yet — set_transform should create an identity entry
        let result = tf
            .set_transform("newObj", Some([1.0, 2.0, 3.0]), None, None)
            .unwrap();
        assert_eq!(result.translate, [1.0, 2.0, 3.0]);
        assert_eq!(result.scale, [1.0, 1.0, 1.0]); // identity default
    }

    #[test]
    fn test_get_bounding_box() {
        let adapter = connected_adapter();
        let tf = adapter.as_transform().unwrap();

        let bb = tf.get_bounding_box("pCube1").unwrap();
        assert_eq!(bb.min, [-1.0, 0.0, -1.0]);
        assert_eq!(bb.max, [1.0, 2.0, 1.0]);
        assert_eq!(bb.center(), [0.0, 1.0, 0.0]);
        assert_eq!(bb.size(), [2.0, 2.0, 2.0]);
    }

    #[test]
    fn test_get_bounding_box_not_found() {
        let adapter = connected_adapter();
        let tf = adapter.as_transform().unwrap();
        let err = tf.get_bounding_box("missing").unwrap_err();
        assert_eq!(err.code, DccErrorCode::InvalidInput);
    }

    #[test]
    fn test_register_bounding_box_helper() {
        let adapter = connected_adapter();
        adapter.register_bounding_box(
            "light_01",
            BoundingBox {
                min: [0.0, 0.0, 0.0],
                max: [0.1, 0.1, 0.1],
            },
        );
        let tf = adapter.as_transform().unwrap();
        let bb = tf.get_bounding_box("light_01").unwrap();
        assert_eq!(bb.size(), [0.1, 0.1, 0.1]);
    }

    #[test]
    fn test_rename_object() {
        let adapter = connected_adapter();
        let tf = adapter.as_transform().unwrap();

        let new_name = tf.rename_object("pCube1", "myCube").unwrap();
        assert_eq!(new_name, "myCube");

        // Original name is gone from object list
        let sm = adapter.as_scene_manager().unwrap();
        let meshes = sm.list_objects(Some("mesh")).unwrap();
        assert_eq!(meshes[0].name, "myCube");
    }

    #[test]
    fn test_rename_object_not_found() {
        let adapter = connected_adapter();
        let tf = adapter.as_transform().unwrap();
        let err = tf.rename_object("ghost", "new").unwrap_err();
        assert_eq!(err.code, DccErrorCode::InvalidInput);
    }

    #[test]
    fn test_transform_not_connected() {
        let adapter = MockDccAdapter::new();
        let tf = adapter.as_transform().unwrap();
        assert!(tf.get_transform("pCube1").is_err());
        assert!(tf.set_transform("pCube1", None, None, None).is_err());
    }
}

// ── DccRenderCapture tests ──

mod render_capture {
    use super::*;
    use std::collections::HashMap;

    fn connected_adapter() -> MockDccAdapter {
        let mut a = MockDccAdapter::new();
        a.connect().unwrap();
        a
    }

    #[test]
    fn test_capture_viewport_via_render_capture_trait() {
        let adapter = connected_adapter();
        let rc = adapter.as_render_capture().unwrap();

        let result = rc
            .capture_viewport(Some("top"), Some(1280), Some(720), "jpeg")
            .unwrap();
        assert_eq!(result.width, 1280);
        assert_eq!(result.height, 720);
        assert_eq!(result.format, "jpeg");
        assert_eq!(result.viewport.as_deref(), Some("top"));
        assert!(!result.data.is_empty());
    }

    #[test]
    fn test_render_scene() {
        let adapter = connected_adapter();
        let rc = adapter.as_render_capture().unwrap();

        let out = rc
            .render_scene(
                "/renders/frame001.png",
                Some(1920),
                Some(1080),
                Some("arnold"),
            )
            .unwrap();
        assert_eq!(out.file_path, "/renders/frame001.png");
        assert_eq!(out.width, 1920);
        assert_eq!(out.height, 1080);
        assert_eq!(out.format, "png");
        assert_eq!(out.render_time_ms, 100); // default
    }

    #[test]
    fn test_render_scene_uses_settings_resolution() {
        let adapter = connected_adapter();
        let rc = adapter.as_render_capture().unwrap();

        // Use None for resolution — should fall back to render settings (1920x1080)
        let out = rc.render_scene("/out/frame.exr", None, None, None).unwrap();
        assert_eq!(out.width, 1920);
        assert_eq!(out.height, 1080);
        assert_eq!(out.format, "exr");
    }

    #[test]
    fn test_get_render_settings() {
        let adapter = connected_adapter();
        let rc = adapter.as_render_capture().unwrap();

        let settings = rc.get_render_settings().unwrap();
        assert_eq!(settings["width"], "1920");
        assert_eq!(settings["height"], "1080");
        assert_eq!(settings["renderer"], "default");
    }

    #[test]
    fn test_set_render_settings_partial_update() {
        let adapter = connected_adapter();
        let rc = adapter.as_render_capture().unwrap();

        let mut updates = HashMap::new();
        updates.insert("renderer".to_string(), "arnold".to_string());
        updates.insert("samples".to_string(), "256".to_string());
        rc.set_render_settings(updates).unwrap();

        let settings = rc.get_render_settings().unwrap();
        assert_eq!(settings["renderer"], "arnold");
        assert_eq!(settings["samples"], "256");
        // Other keys should remain untouched
        assert_eq!(settings["width"], "1920");
    }

    #[test]
    fn test_render_capture_not_exposed_when_snapshot_disabled() {
        let config = MockConfig::builder().snapshot_enabled(false).build();
        let adapter = MockDccAdapter::with_config(config);
        // as_render_capture returns None when snapshot_enabled is false
        assert!(adapter.as_render_capture().is_none());
    }

    #[test]
    fn test_render_capture_not_connected() {
        let adapter = MockDccAdapter::new();
        let rc = adapter.as_render_capture().unwrap();
        assert!(rc.get_render_settings().is_err());
        assert!(rc.render_scene("/out/x.png", None, None, None).is_err());
    }

    #[test]
    fn test_render_counter_increments() {
        let adapter = connected_adapter();
        let rc = adapter.as_render_capture().unwrap();
        rc.capture_viewport(None, None, None, "png").unwrap();
        rc.render_scene("/x.png", None, None, None).unwrap();
        rc.get_render_settings().unwrap();
        assert_eq!(adapter.render_capture_count(), 3);
    }
}

// ── DccHierarchy tests ──

mod hierarchy {
    use super::*;
    use crate::adapters::{SceneNode, SceneObject};
    use std::collections::HashMap;

    fn connected_adapter() -> MockDccAdapter {
        let mut a = MockDccAdapter::new();
        a.connect().unwrap();
        a
    }

    #[test]
    fn test_get_hierarchy_default() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();
        let tree = hier.get_hierarchy().unwrap();
        // Default: pCube1 + persp as root nodes
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].object.name, "pCube1");
        assert_eq!(tree[1].object.name, "persp");
    }

    #[test]
    fn test_get_children_root() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();

        // None = root children
        let children = hier.get_children(None).unwrap();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_get_children_named_node_with_children() {
        // Build a hierarchy: root_grp → [cube, sphere]
        let cube = SceneObject {
            name: "cube".to_string(),
            long_name: "|root_grp|cube".to_string(),
            object_type: "mesh".to_string(),
            parent: Some("|root_grp".to_string()),
            visible: true,
            metadata: HashMap::new(),
        };
        let sphere = SceneObject {
            name: "sphere".to_string(),
            long_name: "|root_grp|sphere".to_string(),
            object_type: "mesh".to_string(),
            parent: Some("|root_grp".to_string()),
            visible: true,
            metadata: HashMap::new(),
        };
        let group = SceneObject {
            name: "root_grp".to_string(),
            long_name: "|root_grp".to_string(),
            object_type: "group".to_string(),
            parent: None,
            visible: true,
            metadata: HashMap::new(),
        };

        let tree = vec![SceneNode {
            object: group,
            children: vec![
                SceneNode {
                    object: cube,
                    children: vec![],
                },
                SceneNode {
                    object: sphere,
                    children: vec![],
                },
            ],
        }];

        let config = MockConfig::builder().build();
        let mut adapter = MockDccAdapter::with_config(config);
        adapter.connect().unwrap();
        *adapter.hierarchy.write() = tree;

        let hier = adapter.as_hierarchy().unwrap();
        let children = hier.get_children(Some("root_grp")).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, "cube");
        assert_eq!(children[1].name, "sphere");
    }

    #[test]
    fn test_get_children_leaf_returns_empty() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();
        // pCube1 has no children in default config
        let children = hier.get_children(Some("pCube1")).unwrap();
        assert!(children.is_empty());
    }

    #[test]
    fn test_get_parent_no_parent() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();
        // pCube1 has no parent in default config
        let parent = hier.get_parent("pCube1").unwrap();
        assert!(parent.is_none());
    }

    #[test]
    fn test_get_parent_with_parent() {
        let adapter = connected_adapter();
        // Add an object that has a parent
        adapter.objects.write().push(crate::adapters::SceneObject {
            name: "child".to_string(),
            long_name: "|grp|child".to_string(),
            object_type: "mesh".to_string(),
            parent: Some("|grp".to_string()),
            visible: true,
            metadata: HashMap::new(),
        });
        let hier = adapter.as_hierarchy().unwrap();
        let parent = hier.get_parent("child").unwrap();
        assert_eq!(parent.as_deref(), Some("|grp"));
    }

    #[test]
    fn test_group_objects() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();

        let group = hier
            .group_objects(&["pCube1", "persp"], "myGroup", None)
            .unwrap();
        assert_eq!(group.name, "myGroup");
        assert_eq!(group.object_type, "group");
        assert!(group.parent.is_none());

        // Group should now be in the object list
        let sm = adapter.as_scene_manager().unwrap();
        let objects = sm.list_objects(None).unwrap();
        assert!(objects.iter().any(|o| o.name == "myGroup"));
    }

    #[test]
    fn test_group_objects_with_parent() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();

        let group = hier
            .group_objects(&["pCube1"], "subGroup", Some("|rootGrp"))
            .unwrap();
        assert_eq!(group.parent.as_deref(), Some("|rootGrp"));
    }

    #[test]
    fn test_ungroup() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();

        // First create a group
        hier.group_objects(&["pCube1", "persp"], "tempGroup", None)
            .unwrap();

        // Now ungroup it
        let released = hier.ungroup("tempGroup").unwrap();
        assert_eq!(released, vec!["pCube1", "persp"]);

        // Group should be gone
        let sm = adapter.as_scene_manager().unwrap();
        let objects = sm.list_objects(None).unwrap();
        assert!(!objects.iter().any(|o| o.name == "tempGroup"));
    }

    #[test]
    fn test_ungroup_not_found() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();
        let err = hier.ungroup("doesNotExist").unwrap_err();
        assert_eq!(err.code, DccErrorCode::InvalidInput);
    }

    #[test]
    fn test_reparent() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();

        let updated = hier.reparent("pCube1", Some("|world"), false).unwrap();
        assert_eq!(updated.parent.as_deref(), Some("|world"));

        // Verify via get_parent
        let parent = hier.get_parent("pCube1").unwrap();
        assert_eq!(parent.as_deref(), Some("|world"));
    }

    #[test]
    fn test_reparent_to_root() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();

        // Give pCube1 a parent first
        hier.reparent("pCube1", Some("|grp"), false).unwrap();
        // Then reparent to root
        let updated = hier.reparent("pCube1", None, false).unwrap();
        assert!(updated.parent.is_none());
    }

    #[test]
    fn test_reparent_not_found() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();
        let err = hier.reparent("ghost", Some("|x"), false).unwrap_err();
        assert_eq!(err.code, DccErrorCode::InvalidInput);
    }

    #[test]
    fn test_hierarchy_not_connected() {
        let adapter = MockDccAdapter::new();
        let hier = adapter.as_hierarchy().unwrap();
        assert!(hier.get_hierarchy().is_err());
        assert!(hier.get_children(None).is_err());
        assert!(hier.get_parent("x").is_err());
    }

    #[test]
    fn test_hierarchy_counter_increments() {
        let adapter = connected_adapter();
        let hier = adapter.as_hierarchy().unwrap();
        hier.get_hierarchy().unwrap();
        hier.get_children(None).unwrap();
        hier.get_parent("pCube1").unwrap();
        assert_eq!(adapter.hierarchy_count(), 3);
    }
}

// ── DccAdapter cross-protocol accessor tests ──

mod adapter_cross_protocol {
    use super::*;

    #[test]
    fn test_all_cross_protocol_traits_available() {
        let adapter = MockDccAdapter::new();
        assert!(adapter.as_scene_manager().is_some());
        assert!(adapter.as_transform().is_some());
        assert!(adapter.as_render_capture().is_some());
        assert!(adapter.as_hierarchy().is_some());
    }

    #[test]
    fn test_render_capture_none_when_snapshot_disabled() {
        let config = MockConfig::builder().snapshot_enabled(false).build();
        let adapter = MockDccAdapter::with_config(config);
        assert!(adapter.as_render_capture().is_none());
        // Other traits still available
        assert!(adapter.as_scene_manager().is_some());
        assert!(adapter.as_transform().is_some());
        assert!(adapter.as_hierarchy().is_some());
    }

    #[test]
    fn test_photoshop_preset_has_all_traits() {
        let config = MockConfig::photoshop("25.0");
        let adapter = MockDccAdapter::with_config(config);
        assert!(adapter.as_scene_manager().is_some());
        assert!(adapter.as_transform().is_some());
        assert!(adapter.as_render_capture().is_some());
        assert!(adapter.as_hierarchy().is_some());
        assert_eq!(adapter.info().dcc_type, "photoshop");
    }

    #[test]
    fn test_unity_preset_has_all_traits() {
        let config = MockConfig::unity("2022.3");
        let adapter = MockDccAdapter::with_config(config);
        assert!(adapter.as_scene_manager().is_some());
        assert!(adapter.as_hierarchy().is_some());
        assert!(adapter.info().python_version.is_none());
    }
}
