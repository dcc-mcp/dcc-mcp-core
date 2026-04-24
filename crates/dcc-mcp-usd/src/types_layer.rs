//! `UsdLayer` — fundamental unit of USD composition.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::prim::UsdPrim;

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
