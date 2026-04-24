//! Mock adapter configuration and builder.

use std::collections::HashMap;

use crate::adapters::{
    BoundingBox, ObjectTransform, SceneInfo, SceneNode, SceneObject, ScriptLanguage,
};

#[path = "config_builder.rs"]
mod builder_impl;
#[path = "config_defaults.rs"]
mod defaults_impl;
#[path = "config_presets.rs"]
mod presets_impl;

/// Script execution handler type.
///
/// Receives `(code, language, timeout_ms)` and returns `Ok(output)` or `Err(error_message)`.
pub type ScriptHandler =
    Box<dyn Fn(&str, ScriptLanguage, Option<u64>) -> Result<String, String> + Send + Sync>;

/// Configuration for the mock DCC adapter.
pub struct MockConfig {
    /// DCC type identifier (default: "mock").
    pub dcc_type: String,
    /// Version string (default: "1.0.0").
    pub version: String,
    /// Python version (default: Some("3.11.0")).
    pub python_version: Option<String>,
    /// Platform (default: current OS).
    pub platform: String,
    /// Process ID (default: current PID).
    pub pid: u32,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
    /// Supported script languages (default: [Python]).
    pub supported_languages: Vec<ScriptLanguage>,
    /// Initial scene info.
    pub scene: SceneInfo,
    /// Whether snapshot is supported (default: true).
    pub snapshot_enabled: bool,
    /// Mock snapshot image data (default: 1x1 PNG).
    pub snapshot_data: Vec<u8>,
    /// Custom script execution handler.
    pub script_handler: Option<ScriptHandler>,
    /// Simulated health check latency in milliseconds (default: 1).
    pub health_check_latency_ms: u64,
    /// Whether connect should fail (for error path testing).
    pub connect_should_fail: bool,
    /// Error message when connect fails.
    pub connect_error_message: String,

    // ── Cross-DCC Protocol mock state ──
    /// Mock scene objects for `DccSceneManager::list_objects`.
    pub objects: Vec<SceneObject>,
    /// Mock selection for `DccSceneManager::get/set_selection`.
    pub selection: Vec<String>,
    /// Mock hierarchy tree for `DccHierarchy::get_hierarchy`.
    pub hierarchy: Vec<SceneNode>,
    /// Mock transforms keyed by object name (short or long).
    pub transforms: HashMap<String, ObjectTransform>,
    /// Mock bounding boxes keyed by object name.
    pub bounding_boxes: HashMap<String, BoundingBox>,
    /// Simulated render time in ms (default: 100).
    pub render_time_ms: u64,
    /// Mock render settings key-value pairs.
    pub render_settings: HashMap<String, String>,
}

/// Builder for [`MockConfig`].
pub struct MockConfigBuilder {
    pub(super) config: MockConfig,
}

impl MockConfig {
    /// Create a builder for MockConfig.
    #[must_use]
    pub fn builder() -> MockConfigBuilder {
        MockConfigBuilder {
            config: MockConfig::default(),
        }
    }
}
