//! Core USD data types.
//!
//! This module provides Rust-native representations of the fundamental USD
//! concepts: `SdfPath`, `VtValue`, `UsdAttribute`, `UsdPrim`, `UsdLayer`,
//! and `UsdStage`.
//!
//! # Why pure Rust instead of OpenUSD C++ bindings?
//!
//! OpenUSD C++ bindings (`usd-rs`) are still experimental and require a full
//! OpenUSD build, which is a 30-minute compile that is impractical for a
//! lightweight core library.  Instead we provide:
//!
//! - A **pure-Rust USD data model** sufficient for scene description exchange.
//! - **USDA (text) serialization**: every `UsdStage` can be written to / read
//!   from the human-readable `.usda` format.
//! - **JSON interop**: stages can be serialized to/from JSON for transport
//!   over the MCP IPC layer.
//! - A **`DccSceneInfo` bridge** so any DCC's scene info can be converted to
//!   a USD-compatible representation.
//!
//! Once `usd-rs` stabilizes, this module can be extended with C++ bridging
//! while keeping the same public API surface.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── SdfPath ──────────────────────────────────────────────────────────────────

/// A USD scene description path (e.g. `/World/Cube`, `/Root`).
///
/// USD paths use forward slashes and start with `/` for absolute paths.
/// Relative paths are also supported (no leading `/`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SdfPath(String);

impl SdfPath {
    /// The absolute root path (`/`).
    pub const ROOT: &'static str = "/";

    /// Create a new `SdfPath`.  Returns an error if the path is empty.
    pub fn new(path: impl Into<String>) -> crate::UsdResult<Self> {
        let s = path.into();
        if s.is_empty() {
            return Err(crate::UsdError::InvalidPath(
                "path must not be empty".to_string(),
            ));
        }
        Ok(Self(s))
    }

    /// Create the absolute root path `/`.
    pub fn root() -> Self {
        Self("/".to_string())
    }

    /// Return the string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is an absolute path (starts with `/`).
    pub fn is_absolute(&self) -> bool {
        self.0.starts_with('/')
    }

    /// Return the parent path.  Returns `None` for the root path.
    pub fn parent(&self) -> Option<Self> {
        if self.0 == "/" {
            return None;
        }
        let idx = self.0.rfind('/')?;
        if idx == 0 {
            Some(Self("/".to_string()))
        } else {
            Some(Self(self.0[..idx].to_string()))
        }
    }

    /// Append a child segment to this path.
    ///
    /// ```
    /// use dcc_mcp_usd::types::SdfPath;
    /// let root = SdfPath::new("/World").unwrap();
    /// let child = root.child("Cube").unwrap();
    /// assert_eq!(child.as_str(), "/World/Cube");
    /// ```
    pub fn child(&self, name: &str) -> crate::UsdResult<Self> {
        if name.is_empty() {
            return Err(crate::UsdError::InvalidPath(
                "child name must not be empty".to_string(),
            ));
        }
        if self.0.ends_with('/') {
            Ok(Self(format!("{}{}", self.0, name)))
        } else {
            Ok(Self(format!("{}/{}", self.0, name)))
        }
    }

    /// Return the last path element name.
    ///
    /// For `/World/Cube` returns `"Cube"`.
    /// For `/` returns `""`.
    pub fn name(&self) -> &str {
        match self.0.rfind('/') {
            Some(idx) if idx < self.0.len() - 1 => &self.0[idx + 1..],
            _ => "",
        }
    }
}

impl std::fmt::Display for SdfPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── VtValue ───────────────────────────────────────────────────────────────────

/// A variant value type representing USD attribute values.
///
/// This is a simplified version of USD's `VtValue` that covers the most
/// common attribute types encountered in DCC workflows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum VtValue {
    /// Boolean
    Bool(bool),
    /// 32-bit integer
    Int(i32),
    /// 64-bit integer
    Int64(i64),
    /// Single-precision float
    Float(f32),
    /// Double-precision float
    Double(f64),
    /// UTF-8 string
    String(String),
    /// USD asset path (a string referencing an external file)
    Asset(String),
    /// Token (enumeration-like identifier in USD)
    Token(String),
    /// 2D vector (x, y)
    Vec2f(f32, f32),
    /// 3D vector (x, y, z)
    Vec3f(f32, f32, f32),
    /// 4D vector (x, y, z, w)
    Vec4f(f32, f32, f32, f32),
    /// 4×4 matrix (row-major, 16 elements)
    Matrix4d([f64; 16]),
    /// Array of floats
    FloatArray(Vec<f32>),
    /// Array of integers
    IntArray(Vec<i32>),
    /// Array of 3D vectors
    Vec3fArray(Vec<[f32; 3]>),
    /// Array of strings
    StringArray(Vec<String>),
}

impl VtValue {
    /// Return the USD type name string (e.g. `"float3"`, `"token"`).
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Int64(_) => "int64",
            Self::Float(_) => "float",
            Self::Double(_) => "double",
            Self::String(_) => "string",
            Self::Asset(_) => "asset",
            Self::Token(_) => "token",
            Self::Vec2f(..) => "float2",
            Self::Vec3f(..) => "float3",
            Self::Vec4f(..) => "float4",
            Self::Matrix4d(_) => "matrix4d",
            Self::FloatArray(_) => "float[]",
            Self::IntArray(_) => "int[]",
            Self::Vec3fArray(_) => "float3[]",
            Self::StringArray(_) => "string[]",
        }
    }

    /// Try to extract a float value, promoting `Int` and `Double` if needed.
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Double(v) => Some(*v as f32),
            Self::Int(v) => Some(*v as f32),
            _ => None,
        }
    }

    /// Try to extract a string value (for `String`, `Token`, and `Asset`).
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::Token(s) | Self::Asset(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

// ── UsdAttribute ─────────────────────────────────────────────────────────────

/// A USD attribute on a prim.
///
/// Attributes hold typed values and can optionally be time-varying.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsdAttribute {
    /// Attribute name (e.g. `"xformOp:translate"`).
    pub name: String,
    /// The attribute's value at the default time.
    pub default_value: Option<VtValue>,
    /// Time-sampled values: frame → value.
    #[serde(default)]
    pub time_samples: HashMap<String, VtValue>,
    /// Whether this attribute is custom (not part of the schema).
    #[serde(default)]
    pub custom: bool,
    /// Metadata on the attribute.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl UsdAttribute {
    /// Create a new attribute with a default value.
    pub fn new(name: impl Into<String>, value: VtValue) -> Self {
        Self {
            name: name.into(),
            default_value: Some(value),
            time_samples: HashMap::new(),
            custom: false,
            metadata: HashMap::new(),
        }
    }

    /// Create a custom attribute (not in the schema).
    pub fn custom(name: impl Into<String>, value: VtValue) -> Self {
        Self {
            name: name.into(),
            default_value: Some(value),
            time_samples: HashMap::new(),
            custom: true,
            metadata: HashMap::new(),
        }
    }

    /// Get the value at a given time code.  Falls back to the default value.
    ///
    /// Keys are stored as decimal strings.  Both `"24"` and `"24.0"` are tried.
    pub fn get_at(&self, time_code: f64) -> Option<&VtValue> {
        // Try both "24.0" and "24" formats (Rust Display for f64 can vary)
        let key1 = format!("{time_code}");
        let key2 = if time_code.fract() == 0.0 {
            format!("{}", time_code as i64)
        } else {
            key1.clone()
        };
        self.time_samples
            .get(&key1)
            .or_else(|| self.time_samples.get(&key2))
            .or(self.default_value.as_ref())
    }
}

// ── UsdPrim ───────────────────────────────────────────────────────────────────

/// A prim (primitive) in a USD stage.
///
/// Prims are the fundamental addressable unit in USD.  Each prim has:
/// - A path (e.g. `/World/Cube`)
/// - A type name (e.g. `"Mesh"`, `"Xform"`, `"Camera"`)
/// - Attributes
/// - Metadata
/// - Child prims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsdPrim {
    /// Absolute path of this prim within the stage.
    pub path: SdfPath,
    /// USD type name (e.g. `"Mesh"`, `"Xform"`, `"Sphere"`, `"Camera"`).
    pub type_name: String,
    /// Whether this prim is active (inactive prims are not rendered/evaluated).
    #[serde(default = "default_true")]
    pub active: bool,
    /// API schemas applied to this prim (e.g. `["GeomModelAPI"]`).
    #[serde(default)]
    pub applied_schemas: Vec<String>,
    /// Attributes keyed by name.
    #[serde(default)]
    pub attributes: HashMap<String, UsdAttribute>,
    /// Child prim paths (maintained for hierarchy navigation).
    #[serde(default)]
    pub children: Vec<SdfPath>,
    /// Arbitrary metadata dictionary.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Purpose hint: `"default"`, `"render"`, `"proxy"`, or `"guide"`.
    #[serde(default)]
    pub purpose: String,
    /// Kind: `"component"`, `"group"`, `"assembly"`, `"subcomponent"`, etc.
    #[serde(default)]
    pub kind: String,
}

fn default_true() -> bool {
    true
}

impl UsdPrim {
    /// Create a new prim with a type name.
    pub fn new(path: SdfPath, type_name: impl Into<String>) -> Self {
        Self {
            path,
            type_name: type_name.into(),
            active: true,
            applied_schemas: Vec::new(),
            attributes: HashMap::new(),
            children: Vec::new(),
            metadata: HashMap::new(),
            purpose: String::new(),
            kind: String::new(),
        }
    }

    /// Create a root pseudo-prim at `/`.
    pub fn root() -> Self {
        Self::new(SdfPath::root(), "")
    }

    /// Return the prim's name (last path element).
    pub fn name(&self) -> &str {
        self.path.name()
    }

    /// Add an attribute to this prim.
    pub fn add_attribute(&mut self, attr: UsdAttribute) {
        self.attributes.insert(attr.name.clone(), attr);
    }

    /// Get an attribute by name.
    pub fn get_attribute(&self, name: &str) -> Option<&UsdAttribute> {
        self.attributes.get(name)
    }

    /// Whether this prim has the given API schema applied.
    pub fn has_api(&self, schema: &str) -> bool {
        self.applied_schemas.iter().any(|s| s == schema)
    }
}

// ── UsdLayer ──────────────────────────────────────────────────────────────────

/// A USD layer — the fundamental unit of composition.
///
/// Layers map paths to prim descriptions.  A `UsdStage` is composed from
/// one or more layers.  The simplest case is a single `root` layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsdLayer {
    /// Unique identifier for this layer (usually a file path or UUID).
    pub identifier: String,
    /// Display name for the layer.
    #[serde(default)]
    pub display_name: String,
    /// Comment/documentation string.
    #[serde(default)]
    pub comment: String,
    /// Up axis: `"Y"` (default) or `"Z"`.
    #[serde(default = "default_y_axis")]
    pub up_axis: String,
    /// Meters per unit (1.0 = meter, 0.01 = centimeter).
    #[serde(default = "default_mpu")]
    pub meters_per_unit: f64,
    /// Default time code (frame).
    pub default_time_code: Option<f64>,
    /// Start time code.
    pub start_time_code: Option<f64>,
    /// End time code.
    pub end_time_code: Option<f64>,
    /// Frames per second.
    pub frames_per_second: Option<f64>,
    /// Prims defined in this layer, keyed by path string.
    #[serde(default)]
    pub prims: HashMap<String, UsdPrim>,
    /// Sub-layer paths (files this layer references).
    #[serde(default)]
    pub sub_layers: Vec<String>,
    /// Arbitrary layer-level metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

fn default_y_axis() -> String {
    "Y".to_string()
}
fn default_mpu() -> f64 {
    1.0
}

impl UsdLayer {
    /// Create a new empty layer with the given identifier.
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
            display_name: String::new(),
            comment: String::new(),
            up_axis: "Y".to_string(),
            meters_per_unit: 1.0,
            default_time_code: None,
            start_time_code: None,
            end_time_code: None,
            frames_per_second: None,
            prims: HashMap::new(),
            sub_layers: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add a prim to this layer.
    pub fn define_prim(&mut self, prim: UsdPrim) {
        self.prims.insert(prim.path.to_string(), prim);
    }

    /// Get a prim by path string.
    pub fn get_prim(&self, path: &str) -> Option<&UsdPrim> {
        self.prims.get(path)
    }

    /// Get a mutable reference to a prim.
    pub fn get_prim_mut(&mut self, path: &str) -> Option<&mut UsdPrim> {
        self.prims.get_mut(path)
    }

    /// Return all prims defined in this layer.
    pub fn all_prims(&self) -> impl Iterator<Item = &UsdPrim> {
        self.prims.values()
    }
}

// ── UsdStageMetrics ──────────────────────────────────────────────────────────

/// High-level statistics about a USD stage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsdStageMetrics {
    /// Total number of prims across all layers.
    pub prim_count: usize,
    /// Number of mesh prims.
    pub mesh_count: usize,
    /// Number of camera prims.
    pub camera_count: usize,
    /// Number of light prims.
    pub light_count: usize,
    /// Number of material prims.
    pub material_count: usize,
    /// Number of xform (transform) prims.
    pub xform_count: usize,
}

#[cfg(test)]
mod tests {
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
    }
}
