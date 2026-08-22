use dcc_mcp_protocols::adapters::{SceneInfo, SceneStatistics};
use dcc_mcp_usd::bridge::{scene_info_to_stage, stage_to_scene_info};
use dcc_mcp_usd::{SdfPath, UsdStage, VtValue};

fn scene(dcc: &str) -> SceneInfo {
    SceneInfo {
        file_path: format!("/show/shot010/scene.{dcc}"),
        name: format!("shot010-{dcc}"),
        format: format!(".{dcc}"),
        frame_range: Some((1001.0, 1050.0)),
        current_frame: Some(1012.0),
        fps: Some(24.0),
        up_axis: Some(if dcc == "blender" { "z" } else { "y" }.to_string()),
        units: Some(if dcc == "blender" { "m" } else { "cm" }.to_string()),
        statistics: SceneStatistics {
            object_count: 12,
            vertex_count: 4_096,
            polygon_count: 2_048,
            material_count: 3,
            light_count: 2,
            camera_count: 1,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn maya_and_blender_scene_contracts_round_trip_natively() {
    for dcc in ["maya", "blender"] {
        let source = scene(dcc);
        let stage = scene_info_to_stage(&source, dcc);
        let wire = stage.to_json().unwrap();
        let restored = UsdStage::from_json(&wire).unwrap();
        let recovered = stage_to_scene_info(&restored);

        assert_eq!(restored.metadata["dcc:type"], dcc);
        assert_eq!(recovered.file_path, source.file_path);
        assert_eq!(recovered.frame_range, source.frame_range);
        assert_eq!(recovered.statistics.vertex_count, 4_096);
        assert!(restored.export_usda().contains("#usda 1.0"));
    }
}

#[test]
fn stage_edit_and_json_transport_preserve_attributes() {
    let mut stage = UsdStage::new("custom-host-scene");
    stage.define_prim(SdfPath::new("/World").unwrap(), "Xform");
    stage.define_prim(SdfPath::new("/World/Hero").unwrap(), "Mesh");
    stage
        .set_attribute("/World/Hero", "displayColor", VtValue::Vec3f(0.2, 0.4, 0.8))
        .unwrap();

    let restored = UsdStage::from_json(&stage.to_json().unwrap()).unwrap();
    assert_eq!(
        restored
            .get_attribute("/World/Hero", "displayColor")
            .unwrap(),
        Some(&VtValue::Vec3f(0.2, 0.4, 0.8))
    );
}
