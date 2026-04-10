//! Mock adapter configuration and builder.

use std::collections::HashMap;

use crate::adapters::{
    BoundingBox, ObjectTransform, SceneInfo, SceneNode, SceneObject, SceneStatistics,
    ScriptLanguage,
};

use super::helpers::{current_platform, minimal_png};

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

impl Default for MockConfig {
    fn default() -> Self {
        // Build a small default scene: one mesh + one camera
        let mesh = SceneObject {
            name: "pCube1".to_string(),
            long_name: "|pCube1".to_string(),
            object_type: "mesh".to_string(),
            parent: None,
            visible: true,
            metadata: HashMap::new(),
        };
        let camera = SceneObject {
            name: "persp".to_string(),
            long_name: "|persp".to_string(),
            object_type: "camera".to_string(),
            parent: None,
            visible: true,
            metadata: HashMap::new(),
        };

        let mut transforms = HashMap::new();
        transforms.insert("pCube1".to_string(), ObjectTransform::identity());
        transforms.insert("|pCube1".to_string(), ObjectTransform::identity());

        let mut bounding_boxes = HashMap::new();
        bounding_boxes.insert(
            "pCube1".to_string(),
            BoundingBox {
                min: [-1.0, 0.0, -1.0],
                max: [1.0, 2.0, 1.0],
            },
        );

        let mut render_settings = HashMap::new();
        render_settings.insert("width".to_string(), "1920".to_string());
        render_settings.insert("height".to_string(), "1080".to_string());
        render_settings.insert("renderer".to_string(), "default".to_string());
        render_settings.insert("samples".to_string(), "64".to_string());

        let mesh_node = SceneNode {
            object: mesh.clone(),
            children: vec![],
        };
        let camera_node = SceneNode {
            object: camera.clone(),
            children: vec![],
        };

        Self {
            dcc_type: "mock".to_string(),
            version: "1.0.0".to_string(),
            python_version: Some("3.11.0".to_string()),
            platform: current_platform().to_string(),
            pid: std::process::id(),
            metadata: HashMap::new(),
            supported_languages: vec![ScriptLanguage::Python],
            scene: SceneInfo {
                file_path: String::new(),
                name: "untitled".to_string(),
                modified: false,
                format: ".mock".to_string(),
                frame_range: Some((1.0, 100.0)),
                current_frame: Some(1.0),
                fps: Some(24.0),
                up_axis: Some("y".to_string()),
                units: Some("cm".to_string()),
                statistics: SceneStatistics::default(),
                metadata: HashMap::new(),
            },
            snapshot_enabled: true,
            snapshot_data: minimal_png(),
            script_handler: None,
            health_check_latency_ms: 1,
            connect_should_fail: false,
            connect_error_message: "Simulated connection failure".to_string(),
            objects: vec![mesh, camera],
            selection: vec![],
            hierarchy: vec![mesh_node, camera_node],
            transforms,
            bounding_boxes,
            render_time_ms: 100,
            render_settings,
        }
    }
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

impl MockConfigBuilder {
    /// Set the DCC type.
    #[must_use]
    pub fn dcc_type(mut self, dcc_type: impl Into<String>) -> Self {
        self.config.dcc_type = dcc_type.into();
        self
    }

    /// Set the version string.
    #[must_use]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.config.version = version.into();
        self
    }

    /// Set the Python version.
    #[must_use]
    pub fn python_version(mut self, version: impl Into<String>) -> Self {
        self.config.python_version = Some(version.into());
        self
    }

    /// Set no Python version (e.g. for Unity mock).
    #[must_use]
    pub fn no_python(mut self) -> Self {
        self.config.python_version = None;
        self
    }

    /// Set the platform.
    #[must_use]
    pub fn platform(mut self, platform: impl Into<String>) -> Self {
        self.config.platform = platform.into();
        self
    }

    /// Set the process ID.
    #[must_use]
    pub fn pid(mut self, pid: u32) -> Self {
        self.config.pid = pid;
        self
    }

    /// Add metadata entry.
    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.metadata.insert(key.into(), value.into());
        self
    }

    /// Set supported script languages.
    #[must_use]
    pub fn supported_languages(mut self, languages: Vec<ScriptLanguage>) -> Self {
        self.config.supported_languages = languages;
        self
    }

    /// Set initial scene info.
    #[must_use]
    pub fn scene(mut self, scene: SceneInfo) -> Self {
        self.config.scene = scene;
        self
    }

    /// Enable or disable snapshot support.
    #[must_use]
    pub fn snapshot_enabled(mut self, enabled: bool) -> Self {
        self.config.snapshot_enabled = enabled;
        self
    }

    /// Set a custom script execution handler.
    #[must_use]
    pub fn script_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(&str, ScriptLanguage, Option<u64>) -> Result<String, String> + Send + Sync + 'static,
    {
        self.config.script_handler = Some(Box::new(handler));
        self
    }

    /// Set the simulated health check latency.
    #[must_use]
    pub fn health_check_latency_ms(mut self, ms: u64) -> Self {
        self.config.health_check_latency_ms = ms;
        self
    }

    /// Make connect() fail with the given message.
    #[must_use]
    pub fn connect_should_fail(mut self, message: impl Into<String>) -> Self {
        self.config.connect_should_fail = true;
        self.config.connect_error_message = message.into();
        self
    }

    /// Set initial scene objects (for DccSceneManager).
    #[must_use]
    pub fn objects(mut self, objects: Vec<SceneObject>) -> Self {
        self.config.objects = objects;
        self
    }

    /// Set initial render settings.
    #[must_use]
    pub fn render_settings(mut self, settings: HashMap<String, String>) -> Self {
        self.config.render_settings = settings;
        self
    }

    /// Set simulated render time in milliseconds.
    #[must_use]
    pub fn render_time_ms(mut self, ms: u64) -> Self {
        self.config.render_time_ms = ms;
        self
    }

    /// Build the configuration.
    #[must_use]
    pub fn build(self) -> MockConfig {
        self.config
    }
}

// ── Preset Factories ──

/// Preset mock configurations for common DCC types.
impl MockConfig {
    /// Create a Maya mock configuration.
    #[must_use]
    pub fn maya(version: &str) -> Self {
        Self {
            dcc_type: "maya".to_string(),
            version: version.to_string(),
            python_version: Some("3.10.11".to_string()),
            supported_languages: vec![ScriptLanguage::Python, ScriptLanguage::Mel],
            scene: SceneInfo {
                format: ".ma".to_string(),
                up_axis: Some("y".to_string()),
                units: Some("cm".to_string()),
                fps: Some(24.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a Blender mock configuration.
    #[must_use]
    pub fn blender(version: &str) -> Self {
        Self {
            dcc_type: "blender".to_string(),
            version: version.to_string(),
            python_version: Some("3.11.0".to_string()),
            supported_languages: vec![ScriptLanguage::Python],
            scene: SceneInfo {
                format: ".blend".to_string(),
                up_axis: Some("z".to_string()),
                units: Some("m".to_string()),
                fps: Some(24.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a Houdini mock configuration.
    #[must_use]
    pub fn houdini(version: &str) -> Self {
        Self {
            dcc_type: "houdini".to_string(),
            version: version.to_string(),
            python_version: Some("3.10.10".to_string()),
            supported_languages: vec![
                ScriptLanguage::Python,
                ScriptLanguage::HScript,
                ScriptLanguage::Vex,
            ],
            scene: SceneInfo {
                format: ".hip".to_string(),
                up_axis: Some("y".to_string()),
                units: Some("m".to_string()),
                fps: Some(24.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a 3ds Max mock configuration.
    #[must_use]
    pub fn max_3ds(version: &str) -> Self {
        Self {
            dcc_type: "3dsmax".to_string(),
            version: version.to_string(),
            python_version: Some("3.11.0".to_string()),
            supported_languages: vec![ScriptLanguage::Python, ScriptLanguage::MaxScript],
            scene: SceneInfo {
                format: ".max".to_string(),
                up_axis: Some("z".to_string()),
                units: Some("cm".to_string()),
                fps: Some(30.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create an Unreal Engine mock configuration (HTTP-based, no Python on DCC side).
    #[must_use]
    pub fn unreal(version: &str) -> Self {
        Self {
            dcc_type: "unreal".to_string(),
            version: version.to_string(),
            python_version: Some("3.11.0".to_string()),
            supported_languages: vec![ScriptLanguage::Python, ScriptLanguage::Blueprint],
            scene: SceneInfo {
                format: ".umap".to_string(),
                up_axis: Some("z".to_string()),
                units: Some("cm".to_string()),
                fps: Some(30.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a Unity mock configuration (no Python, C# only).
    #[must_use]
    pub fn unity(version: &str) -> Self {
        Self {
            dcc_type: "unity".to_string(),
            version: version.to_string(),
            python_version: None,
            supported_languages: vec![ScriptLanguage::CSharp],
            scene: SceneInfo {
                format: ".unity".to_string(),
                up_axis: Some("y".to_string()),
                units: Some("m".to_string()),
                fps: Some(60.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create a Photoshop mock configuration (2D, image layers).
    #[must_use]
    pub fn photoshop(version: &str) -> Self {
        Self {
            dcc_type: "photoshop".to_string(),
            version: version.to_string(),
            python_version: Some("3.11.0".to_string()),
            supported_languages: vec![ScriptLanguage::Python],
            scene: SceneInfo {
                format: ".psd".to_string(),
                up_axis: None,
                units: Some("px".to_string()),
                fps: None,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
