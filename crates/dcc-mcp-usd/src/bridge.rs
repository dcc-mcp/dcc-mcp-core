//! Bridge between the `dcc-mcp-protocols` types and the USD data model.
//!
//! This module provides conversion functions that turn DCC-native scene
//! representations (`SceneInfo`, `SceneStatistics`) into their USD equivalents
//! (`UsdStage`, `UsdLayer`), enabling cross-DCC scene exchange via the USD
//! interchange format.
//!
//! # Usage
//!
//! ```rust
//! use dcc_mcp_protocols::adapters::{SceneInfo, SceneStatistics};
//! use dcc_mcp_usd::bridge::scene_info_to_stage;
//!
//! let info = SceneInfo {
//!     file_path: "/projects/shot_010.ma".to_string(),
//!     name: "shot_010".to_string(),
//!     modified: false,
//!     format: ".ma".to_string(),
//!     frame_range: Some((1.0, 120.0)),
//!     fps: Some(24.0),
//!     up_axis: Some("y".to_string()),
//!     units: Some("cm".to_string()),
//!     ..Default::default()
//! };
//! let stage = scene_info_to_stage(&info, "maya");
//! assert!(stage.has_prim("/World"));
//! ```

use dcc_mcp_protocols::adapters::SceneInfo;

use crate::stage::UsdStage;
use crate::types::{SdfPath, UsdLayer, UsdPrim, VtValue};

/// Convert a DCC `SceneInfo` into a `UsdStage`.
///
/// The conversion creates a minimal but well-formed USD stage that captures
/// the DCC-reported scene metadata:
///
/// - Root layer `upAxis` / `metersPerUnit` / frame range are set from `info`.
/// - A `/World` Xform prim is added as the default scene root.
/// - Statistics (object count, vertex count, etc.) are stored as custom
///   metadata on the `/World` prim.
/// - The source DCC type is recorded in stage metadata.
///
/// # Arguments
/// * `info`     — The DCC scene information to convert.
/// * `dcc_type` — A string identifying the source DCC (e.g. `"maya"`).
pub fn scene_info_to_stage(info: &SceneInfo, dcc_type: &str) -> UsdStage {
    let mut layer = UsdLayer::new(if info.file_path.is_empty() {
        format!("anon:dcc-{dcc_type}")
    } else {
        info.file_path.clone()
    });

    layer.display_name = info.name.clone();

    // Up axis: USD uses "Y" or "Z" (uppercase)
    layer.up_axis = match info.up_axis.as_deref().unwrap_or("y") {
        "z" | "Z" => "Z".to_string(),
        _ => "Y".to_string(),
    };

    // Meters per unit from DCC units string
    layer.meters_per_unit = units_to_meters_per_unit(info.units.as_deref().unwrap_or("cm"));

    // Frame range
    if let Some((start, end)) = info.frame_range {
        layer.start_time_code = Some(start);
        layer.end_time_code = Some(end);
    }
    if let Some(fps) = info.fps {
        layer.frames_per_second = Some(fps);
    }
    if let Some(frame) = info.current_frame {
        layer.default_time_code = Some(frame);
    }

    // Create the /World root prim
    let world_path = SdfPath::new("/World").expect("valid path");
    let mut world = UsdPrim::new(world_path, "Xform");

    // Store scene statistics as custom metadata
    let stats = &info.statistics;
    world
        .metadata
        .insert("dcc:type".to_string(), dcc_type.to_string());
    world.metadata.insert(
        "dcc:objectCount".to_string(),
        stats.object_count.to_string(),
    );
    world.metadata.insert(
        "dcc:vertexCount".to_string(),
        stats.vertex_count.to_string(),
    );
    world.metadata.insert(
        "dcc:polygonCount".to_string(),
        stats.polygon_count.to_string(),
    );
    world.metadata.insert(
        "dcc:materialCount".to_string(),
        stats.material_count.to_string(),
    );
    world
        .metadata
        .insert("dcc:lightCount".to_string(), stats.light_count.to_string());
    world.metadata.insert(
        "dcc:cameraCount".to_string(),
        stats.camera_count.to_string(),
    );

    // Record scene file path and format as custom attributes
    if !info.file_path.is_empty() {
        world.add_attribute(crate::types::UsdAttribute::custom(
            "dcc:sourceFile",
            VtValue::Asset(info.file_path.clone()),
        ));
    }
    if !info.format.is_empty() {
        world.add_attribute(crate::types::UsdAttribute::custom(
            "dcc:sourceFormat",
            VtValue::Token(info.format.clone()),
        ));
    }

    layer.define_prim(world);

    let mut stage = UsdStage::from_layer(layer);
    stage.default_prim = Some("World".to_string());
    stage
        .metadata
        .insert("dcc:type".to_string(), dcc_type.to_string());
    stage
        .metadata
        .insert("dcc:sceneName".to_string(), info.name.clone());

    stage
}

/// Convert a `UsdStage` back into a `SceneInfo` (best-effort).
///
/// Only the information that can be round-tripped through USD is recovered.
/// In particular, polygon-level statistics come from the `/World` prim's
/// custom metadata if present.
pub fn stage_to_scene_info(stage: &UsdStage) -> SceneInfo {
    let layer = &stage.root_layer;

    let mut info = SceneInfo {
        file_path: if layer.identifier.starts_with("anon:") {
            String::new()
        } else {
            layer.identifier.clone()
        },
        // Use stage name first (more reliable), fall back to layer display_name
        name: if !stage.name.is_empty() {
            stage.name.clone()
        } else {
            layer.display_name.clone()
        },
        modified: false,
        format: String::new(),
        frame_range: match (layer.start_time_code, layer.end_time_code) {
            (Some(s), Some(e)) => Some((s, e)),
            _ => None,
        },
        current_frame: layer.default_time_code,
        fps: layer.frames_per_second,
        up_axis: Some(layer.up_axis.clone()),
        units: Some(meters_per_unit_to_units(layer.meters_per_unit)),
        ..Default::default()
    };

    // Recover statistics from /World prim metadata
    if let Some(world) = stage.get_prim("/World") {
        let get_u64 = |key: &str| {
            world
                .metadata
                .get(key)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
        };
        info.statistics.object_count = get_u64("dcc:objectCount");
        info.statistics.vertex_count = get_u64("dcc:vertexCount");
        info.statistics.polygon_count = get_u64("dcc:polygonCount");
        info.statistics.material_count = get_u64("dcc:materialCount");
        info.statistics.light_count = get_u64("dcc:lightCount");
        info.statistics.camera_count = get_u64("dcc:cameraCount");

        // Source format
        if let Some(attr) = world.get_attribute("dcc:sourceFormat") {
            if let Some(s) = attr.default_value.as_ref().and_then(|v| v.as_str()) {
                info.format = s.to_string();
            }
        }
    }

    info
}

// ── Unit conversion helpers ──────────────────────────────────────────────────

/// Convert a DCC unit string to USD `metersPerUnit` value.
pub fn units_to_meters_per_unit(units: &str) -> f64 {
    match units.to_lowercase().as_str() {
        "m" | "meter" | "meters" => 1.0,
        "cm" | "centimeter" | "centimeters" => 0.01,
        "mm" | "millimeter" | "millimeters" => 0.001,
        "km" | "kilometer" | "kilometers" => 1_000.0,
        "inch" | "inches" | "in" => 0.0254,
        "foot" | "feet" | "ft" => 0.3048,
        "yard" | "yards" | "yd" => 0.9144,
        _ => 0.01, // default to cm (Maya default)
    }
}

/// Convert a USD `metersPerUnit` value back to a human-readable unit string.
pub fn meters_per_unit_to_units(mpu: f64) -> String {
    // Tolerance-based comparison
    if (mpu - 1.0).abs() < 1e-9 {
        "m".to_string()
    } else if (mpu - 0.01).abs() < 1e-9 {
        "cm".to_string()
    } else if (mpu - 0.001).abs() < 1e-9 {
        "mm".to_string()
    } else if (mpu - 1000.0).abs() < 1e-6 {
        "km".to_string()
    } else if (mpu - 0.0254).abs() < 1e-6 {
        "inch".to_string()
    } else if (mpu - 0.3048).abs() < 1e-6 {
        "ft".to_string()
    } else {
        format!("{}m", mpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_protocols::adapters::SceneStatistics;

    mod test_unit_conversion {
        use super::*;

        #[test]
        fn test_cm_to_mpu() {
            assert!((units_to_meters_per_unit("cm") - 0.01).abs() < 1e-9);
        }

        #[test]
        fn test_m_to_mpu() {
            assert!((units_to_meters_per_unit("m") - 1.0).abs() < 1e-9);
        }

        #[test]
        fn test_inch_to_mpu() {
            assert!((units_to_meters_per_unit("inch") - 0.0254).abs() < 1e-6);
        }

        #[test]
        fn test_unknown_defaults_to_cm() {
            assert!((units_to_meters_per_unit("furlongs") - 0.01).abs() < 1e-9);
        }

        #[test]
        fn test_mpu_roundtrip_cm() {
            let mpu = units_to_meters_per_unit("cm");
            assert_eq!(meters_per_unit_to_units(mpu), "cm");
        }

        #[test]
        fn test_mpu_roundtrip_m() {
            let mpu = units_to_meters_per_unit("m");
            assert_eq!(meters_per_unit_to_units(mpu), "m");
        }

        #[test]
        fn test_mpu_roundtrip_mm() {
            let mpu = units_to_meters_per_unit("mm");
            assert_eq!(meters_per_unit_to_units(mpu), "mm");
        }

        #[test]
        fn test_mpu_roundtrip_inch() {
            let mpu = units_to_meters_per_unit("inch");
            assert_eq!(meters_per_unit_to_units(mpu), "inch");
        }
    }

    mod test_scene_info_to_stage {
        use super::*;

        fn make_scene_info() -> SceneInfo {
            SceneInfo {
                file_path: "/projects/shot_010.ma".to_string(),
                name: "shot_010".to_string(),
                modified: false,
                format: ".ma".to_string(),
                frame_range: Some((1.0, 120.0)),
                current_frame: Some(24.0),
                fps: Some(24.0),
                up_axis: Some("y".to_string()),
                units: Some("cm".to_string()),
                statistics: SceneStatistics {
                    object_count: 50,
                    vertex_count: 100_000,
                    polygon_count: 50_000,
                    material_count: 10,
                    texture_count: 20,
                    light_count: 3,
                    camera_count: 2,
                },
                metadata: Default::default(),
            }
        }

        #[test]
        fn test_basic_conversion() {
            let info = make_scene_info();
            let stage = scene_info_to_stage(&info, "maya");
            assert!(stage.has_prim("/World"));
            assert_eq!(stage.default_prim.as_deref(), Some("World"));
        }

        #[test]
        fn test_frame_range_preserved() {
            let info = make_scene_info();
            let stage = scene_info_to_stage(&info, "maya");
            assert_eq!(stage.root_layer.start_time_code, Some(1.0));
            assert_eq!(stage.root_layer.end_time_code, Some(120.0));
            assert_eq!(stage.root_layer.frames_per_second, Some(24.0));
        }

        #[test]
        fn test_up_axis_y() {
            let info = make_scene_info();
            let stage = scene_info_to_stage(&info, "maya");
            assert_eq!(stage.root_layer.up_axis, "Y");
        }

        #[test]
        fn test_up_axis_z() {
            let mut info = make_scene_info();
            info.up_axis = Some("z".to_string());
            let stage = scene_info_to_stage(&info, "houdini");
            assert_eq!(stage.root_layer.up_axis, "Z");
        }

        #[test]
        fn test_statistics_in_metadata() {
            let info = make_scene_info();
            let stage = scene_info_to_stage(&info, "maya");
            let world = stage.get_prim("/World").unwrap();
            assert_eq!(world.metadata["dcc:objectCount"], "50");
            assert_eq!(world.metadata["dcc:vertexCount"], "100000");
            assert_eq!(world.metadata["dcc:lightCount"], "3");
        }

        #[test]
        fn test_dcc_type_in_metadata() {
            let info = make_scene_info();
            let stage = scene_info_to_stage(&info, "blender");
            assert_eq!(stage.metadata["dcc:type"], "blender");
        }

        #[test]
        fn test_empty_file_path_uses_anon() {
            let mut info = make_scene_info();
            info.file_path = String::new();
            let stage = scene_info_to_stage(&info, "maya");
            assert!(stage.root_layer.identifier.starts_with("anon:"));
        }

        #[test]
        fn test_meters_per_unit_cm() {
            let info = make_scene_info(); // units = "cm"
            let stage = scene_info_to_stage(&info, "maya");
            assert!((stage.root_layer.meters_per_unit - 0.01).abs() < 1e-9);
        }
    }

    mod test_stage_to_scene_info {
        use super::*;

        #[test]
        fn test_roundtrip() {
            let original = SceneInfo {
                file_path: "/shot.ma".to_string(),
                name: "shot".to_string(),
                frame_range: Some((1.0, 48.0)),
                fps: Some(24.0),
                up_axis: Some("y".to_string()),
                units: Some("cm".to_string()),
                statistics: SceneStatistics {
                    object_count: 10,
                    vertex_count: 5_000,
                    polygon_count: 2_500,
                    material_count: 3,
                    texture_count: 6,
                    light_count: 1,
                    camera_count: 1,
                },
                ..Default::default()
            };
            let stage = scene_info_to_stage(&original, "maya");
            let recovered = stage_to_scene_info(&stage);

            assert_eq!(recovered.file_path, original.file_path);
            assert_eq!(recovered.frame_range, original.frame_range);
            assert_eq!(recovered.fps, original.fps);
            assert_eq!(recovered.up_axis.as_deref(), Some("Y")); // USD normalizes
            assert_eq!(recovered.statistics.object_count, 10);
            assert_eq!(recovered.statistics.vertex_count, 5_000);
            assert_eq!(recovered.statistics.polygon_count, 2_500);
        }

        #[test]
        fn test_anon_layer_gives_empty_path() {
            let stage = UsdStage::new("untitled");
            let info = stage_to_scene_info(&stage);
            assert!(info.file_path.is_empty());
        }
    }
}
