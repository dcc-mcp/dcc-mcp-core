//! `UsdStageMetrics` — high-level statistics about a USD stage.

use serde::{Deserialize, Serialize};

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
