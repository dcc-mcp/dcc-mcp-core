//! `UsdStage` — the composed view of a USD scene.
//!
//! A stage is the primary entry point for working with USD.  It holds one
//! or more `UsdLayer`s and provides a unified prim hierarchy.  For
//! dcc-mcp-core purposes the stage is the transport unit for cross-DCC
//! scene exchange.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

use crate::error::{UsdError, UsdResult};
use crate::types::{SdfPath, UsdAttribute, UsdLayer, UsdPrim, UsdStageMetrics, VtValue};

// ── UsdStage ──────────────────────────────────────────────────────────────────

/// A composed USD stage.
///
/// The stage owns a root layer plus any number of sub-layers.  Prims are
/// looked up with a simple "strongest-layer wins" opinion resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsdStage {
    /// Unique stage identifier (UUID).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Root (edit) layer.
    pub root_layer: UsdLayer,
    /// Additional layers in strongest-to-weakest order (excludes root).
    #[serde(default)]
    pub sublayers: Vec<UsdLayer>,
    /// Default prim path hint (e.g. `/World`).
    pub default_prim: Option<String>,
    /// Stage-level metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl UsdStage {
    /// Create a new empty stage with the given name.
    ///
    /// A fresh root layer is created with a generated anonymous identifier.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let layer_id = format!("anon:{}", Uuid::new_v4());
        debug!(stage_name = %name, layer_id = %layer_id, "creating new UsdStage");
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            root_layer: UsdLayer::new(layer_id),
            sublayers: Vec::new(),
            default_prim: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a stage from an existing root layer.
    pub fn from_layer(layer: UsdLayer) -> Self {
        let name = if layer.display_name.is_empty() {
            layer.identifier.clone()
        } else {
            layer.display_name.clone()
        };
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            root_layer: layer,
            sublayers: Vec::new(),
            default_prim: None,
            metadata: HashMap::new(),
        }
    }

    // ── Prim operations ──

    /// Define a prim at the given path in the root layer.
    ///
    /// Returns a mutable reference to the prim.
    pub fn define_prim(&mut self, path: SdfPath, type_name: impl Into<String>) -> &mut UsdPrim {
        let prim = UsdPrim::new(path.clone(), type_name);
        self.root_layer.define_prim(prim);
        self.root_layer
            .prims
            .get_mut(path.as_str())
            .expect("just inserted")
    }

    /// Get a prim by path, searching layers from strongest to weakest.
    pub fn get_prim(&self, path: &str) -> Option<&UsdPrim> {
        // Root layer has highest opinion strength
        if let Some(p) = self.root_layer.get_prim(path) {
            return Some(p);
        }
        for layer in &self.sublayers {
            if let Some(p) = layer.get_prim(path) {
                return Some(p);
            }
        }
        None
    }

    /// Get a mutable prim (from the root layer only — edit target).
    pub fn get_prim_mut(&mut self, path: &str) -> Option<&mut UsdPrim> {
        self.root_layer.get_prim_mut(path)
    }

    /// Whether the stage has a prim at the given path.
    pub fn has_prim(&self, path: &str) -> bool {
        self.get_prim(path).is_some()
    }

    /// Remove a prim from the root layer.  Returns `true` if the prim existed.
    pub fn remove_prim(&mut self, path: &str) -> bool {
        self.root_layer.prims.remove(path).is_some()
    }

    /// Traverse all prims in all layers, yielding each unique path once.
    ///
    /// Root-layer prims have priority; sub-layer prims only appear if no
    /// root-layer opinion exists for that path.
    pub fn traverse(&self) -> Vec<&UsdPrim> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for prim in self.root_layer.all_prims() {
            seen.insert(prim.path.as_str().to_string());
            result.push(prim);
        }
        for layer in &self.sublayers {
            for prim in layer.all_prims() {
                if seen.insert(prim.path.as_str().to_string()) {
                    result.push(prim);
                }
            }
        }
        result
    }

    /// Return all prims whose `type_name` matches the given string.
    pub fn prims_of_type(&self, type_name: &str) -> Vec<&UsdPrim> {
        self.traverse()
            .into_iter()
            .filter(|p| p.type_name == type_name)
            .collect()
    }

    // ── Stage-level attribute helpers ──

    /// Set an attribute on a prim (defines it if not present).
    ///
    /// Returns `Err(PrimNotFound)` if the prim does not exist in the root layer.
    pub fn set_attribute(
        &mut self,
        prim_path: &str,
        attr_name: impl Into<String>,
        value: VtValue,
    ) -> UsdResult<()> {
        let prim = self
            .root_layer
            .prims
            .get_mut(prim_path)
            .ok_or_else(|| UsdError::PrimNotFound(prim_path.to_string()))?;
        let name = attr_name.into();
        prim.attributes
            .entry(name.clone())
            .and_modify(|a| a.default_value = Some(value.clone()))
            .or_insert_with(|| UsdAttribute::new(name, value));
        Ok(())
    }

    /// Get an attribute value from a prim.
    pub fn get_attribute(&self, prim_path: &str, attr_name: &str) -> UsdResult<Option<&VtValue>> {
        let prim = self
            .get_prim(prim_path)
            .ok_or_else(|| UsdError::PrimNotFound(prim_path.to_string()))?;
        Ok(prim
            .get_attribute(attr_name)
            .and_then(|a| a.default_value.as_ref()))
    }

    // ── Metrics ──

    /// Compute stage-level statistics.
    pub fn metrics(&self) -> UsdStageMetrics {
        let mut m = UsdStageMetrics::default();
        for prim in self.traverse() {
            m.prim_count += 1;
            match prim.type_name.as_str() {
                "Mesh" => m.mesh_count += 1,
                "Camera" => m.camera_count += 1,
                "SphereLight" | "DiskLight" | "DistantLight" | "RectLight" | "DomeLight"
                | "PortalLight" => m.light_count += 1,
                "Material" => m.material_count += 1,
                "Xform" => m.xform_count += 1,
                _ => {}
            }
        }
        m
    }

    // ── Serialization helpers ──

    /// Serialize the stage to a compact JSON string.
    pub fn to_json(&self) -> UsdResult<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// Deserialize a stage from a JSON string.
    pub fn from_json(json: &str) -> UsdResult<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// Export the root layer as minimal USDA (USD ASCII) text.
    ///
    /// The output is a human-readable `.usda` file that can be loaded by
    /// any tool that understands OpenUSD.
    pub fn export_usda(&self) -> String {
        let layer = &self.root_layer;
        let mut out = String::new();

        // Header
        out.push_str("#usda 1.0\n(\n");
        if !layer.comment.is_empty() {
            out.push_str(&format!("    doc = \"{}\"\n", layer.comment));
        }
        out.push_str(&format!("    upAxis = \"{}\"\n", layer.up_axis));
        out.push_str(&format!("    metersPerUnit = {}\n", layer.meters_per_unit));
        if let Some(fps) = layer.frames_per_second {
            out.push_str(&format!("    framesPerSecond = {fps}\n"));
        }
        if let Some(start) = layer.start_time_code {
            out.push_str(&format!("    startTimeCode = {start}\n"));
        }
        if let Some(end) = layer.end_time_code {
            out.push_str(&format!("    endTimeCode = {end}\n"));
        }
        if let Some(ref dp) = self.default_prim {
            out.push_str(&format!("    defaultPrim = \"{dp}\"\n"));
        }
        out.push_str(")\n\n");

        // Prims (sorted for deterministic output)
        let mut paths: Vec<&String> = layer.prims.keys().collect();
        paths.sort();
        for path in paths {
            let prim = &layer.prims[path];
            let active_str = if prim.active { "" } else { " (active = false)" };
            let type_str = if prim.type_name.is_empty() {
                String::new()
            } else {
                format!(" \"{}\"", prim.type_name)
            };
            out.push_str(&format!(
                "def{type_str} \"{}\"{active_str} {{\n",
                prim.name()
            ));

            // Attributes
            let mut attr_names: Vec<&String> = prim.attributes.keys().collect();
            attr_names.sort();
            for name in attr_names {
                let attr = &prim.attributes[name];
                if let Some(val) = &attr.default_value {
                    let usda_val = vt_to_usda(val);
                    let custom_kw = if attr.custom { "custom " } else { "" };
                    out.push_str(&format!(
                        "    {}{} {} = {}\n",
                        custom_kw,
                        val.type_name(),
                        name,
                        usda_val
                    ));
                }
            }
            out.push_str("}\n\n");
        }

        out
    }
}

/// Format a `VtValue` as a USDA literal.
fn vt_to_usda(val: &VtValue) -> String {
    match val {
        VtValue::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        VtValue::Int(i) => i.to_string(),
        VtValue::Int64(i) => i.to_string(),
        VtValue::Float(f) => format!("{f}"),
        VtValue::Double(d) => format!("{d}"),
        VtValue::String(s) => format!("\"{s}\""),
        VtValue::Token(t) => format!("\"{t}\""),
        VtValue::Asset(a) => format!("@{a}@"),
        VtValue::Vec2f(x, y) => format!("({x}, {y})"),
        VtValue::Vec3f(x, y, z) => format!("({x}, {y}, {z})"),
        VtValue::Vec4f(x, y, z, w) => format!("({x}, {y}, {z}, {w})"),
        VtValue::Matrix4d(m) => format!(
            "(({}, {}, {}, {}), ({}, {}, {}, {}), ({}, {}, {}, {}), ({}, {}, {}, {}))",
            m[0],
            m[1],
            m[2],
            m[3],
            m[4],
            m[5],
            m[6],
            m[7],
            m[8],
            m[9],
            m[10],
            m[11],
            m[12],
            m[13],
            m[14],
            m[15]
        ),
        VtValue::FloatArray(arr) => {
            let inner: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
            format!("[{}]", inner.join(", "))
        }
        VtValue::IntArray(arr) => {
            let inner: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
            format!("[{}]", inner.join(", "))
        }
        VtValue::Vec3fArray(arr) => {
            let inner: Vec<String> = arr
                .iter()
                .map(|v| format!("({}, {}, {})", v[0], v[1], v[2]))
                .collect();
            format!("[{}]", inner.join(", "))
        }
        VtValue::StringArray(arr) => {
            let inner: Vec<String> = arr.iter().map(|s| format!("\"{s}\"")).collect();
            format!("[{}]", inner.join(", "))
        }
    }
}

impl Default for UsdStage {
    fn default() -> Self {
        Self::new("default")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_usd_stage {
        use super::*;

        #[test]
        fn test_new_stage() {
            let stage = UsdStage::new("my_scene");
            assert_eq!(stage.name, "my_scene");
            assert!(stage.root_layer.prims.is_empty());
            assert!(stage.default_prim.is_none());
        }

        #[test]
        fn test_define_and_get_prim() {
            let mut stage = UsdStage::new("test");
            stage.define_prim(SdfPath::new("/World").unwrap(), "Xform");
            stage.define_prim(SdfPath::new("/World/Cube").unwrap(), "Mesh");
            assert!(stage.has_prim("/World"));
            assert!(stage.has_prim("/World/Cube"));
            assert!(!stage.has_prim("/World/Sphere"));
        }

        #[test]
        fn test_remove_prim() {
            let mut stage = UsdStage::new("test");
            stage.define_prim(SdfPath::new("/Temp").unwrap(), "Xform");
            assert!(stage.has_prim("/Temp"));
            assert!(stage.remove_prim("/Temp"));
            assert!(!stage.has_prim("/Temp"));
            // Removing non-existent returns false
            assert!(!stage.remove_prim("/Temp"));
        }

        #[test]
        fn test_traverse_all_prims() {
            let mut stage = UsdStage::new("test");
            stage.define_prim(SdfPath::new("/A").unwrap(), "Xform");
            stage.define_prim(SdfPath::new("/A/B").unwrap(), "Mesh");
            stage.define_prim(SdfPath::new("/A/C").unwrap(), "Camera");
            let prims = stage.traverse();
            assert_eq!(prims.len(), 3);
        }

        #[test]
        fn test_prims_of_type() {
            let mut stage = UsdStage::new("test");
            stage.define_prim(SdfPath::new("/Mesh1").unwrap(), "Mesh");
            stage.define_prim(SdfPath::new("/Mesh2").unwrap(), "Mesh");
            stage.define_prim(SdfPath::new("/Cam").unwrap(), "Camera");
            let meshes = stage.prims_of_type("Mesh");
            assert_eq!(meshes.len(), 2);
            let cams = stage.prims_of_type("Camera");
            assert_eq!(cams.len(), 1);
        }

        #[test]
        fn test_set_get_attribute() {
            let mut stage = UsdStage::new("test");
            stage.define_prim(SdfPath::new("/Sphere").unwrap(), "Sphere");
            stage
                .set_attribute("/Sphere", "radius", VtValue::Float(0.5))
                .unwrap();
            let val = stage.get_attribute("/Sphere", "radius").unwrap().unwrap();
            assert_eq!(val.as_float(), Some(0.5));
        }

        #[test]
        fn test_set_attribute_prim_not_found() {
            let mut stage = UsdStage::new("test");
            let err = stage
                .set_attribute("/NonExistent", "radius", VtValue::Float(1.0))
                .unwrap_err();
            assert!(matches!(err, crate::UsdError::PrimNotFound(_)));
        }

        #[test]
        fn test_metrics() {
            let mut stage = UsdStage::new("test");
            stage.define_prim(SdfPath::new("/World").unwrap(), "Xform");
            stage.define_prim(SdfPath::new("/World/Car").unwrap(), "Mesh");
            stage.define_prim(SdfPath::new("/World/Cam").unwrap(), "Camera");
            stage.define_prim(SdfPath::new("/World/Light").unwrap(), "SphereLight");
            let m = stage.metrics();
            assert_eq!(m.prim_count, 4);
            assert_eq!(m.mesh_count, 1);
            assert_eq!(m.camera_count, 1);
            assert_eq!(m.light_count, 1);
            assert_eq!(m.xform_count, 1);
        }

        #[test]
        fn test_json_roundtrip() {
            let mut stage = UsdStage::new("shot_010");
            stage.define_prim(SdfPath::new("/World").unwrap(), "Xform");
            stage.default_prim = Some("World".to_string());
            let json = stage.to_json().unwrap();
            let back = UsdStage::from_json(&json).unwrap();
            assert_eq!(back.name, "shot_010");
            assert!(back.has_prim("/World"));
            assert_eq!(back.default_prim.as_deref(), Some("World"));
        }

        #[test]
        fn test_export_usda_header() {
            let mut stage = UsdStage::new("test");
            stage.root_layer.frames_per_second = Some(24.0);
            stage.root_layer.start_time_code = Some(1.0);
            stage.root_layer.end_time_code = Some(120.0);
            stage.default_prim = Some("World".to_string());
            let usda = stage.export_usda();
            assert!(usda.starts_with("#usda 1.0"));
            assert!(usda.contains("framesPerSecond = 24"));
            assert!(usda.contains("startTimeCode = 1"));
            assert!(usda.contains("defaultPrim = \"World\""));
        }

        #[test]
        fn test_export_usda_prim() {
            let mut stage = UsdStage::new("test");
            let path = SdfPath::new("/Cube").unwrap();
            stage.define_prim(path, "Mesh");
            stage
                .set_attribute("/Cube", "extent", VtValue::Vec3f(1.0, 1.0, 1.0))
                .unwrap();
            let usda = stage.export_usda();
            assert!(usda.contains("def \"Mesh\" \"Cube\""));
            assert!(usda.contains("float3 extent = (1, 1, 1)"));
        }

        #[test]
        fn test_sublayer_opinion_precedence() {
            let mut sub = UsdLayer::new("sublayer.usda");
            sub.define_prim(UsdPrim::new(SdfPath::new("/SharedPrim").unwrap(), "Mesh"));
            sub.define_prim(UsdPrim::new(SdfPath::new("/SubOnly").unwrap(), "Sphere"));

            let mut stage = UsdStage::new("composed");
            stage.define_prim(SdfPath::new("/SharedPrim").unwrap(), "Xform"); // override in root
            stage.sublayers.push(sub);

            // Root layer overrides sublayer
            let prim = stage.get_prim("/SharedPrim").unwrap();
            assert_eq!(prim.type_name, "Xform");

            // SubOnly still accessible
            assert!(stage.has_prim("/SubOnly"));
        }
    }

    mod test_vt_to_usda {
        use super::*;

        #[test]
        fn test_float_array() {
            let val = VtValue::FloatArray(vec![1.0, 2.0, 3.0]);
            let s = vt_to_usda(&val);
            assert_eq!(s, "[1, 2, 3]");
        }

        #[test]
        fn test_asset_path() {
            let val = VtValue::Asset("/textures/diffuse.png".to_string());
            let s = vt_to_usda(&val);
            assert_eq!(s, "@/textures/diffuse.png@");
        }

        #[test]
        fn test_matrix4d() {
            let identity: [f64; 16] = [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ];
            let s = vt_to_usda(&VtValue::Matrix4d(identity));
            assert!(s.starts_with("((1, 0, 0, 0)"));
        }

        #[test]
        fn test_string_array() {
            let val = VtValue::StringArray(vec!["a".to_string(), "b".to_string()]);
            let s = vt_to_usda(&val);
            assert_eq!(s, "[\"a\", \"b\"]");
        }
    }
}
