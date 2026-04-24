//! `UsdPrim` — fundamental addressable unit of a USD stage.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::attribute::UsdAttribute;
use super::sdf_path::SdfPath;

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
