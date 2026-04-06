//! Tests for the mock DCC adapter.

use crate::adapters::{DccErrorCode, SceneInfo, SceneStatistics, ScriptLanguage};

use super::{MockConfig, MockDccAdapter};
use crate::adapters::{DccAdapter, DccConnection, DccSceneInfo, DccScriptEngine, DccSnapshot};

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

        let info = adapter.get_scene_info().unwrap();
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

        let info = adapter.get_scene_info().unwrap();
        assert_eq!(info.name, "shot_010");
        assert!(info.modified);
        assert_eq!(info.statistics.object_count, 100);
    }

    #[test]
    fn test_set_modified() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        adapter.set_modified(true);
        assert!(adapter.get_scene_info().unwrap().modified);

        adapter.set_modified(false);
        assert!(!adapter.get_scene_info().unwrap().modified);
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

        let objects = adapter.list_objects().unwrap();
        assert_eq!(objects.len(), 6); // 3 mesh + 2 light + 1 camera
        assert_eq!(objects[0].1, "mesh");
        assert_eq!(objects[3].1, "light");
        assert_eq!(objects[5].1, "camera");
    }

    #[test]
    fn test_get_selection() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        let selection = adapter.get_selection().unwrap();
        assert!(selection.is_empty());
    }

    #[test]
    fn test_scene_query_not_connected() {
        let adapter = MockDccAdapter::new();
        assert!(adapter.get_scene_info().is_err());
        assert!(adapter.list_objects().is_err());
        assert!(adapter.get_selection().is_err());
    }
}

// ── Snapshot tests ──

mod snapshot {
    use super::*;

    #[test]
    fn test_capture_viewport() {
        let mut adapter = MockDccAdapter::new();
        adapter.connect().unwrap();

        let result = adapter
            .capture_viewport(Some("persp"), Some(800), Some(600), "png")
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

        let result = adapter.capture_viewport(None, None, None, "png").unwrap();
        assert_eq!(result.width, 1920);
        assert_eq!(result.height, 1080);
    }

    #[test]
    fn test_snapshot_disabled() {
        let config = MockConfig::builder().snapshot_enabled(false).build();
        let mut adapter = MockDccAdapter::with_config(config);
        adapter.connect().unwrap();

        let result = adapter.capture_viewport(None, None, None, "png");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, DccErrorCode::Unsupported);
    }

    #[test]
    fn test_snapshot_not_connected() {
        let adapter = MockDccAdapter::new();
        let result = adapter.capture_viewport(None, None, None, "png");
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

        adapter.get_scene_info().unwrap();
        adapter.capture_viewport(None, None, None, "png").unwrap();
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
}
