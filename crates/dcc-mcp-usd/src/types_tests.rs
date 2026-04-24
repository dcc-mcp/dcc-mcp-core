//! Unit tests for the `types` module.
#![cfg(test)]

use super::*;

mod test_sdf_path {
    use super::*;

    #[test]
    fn test_new_valid() {
        let p = SdfPath::new("/World/Cube").unwrap();
        assert_eq!(p.as_str(), "/World/Cube");
    }

    #[test]
    fn test_new_empty_fails() {
        assert!(SdfPath::new("").is_err());
    }

    #[test]
    fn test_root() {
        let r = SdfPath::root();
        assert_eq!(r.as_str(), "/");
        assert!(r.is_absolute());
    }

    #[test]
    fn test_parent() {
        let p = SdfPath::new("/World/Cube").unwrap();
        let parent = p.parent().unwrap();
        assert_eq!(parent.as_str(), "/World");
    }

    #[test]
    fn test_parent_of_root_is_none() {
        let r = SdfPath::root();
        assert!(r.parent().is_none());
    }

    #[test]
    fn test_parent_of_top_level() {
        let p = SdfPath::new("/World").unwrap();
        let parent = p.parent().unwrap();
        assert_eq!(parent.as_str(), "/");
    }

    #[test]
    fn test_child() {
        let p = SdfPath::new("/World").unwrap();
        let child = p.child("Cube").unwrap();
        assert_eq!(child.as_str(), "/World/Cube");
    }

    #[test]
    fn test_child_empty_name_fails() {
        let p = SdfPath::new("/World").unwrap();
        assert!(p.child("").is_err());
    }

    #[test]
    fn test_name() {
        let p = SdfPath::new("/World/Cube").unwrap();
        assert_eq!(p.name(), "Cube");
    }

    #[test]
    fn test_name_root_empty() {
        let r = SdfPath::root();
        assert_eq!(r.name(), "");
    }

    #[test]
    fn test_is_absolute_relative() {
        let p = SdfPath::new("World/Cube").unwrap();
        assert!(!p.is_absolute());
    }

    #[test]
    fn test_display() {
        let p = SdfPath::new("/World/Cube").unwrap();
        assert_eq!(format!("{p}"), "/World/Cube");
    }

    #[test]
    fn test_serialization_roundtrip() {
        let p = SdfPath::new("/World/Mesh_001").unwrap();
        let json = serde_json::to_string(&p).unwrap();
        let back: SdfPath = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}

mod test_vt_value {
    use super::*;

    #[test]
    fn test_type_names() {
        assert_eq!(VtValue::Bool(true).type_name(), "bool");
        assert_eq!(VtValue::Int(1).type_name(), "int");
        assert_eq!(VtValue::Float(1.0).type_name(), "float");
        assert_eq!(VtValue::Double(1.0).type_name(), "double");
        assert_eq!(VtValue::String("x".into()).type_name(), "string");
        assert_eq!(VtValue::Token("mesh".into()).type_name(), "token");
        assert_eq!(VtValue::Asset("/path".into()).type_name(), "asset");
        assert_eq!(VtValue::Vec3f(1.0, 2.0, 3.0).type_name(), "float3");
        assert_eq!(VtValue::Matrix4d([0.0; 16]).type_name(), "matrix4d");
        assert_eq!(VtValue::FloatArray(vec![]).type_name(), "float[]");
    }

    #[test]
    fn test_as_float() {
        assert_eq!(VtValue::Float(1.5).as_float(), Some(1.5));
        assert!(VtValue::String("x".into()).as_float().is_none());
    }

    #[test]
    fn test_as_str() {
        assert_eq!(
            VtValue::Token("xformOp:translate".into()).as_str(),
            Some("xformOp:translate")
        );
        assert_eq!(
            VtValue::Asset("/textures/diffuse.png".into()).as_str(),
            Some("/textures/diffuse.png")
        );
        assert!(VtValue::Int(1).as_str().is_none());
    }

    #[test]
    fn test_vec3f_serialization() {
        let v = VtValue::Vec3f(1.0, 2.0, 3.0);
        let json = serde_json::to_string(&v).unwrap();
        let back: VtValue = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

mod test_usd_attribute {
    use super::*;

    #[test]
    fn test_new_attribute() {
        let attr = UsdAttribute::new("xformOp:translate", VtValue::Vec3f(1.0, 2.0, 3.0));
        assert_eq!(attr.name, "xformOp:translate");
        assert!(!attr.custom);
        assert!(attr.default_value.is_some());
    }

    #[test]
    fn test_custom_attribute() {
        let attr = UsdAttribute::custom("myCustom:data", VtValue::Int(42));
        assert!(attr.custom);
    }

    #[test]
    fn test_get_at_default() {
        let attr = UsdAttribute::new("radius", VtValue::Float(0.5));
        let val = attr.get_at(0.0).unwrap();
        assert_eq!(val.as_float(), Some(0.5));
    }

    #[test]
    fn test_time_sampled() {
        let mut attr = UsdAttribute::new("xformOp:translate", VtValue::Vec3f(0.0, 0.0, 0.0));
        // Use the same key format that get_at() will generate: format!("{}", 24.0_f64)
        let key = format!("{}", 24.0_f64);
        attr.time_samples.insert(key, VtValue::Vec3f(1.0, 0.0, 0.0));
        let val = attr.get_at(24.0).unwrap();
        assert!(matches!(val, VtValue::Vec3f(x, _, _) if (*x - 1.0).abs() < 1e-6));
    }
}

mod test_usd_prim {
    use super::*;

    #[test]
    fn test_new_prim() {
        let path = SdfPath::new("/World/Cube").unwrap();
        let prim = UsdPrim::new(path.clone(), "Mesh");
        assert_eq!(prim.type_name, "Mesh");
        assert_eq!(prim.name(), "Cube");
        assert!(prim.active);
    }

    #[test]
    fn test_add_get_attribute() {
        let mut prim = UsdPrim::new(SdfPath::new("/Sphere").unwrap(), "Sphere");
        prim.add_attribute(UsdAttribute::new("radius", VtValue::Float(1.0)));
        let attr = prim.get_attribute("radius").unwrap();
        assert_eq!(attr.name, "radius");
    }

    #[test]
    fn test_has_api() {
        let mut prim = UsdPrim::new(SdfPath::new("/Model").unwrap(), "Xform");
        prim.applied_schemas.push("GeomModelAPI".to_string());
        assert!(prim.has_api("GeomModelAPI"));
        assert!(!prim.has_api("MaterialBindingAPI"));
    }

    #[test]
    fn test_root_prim() {
        let root = UsdPrim::root();
        assert_eq!(root.path.as_str(), "/");
        assert_eq!(root.type_name, "");
    }
}

mod test_usd_layer {
    use super::*;

    #[test]
    fn test_new_layer() {
        let layer = UsdLayer::new("anon:0x1234");
        assert_eq!(layer.identifier, "anon:0x1234");
        assert_eq!(layer.up_axis, "Y");
        assert!((layer.meters_per_unit - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_define_get_prim() {
        let mut layer = UsdLayer::new("test.usda");
        let prim = UsdPrim::new(SdfPath::new("/World").unwrap(), "Xform");
        layer.define_prim(prim);
        assert!(layer.get_prim("/World").is_some());
        assert!(layer.get_prim("/NonExistent").is_none());
    }

    #[test]
    fn test_all_prims() {
        let mut layer = UsdLayer::new("test.usda");
        layer.define_prim(UsdPrim::new(SdfPath::new("/A").unwrap(), "Xform"));
        layer.define_prim(UsdPrim::new(SdfPath::new("/A/B").unwrap(), "Mesh"));
        assert_eq!(layer.all_prims().count(), 2);
    }

    #[test]
    fn test_layer_serialization() {
        let mut layer = UsdLayer::new("shot_010.usda");
        layer.start_time_code = Some(1.0);
        layer.end_time_code = Some(120.0);
        layer.frames_per_second = Some(24.0);
        layer.define_prim(UsdPrim::new(SdfPath::new("/World").unwrap(), "Xform"));
        let json = serde_json::to_string(&layer).unwrap();
        let back: UsdLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(back.identifier, "shot_010.usda");
        assert_eq!(back.frames_per_second, Some(24.0));
        assert!(back.get_prim("/World").is_some());
    }

    #[test]
    fn test_get_prim_mut() {
        let mut layer = UsdLayer::new("test.usda");
        layer.define_prim(UsdPrim::new(SdfPath::new("/Root").unwrap(), "Xform"));
        {
            let prim = layer.get_prim_mut("/Root").unwrap();
            prim.kind = "assembly".to_string();
        }
        let prim = layer.get_prim("/Root").unwrap();
        assert_eq!(prim.kind, "assembly");
    }

    #[test]
    fn test_get_prim_mut_nonexistent() {
        let mut layer = UsdLayer::new("test.usda");
        assert!(layer.get_prim_mut("/NonExistent").is_none());
    }
}

mod test_vt_value_additional {
    use super::*;

    #[test]
    fn test_type_names_all_variants() {
        assert_eq!(VtValue::Int64(100).type_name(), "int64");
        assert_eq!(VtValue::Vec2f(1.0, 2.0).type_name(), "float2");
        assert_eq!(VtValue::Vec4f(1.0, 2.0, 3.0, 4.0).type_name(), "float4");
        assert_eq!(VtValue::IntArray(vec![1, 2, 3]).type_name(), "int[]");
        assert_eq!(
            VtValue::Vec3fArray(vec![[1.0, 2.0, 3.0]]).type_name(),
            "float3[]"
        );
        assert_eq!(
            VtValue::StringArray(vec!["a".to_string()]).type_name(),
            "string[]"
        );
    }

    #[test]
    fn test_as_float_promotes_double() {
        let v = VtValue::Double(2.5_f64);
        let f = v.as_float().unwrap();
        assert!((f - 2.5_f32).abs() < 1e-4);
    }

    #[test]
    fn test_as_float_promotes_int() {
        let v = VtValue::Int(7);
        let f = v.as_float().unwrap();
        assert!((f - 7.0_f32).abs() < 1e-6);
    }

    #[test]
    fn test_as_float_none_for_non_numeric() {
        assert!(VtValue::Bool(true).as_float().is_none());
        assert!(VtValue::Token("foo".into()).as_float().is_none());
    }

    #[test]
    fn test_as_str_none_for_non_string() {
        assert!(VtValue::Float(1.0).as_str().is_none());
        assert!(VtValue::Bool(false).as_str().is_none());
    }

    #[test]
    fn test_matrix4d_serialization() {
        let m = VtValue::Matrix4d([1.0; 16]);
        let json = serde_json::to_string(&m).unwrap();
        let back: VtValue = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, VtValue::Matrix4d(_)));
    }
}

mod test_sdf_path_additional {
    use super::*;

    #[test]
    fn test_child_on_root_slash() {
        // Root path ends with '/' — child should not double-slash
        let root = SdfPath::root();
        let child = root.child("World").unwrap();
        assert_eq!(child.as_str(), "/World");
    }

    #[test]
    fn test_parent_of_deep_path() {
        let p = SdfPath::new("/A/B/C/D").unwrap();
        let parent = p.parent().unwrap();
        assert_eq!(parent.as_str(), "/A/B/C");
    }
}

mod test_usd_stage_metrics {
    use super::*;

    #[test]
    fn test_default_is_all_zero() {
        let m = UsdStageMetrics::default();
        assert_eq!(m.prim_count, 0);
        assert_eq!(m.mesh_count, 0);
        assert_eq!(m.camera_count, 0);
        assert_eq!(m.light_count, 0);
        assert_eq!(m.material_count, 0);
        assert_eq!(m.xform_count, 0);
    }

    #[test]
    fn test_populated_metrics() {
        let m = UsdStageMetrics {
            prim_count: 100,
            mesh_count: 50,
            camera_count: 2,
            light_count: 5,
            material_count: 20,
            xform_count: 23,
        };
        assert_eq!(m.prim_count, 100);
        assert_eq!(
            m.mesh_count + m.camera_count + m.light_count + m.material_count + m.xform_count,
            100
        );
    }

    #[test]
    fn test_serialization_roundtrip() {
        let m = UsdStageMetrics {
            prim_count: 42,
            mesh_count: 10,
            camera_count: 1,
            light_count: 3,
            material_count: 8,
            xform_count: 20,
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: UsdStageMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(back.prim_count, 42);
        assert_eq!(back.mesh_count, 10);
        assert_eq!(back.camera_count, 1);
    }
}
