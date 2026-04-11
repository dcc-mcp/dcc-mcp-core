use std::collections::HashMap;

use super::traits::{DccAdapter, DccConnection, DccSceneInfo, DccScriptEngine, DccSnapshot};
use super::types::{
    BoundingBox, DccCapabilities, DccError, DccErrorCode, DccInfo, FrameRange, ObjectTransform,
    SceneInfo, SceneObject, SceneStatistics, ScriptLanguage, ScriptResult,
};

// ── Data structure tests ──

#[test]
fn test_dcc_info_serialization() {
    let info = DccInfo {
        dcc_type: "maya".to_string(),
        version: "2024.2".to_string(),
        python_version: Some("3.10.11".to_string()),
        platform: "windows".to_string(),
        pid: 12345,
        metadata: HashMap::from([("renderer".to_string(), "arnold".to_string())]),
    };
    let json = serde_json::to_string(&info).unwrap();
    let deserialized: DccInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.dcc_type, "maya");
    assert_eq!(deserialized.pid, 12345);
    assert_eq!(deserialized.metadata["renderer"], "arnold");
}

#[test]
fn test_script_result_success() {
    let result = ScriptResult::success("42", 100);
    assert!(result.success);
    assert_eq!(result.output.as_deref(), Some("42"));
    assert!(result.error.is_none());
    assert_eq!(result.execution_time_ms, 100);
}

#[test]
fn test_script_result_failure() {
    let result = ScriptResult::failure("NameError: undefined variable", 50);
    assert!(!result.success);
    assert!(result.output.is_none());
    assert_eq!(
        result.error.as_deref(),
        Some("NameError: undefined variable")
    );
}

#[test]
fn test_script_language_display() {
    assert_eq!(ScriptLanguage::Python.to_string(), "python");
    assert_eq!(ScriptLanguage::Mel.to_string(), "mel");
    assert_eq!(ScriptLanguage::MaxScript.to_string(), "maxscript");
    assert_eq!(ScriptLanguage::HScript.to_string(), "hscript");
    assert_eq!(ScriptLanguage::Vex.to_string(), "vex");
    assert_eq!(ScriptLanguage::Lua.to_string(), "lua");
    assert_eq!(ScriptLanguage::CSharp.to_string(), "csharp");
    assert_eq!(ScriptLanguage::Blueprint.to_string(), "blueprint");
}

#[test]
fn test_script_language_serialization_roundtrip() {
    let lang = ScriptLanguage::Python;
    let json = serde_json::to_string(&lang).unwrap();
    assert_eq!(json, "\"python\"");
    let deserialized: ScriptLanguage = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ScriptLanguage::Python);
}

#[test]
fn test_scene_info_default() {
    let scene = SceneInfo::default();
    assert!(scene.file_path.is_empty());
    assert!(!scene.modified);
    assert!(scene.frame_range.is_none());
}

#[test]
fn test_scene_info_serialization() {
    let scene = SceneInfo {
        file_path: "/projects/shot_010.ma".to_string(),
        name: "shot_010".to_string(),
        modified: true,
        format: ".ma".to_string(),
        frame_range: Some((1.0, 120.0)),
        current_frame: Some(24.0),
        fps: Some(24.0),
        up_axis: Some("y".to_string()),
        units: Some("cm".to_string()),
        statistics: SceneStatistics {
            object_count: 150,
            vertex_count: 500_000,
            polygon_count: 250_000,
            material_count: 20,
            texture_count: 45,
            light_count: 5,
            camera_count: 3,
        },
        metadata: HashMap::new(),
    };
    let json = serde_json::to_string(&scene).unwrap();
    let deserialized: SceneInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "shot_010");
    assert!(deserialized.modified);
    assert_eq!(deserialized.statistics.vertex_count, 500_000);
    assert_eq!(deserialized.frame_range, Some((1.0, 120.0)));
}

#[test]
fn test_scene_statistics_default() {
    let stats = SceneStatistics::default();
    assert_eq!(stats.object_count, 0);
    assert_eq!(stats.vertex_count, 0);
    assert_eq!(stats.polygon_count, 0);
}

#[test]
fn test_dcc_capabilities_default() {
    let caps = DccCapabilities::default();
    assert!(caps.script_languages.is_empty());
    assert!(!caps.scene_info);
    assert!(!caps.snapshot);
    assert!(!caps.undo_redo);
}

#[test]
fn test_dcc_capabilities_serialization() {
    let caps = DccCapabilities {
        script_languages: vec![ScriptLanguage::Python, ScriptLanguage::Mel],
        scene_info: true,
        snapshot: true,
        undo_redo: true,
        progress_reporting: false,
        file_operations: true,
        selection: true,
        scene_manager: true,
        transform: true,
        render_capture: true,
        hierarchy: true,
        extensions: HashMap::from([("usd_export".to_string(), true)]),
        ..Default::default()
    };
    let json = serde_json::to_string(&caps).unwrap();
    let deserialized: DccCapabilities = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.script_languages.len(), 2);
    assert!(deserialized.scene_info);
    assert!(deserialized.extensions["usd_export"]);
}

#[test]
fn test_dcc_error_display() {
    let err = DccError {
        code: DccErrorCode::ScriptError,
        message: "NameError: x is not defined".to_string(),
        details: Some("Traceback...".to_string()),
        recoverable: true,
    };
    assert_eq!(
        err.to_string(),
        "[SCRIPT_ERROR] NameError: x is not defined"
    );
}

#[test]
fn test_dcc_error_code_display() {
    assert_eq!(
        DccErrorCode::ConnectionFailed.to_string(),
        "CONNECTION_FAILED"
    );
    assert_eq!(DccErrorCode::Timeout.to_string(), "TIMEOUT");
    assert_eq!(DccErrorCode::ScriptError.to_string(), "SCRIPT_ERROR");
    assert_eq!(DccErrorCode::NotResponding.to_string(), "NOT_RESPONDING");
    assert_eq!(DccErrorCode::Unsupported.to_string(), "UNSUPPORTED");
    assert_eq!(
        DccErrorCode::PermissionDenied.to_string(),
        "PERMISSION_DENIED"
    );
    assert_eq!(DccErrorCode::InvalidInput.to_string(), "INVALID_INPUT");
    assert_eq!(DccErrorCode::SceneError.to_string(), "SCENE_ERROR");
    assert_eq!(DccErrorCode::Internal.to_string(), "INTERNAL");
}

#[test]
fn test_dcc_error_serialization() {
    let err = DccError {
        code: DccErrorCode::ConnectionFailed,
        message: "Connection refused".to_string(),
        details: None,
        recoverable: true,
    };
    let json = serde_json::to_string(&err).unwrap();
    let deserialized: DccError = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.code, DccErrorCode::ConnectionFailed);
    assert!(deserialized.recoverable);
}

// ── Mock adapter test ──

struct MockDccAdapter {
    info: DccInfo,
}

impl MockDccAdapter {
    fn new() -> Self {
        Self {
            info: DccInfo {
                dcc_type: "mock".to_string(),
                version: "1.0.0".to_string(),
                python_version: Some("3.11".to_string()),
                platform: "windows".to_string(),
                pid: 1234,
                metadata: HashMap::new(),
            },
        }
    }
}

impl DccAdapter for MockDccAdapter {
    fn info(&self) -> &DccInfo {
        &self.info
    }

    fn capabilities(&self) -> DccCapabilities {
        DccCapabilities {
            script_languages: vec![ScriptLanguage::Python],
            scene_info: false,
            snapshot: false,
            ..Default::default()
        }
    }

    fn as_connection(&mut self) -> Option<&mut dyn DccConnection> {
        None
    }

    fn as_script_engine(&self) -> Option<&dyn DccScriptEngine> {
        None
    }

    fn as_scene_info(&self) -> Option<&dyn DccSceneInfo> {
        None
    }

    fn as_snapshot(&self) -> Option<&dyn DccSnapshot> {
        None
    }
}

#[test]
fn test_mock_adapter() {
    let adapter = MockDccAdapter::new();
    assert_eq!(adapter.info().dcc_type, "mock");
    assert_eq!(adapter.capabilities().script_languages.len(), 1);
    assert!(!adapter.capabilities().scene_info);
}

#[test]
fn test_mock_adapter_optional_sub_traits() {
    let mut adapter = MockDccAdapter::new();
    assert!(adapter.as_connection().is_none());
    assert!(adapter.as_script_engine().is_none());
    assert!(adapter.as_scene_info().is_none());
    assert!(adapter.as_snapshot().is_none());
    // Cross-DCC protocol traits default to None
    assert!(adapter.as_scene_manager().is_none());
    assert!(adapter.as_transform().is_none());
    assert!(adapter.as_render_capture().is_none());
    assert!(adapter.as_hierarchy().is_none());
}

// ── ObjectTransform tests ──

#[test]
fn test_object_transform_identity() {
    let t = ObjectTransform::identity();
    assert_eq!(t.translate, [0.0, 0.0, 0.0]);
    assert_eq!(t.rotate, [0.0, 0.0, 0.0]);
    assert_eq!(t.scale, [1.0, 1.0, 1.0]);
}

#[test]
fn test_object_transform_default() {
    let t = ObjectTransform::default();
    assert_eq!(t.translate, [0.0, 0.0, 0.0]);
    assert_eq!(t.scale, [0.0, 0.0, 0.0]); // default ≠ identity
}

#[test]
fn test_object_transform_serialization() {
    let t = ObjectTransform {
        translate: [1.0, 2.0, 3.0],
        rotate: [45.0, 0.0, -90.0],
        scale: [1.0, 2.0, 1.0],
    };
    let json = serde_json::to_string(&t).unwrap();
    let back: ObjectTransform = serde_json::from_str(&json).unwrap();
    assert_eq!(back.translate, [1.0, 2.0, 3.0]);
    assert_eq!(back.rotate, [45.0, 0.0, -90.0]);
    assert_eq!(back.scale, [1.0, 2.0, 1.0]);
}

// ── BoundingBox tests ──

#[test]
fn test_bounding_box_center() {
    let bb = BoundingBox {
        min: [0.0, 0.0, 0.0],
        max: [2.0, 4.0, 6.0],
    };
    assert_eq!(bb.center(), [1.0, 2.0, 3.0]);
}

#[test]
fn test_bounding_box_size() {
    let bb = BoundingBox {
        min: [-1.0, -2.0, -3.0],
        max: [1.0, 2.0, 3.0],
    };
    assert_eq!(bb.size(), [2.0, 4.0, 6.0]);
}

#[test]
fn test_bounding_box_serialization() {
    let bb = BoundingBox {
        min: [-10.0, 0.0, -5.0],
        max: [10.0, 20.0, 5.0],
    };
    let json = serde_json::to_string(&bb).unwrap();
    let back: BoundingBox = serde_json::from_str(&json).unwrap();
    assert_eq!(back.min, [-10.0, 0.0, -5.0]);
    assert_eq!(back.max, [10.0, 20.0, 5.0]);
}

// ── SceneObject tests ──

#[test]
fn test_scene_object_serialization() {
    let obj = SceneObject {
        name: "pCube1".to_string(),
        long_name: "|group1|pCube1".to_string(),
        object_type: "mesh".to_string(),
        parent: Some("|group1".to_string()),
        visible: true,
        metadata: HashMap::from([("material".to_string(), "lambert1".to_string())]),
    };
    let json = serde_json::to_string(&obj).unwrap();
    let back: SceneObject = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "pCube1");
    assert_eq!(back.long_name, "|group1|pCube1");
    assert_eq!(back.parent.as_deref(), Some("|group1"));
    assert!(back.visible);
    assert_eq!(back.metadata["material"], "lambert1");
}

#[test]
fn test_scene_object_no_parent() {
    let obj = SceneObject {
        name: "root".to_string(),
        long_name: "|root".to_string(),
        object_type: "transform".to_string(),
        parent: None,
        visible: true,
        metadata: HashMap::new(),
    };
    assert!(obj.parent.is_none());
}

// ── FrameRange tests ──

#[test]
fn test_frame_range_default() {
    let fr = FrameRange::default();
    assert_eq!(fr.start, 0.0);
    assert_eq!(fr.end, 0.0);
    assert_eq!(fr.fps, 0.0);
}

#[test]
fn test_frame_range_serialization() {
    let fr = FrameRange {
        start: 1.0,
        end: 240.0,
        fps: 24.0,
        current: 48.0,
    };
    let json = serde_json::to_string(&fr).unwrap();
    let back: FrameRange = serde_json::from_str(&json).unwrap();
    assert_eq!(back.start, 1.0);
    assert_eq!(back.end, 240.0);
    assert_eq!(back.fps, 24.0);
    assert_eq!(back.current, 48.0);
}
