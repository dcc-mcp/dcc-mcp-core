//! Mock DCC adapter — a fully functional mock for development and testing.
//!
//! Provides [`MockDccAdapter`], a configurable mock that implements all DCC adapter
//! traits ([`DccConnection`], [`DccScriptEngine`], [`DccSceneInfo`], [`DccSnapshot`]).
//! Designed for:
//!
//! - **Unit testing** without a real DCC application
//! - **Integration testing** of the MCP server → core → adapter pipeline
//! - **Development** of new DCC integrations (use as a reference implementation)
//! - **CI/CD** environments where no DCC is available
//!
//! # Quick Start
//!
//! ```rust
//! use dcc_mcp_protocols::mock::{MockDccAdapter, MockConfig};
//!
//! // Create with defaults (a "maya" mock)
//! let mut adapter = MockDccAdapter::new();
//!
//! // Or customize
//! let config = MockConfig::builder()
//!     .dcc_type("blender")
//!     .version("4.1.0")
//!     .python_version("3.11.0")
//!     .platform("linux")
//!     .build();
//! let mut adapter = MockDccAdapter::with_config(config);
//! ```
//!
//! # Script Execution
//!
//! The mock adapter executes scripts by returning the script source as output.
//! You can inject custom behavior via [`MockConfig::script_handler`]:
//!
//! ```rust
//! use dcc_mcp_protocols::mock::MockConfig;
//! use dcc_mcp_protocols::adapters::ScriptLanguage;
//!
//! let config = MockConfig::builder()
//!     .script_handler(|code, lang, _timeout| {
//!         if code.contains("error") {
//!             Err("Simulated error".to_string())
//!         } else {
//!             Ok(format!("[{}] {}", lang, code))
//!         }
//!     })
//!     .build();
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use crate::adapters::{
    CaptureResult, DccAdapter, DccCapabilities, DccConnection, DccError, DccErrorCode, DccInfo,
    DccResult, DccSceneInfo, DccScriptEngine, DccSnapshot, SceneInfo, SceneStatistics,
    ScriptLanguage, ScriptResult,
};

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
}

impl Default for MockConfig {
    fn default() -> Self {
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
        }
    }
}

/// Builder for [`MockConfig`].
pub struct MockConfigBuilder {
    config: MockConfig,
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

    /// Build the configuration.
    #[must_use]
    pub fn build(self) -> MockConfig {
        self.config
    }
}

/// A fully functional mock DCC adapter for testing and development.
///
/// Implements all DCC adapter traits with configurable behavior:
/// - Connection: tracks connected/disconnected state
/// - Script execution: echo-back or custom handler
/// - Scene info: configurable scene with mutable statistics
/// - Snapshot: returns configurable image data
///
/// All operations track invocation counts via atomic counters, useful for
/// verifying test expectations.
pub struct MockDccAdapter {
    info: DccInfo,
    connected: AtomicBool,
    scene: parking_lot::RwLock<SceneInfo>,
    capabilities: DccCapabilities,
    snapshot_enabled: bool,
    snapshot_data: Vec<u8>,
    script_handler: Option<ScriptHandler>,
    health_check_latency_ms: u64,
    connect_should_fail: bool,
    connect_error_message: String,

    // Invocation counters
    connect_count: AtomicU64,
    disconnect_count: AtomicU64,
    script_count: AtomicU64,
    scene_query_count: AtomicU64,
    snapshot_count: AtomicU64,
    health_check_count: AtomicU64,
}

impl MockDccAdapter {
    /// Create a new mock adapter with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MockConfig::default())
    }

    /// Create a new mock adapter with custom configuration.
    #[must_use]
    pub fn with_config(config: MockConfig) -> Self {
        let capabilities = DccCapabilities {
            script_languages: config.supported_languages.clone(),
            scene_info: true,
            snapshot: config.snapshot_enabled,
            undo_redo: false,
            progress_reporting: false,
            file_operations: true,
            selection: true,
            extensions: HashMap::new(),
        };

        Self {
            info: DccInfo {
                dcc_type: config.dcc_type,
                version: config.version,
                python_version: config.python_version,
                platform: config.platform,
                pid: config.pid,
                metadata: config.metadata,
            },
            connected: AtomicBool::new(false),
            scene: parking_lot::RwLock::new(config.scene),
            capabilities,
            snapshot_enabled: config.snapshot_enabled,
            snapshot_data: config.snapshot_data,
            script_handler: config.script_handler,
            health_check_latency_ms: config.health_check_latency_ms,
            connect_should_fail: config.connect_should_fail,
            connect_error_message: config.connect_error_message,
            connect_count: AtomicU64::new(0),
            disconnect_count: AtomicU64::new(0),
            script_count: AtomicU64::new(0),
            scene_query_count: AtomicU64::new(0),
            snapshot_count: AtomicU64::new(0),
            health_check_count: AtomicU64::new(0),
        }
    }

    // ── Invocation counters (for test assertions) ──

    /// Number of times `connect()` was called.
    pub fn connect_count(&self) -> u64 {
        self.connect_count.load(Ordering::Relaxed)
    }

    /// Number of times `disconnect()` was called.
    pub fn disconnect_count(&self) -> u64 {
        self.disconnect_count.load(Ordering::Relaxed)
    }

    /// Number of times `execute_script()` was called.
    pub fn script_count(&self) -> u64 {
        self.script_count.load(Ordering::Relaxed)
    }

    /// Number of times scene info was queried.
    pub fn scene_query_count(&self) -> u64 {
        self.scene_query_count.load(Ordering::Relaxed)
    }

    /// Number of times `capture_viewport()` was called.
    pub fn snapshot_count(&self) -> u64 {
        self.snapshot_count.load(Ordering::Relaxed)
    }

    /// Number of times `health_check()` was called.
    pub fn health_check_count(&self) -> u64 {
        self.health_check_count.load(Ordering::Relaxed)
    }

    /// Reset all invocation counters.
    pub fn reset_counters(&self) {
        self.connect_count.store(0, Ordering::Relaxed);
        self.disconnect_count.store(0, Ordering::Relaxed);
        self.script_count.store(0, Ordering::Relaxed);
        self.scene_query_count.store(0, Ordering::Relaxed);
        self.snapshot_count.store(0, Ordering::Relaxed);
        self.health_check_count.store(0, Ordering::Relaxed);
    }

    // ── Scene manipulation (for test setup) ──

    /// Update the scene info (e.g. simulate opening a file).
    pub fn set_scene(&self, scene: SceneInfo) {
        *self.scene.write() = scene;
    }

    /// Update scene statistics.
    pub fn set_statistics(&self, stats: SceneStatistics) {
        self.scene.write().statistics = stats;
    }

    /// Mark the scene as modified or unmodified.
    pub fn set_modified(&self, modified: bool) {
        self.scene.write().modified = modified;
    }
}

impl Default for MockDccAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// ── DccConnection ──

impl DccConnection for MockDccAdapter {
    fn connect(&mut self) -> DccResult<()> {
        self.connect_count.fetch_add(1, Ordering::Relaxed);

        if self.connect_should_fail {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: self.connect_error_message.clone(),
                details: None,
                recoverable: true,
            });
        }

        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn disconnect(&mut self) -> DccResult<()> {
        self.disconnect_count.fetch_add(1, Ordering::Relaxed);
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn health_check(&self) -> DccResult<u64> {
        self.health_check_count.fetch_add(1, Ordering::Relaxed);

        if !self.connected.load(Ordering::SeqCst) {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: "Not connected".to_string(),
                details: None,
                recoverable: true,
            });
        }

        Ok(self.health_check_latency_ms)
    }
}

// ── DccScriptEngine ──

impl DccScriptEngine for MockDccAdapter {
    fn execute_script(
        &self,
        code: &str,
        language: ScriptLanguage,
        timeout_ms: Option<u64>,
    ) -> DccResult<ScriptResult> {
        self.script_count.fetch_add(1, Ordering::Relaxed);

        if !self.connected.load(Ordering::SeqCst) {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: "Not connected — call connect() first".to_string(),
                details: None,
                recoverable: true,
            });
        }

        // Check if language is supported
        if !self.capabilities.script_languages.contains(&language) {
            return Ok(ScriptResult::failure(
                format!("Unsupported language: {language}"),
                0,
            ));
        }

        let start = Instant::now();

        let result = if let Some(ref handler) = self.script_handler {
            handler(code, language, timeout_ms)
        } else {
            // Default: echo the code back as output
            Ok(code.to_string())
        };

        let elapsed_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(output) => Ok(ScriptResult::success(output, elapsed_ms)),
            Err(error) => Ok(ScriptResult::failure(error, elapsed_ms)),
        }
    }

    fn supported_languages(&self) -> Vec<ScriptLanguage> {
        self.capabilities.script_languages.clone()
    }
}

// ── DccSceneInfo ──

impl DccSceneInfo for MockDccAdapter {
    fn get_scene_info(&self) -> DccResult<SceneInfo> {
        self.scene_query_count.fetch_add(1, Ordering::Relaxed);

        if !self.connected.load(Ordering::SeqCst) {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: "Not connected".to_string(),
                details: None,
                recoverable: true,
            });
        }

        Ok(self.scene.read().clone())
    }

    fn list_objects(&self) -> DccResult<Vec<(String, String)>> {
        self.scene_query_count.fetch_add(1, Ordering::Relaxed);

        if !self.connected.load(Ordering::SeqCst) {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: "Not connected".to_string(),
                details: None,
                recoverable: true,
            });
        }

        // Return some mock objects based on scene statistics
        let stats = &self.scene.read().statistics;
        let mut objects = Vec::new();
        for i in 0..stats.object_count.min(100) {
            objects.push((format!("object_{i}"), "mesh".to_string()));
        }
        for i in 0..stats.light_count.min(10) {
            objects.push((format!("light_{i}"), "light".to_string()));
        }
        for i in 0..stats.camera_count.min(10) {
            objects.push((format!("camera_{i}"), "camera".to_string()));
        }
        Ok(objects)
    }

    fn get_selection(&self) -> DccResult<Vec<String>> {
        self.scene_query_count.fetch_add(1, Ordering::Relaxed);

        if !self.connected.load(Ordering::SeqCst) {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: "Not connected".to_string(),
                details: None,
                recoverable: true,
            });
        }

        // Return empty selection by default
        Ok(vec![])
    }
}

// ── DccSnapshot ──

impl DccSnapshot for MockDccAdapter {
    fn capture_viewport(
        &self,
        viewport: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        format: &str,
    ) -> DccResult<CaptureResult> {
        self.snapshot_count.fetch_add(1, Ordering::Relaxed);

        if !self.connected.load(Ordering::SeqCst) {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: "Not connected".to_string(),
                details: None,
                recoverable: true,
            });
        }

        if !self.snapshot_enabled {
            return Err(DccError {
                code: DccErrorCode::Unsupported,
                message: "Snapshot not supported by this mock adapter".to_string(),
                details: None,
                recoverable: false,
            });
        }

        Ok(CaptureResult {
            data: self.snapshot_data.clone(),
            width: width.unwrap_or(1920),
            height: height.unwrap_or(1080),
            format: format.to_string(),
            viewport: viewport.map(String::from),
        })
    }
}

// ── DccAdapter (top-level) ──

impl DccAdapter for MockDccAdapter {
    fn info(&self) -> &DccInfo {
        &self.info
    }

    fn capabilities(&self) -> DccCapabilities {
        self.capabilities.clone()
    }

    fn as_connection(&mut self) -> Option<&mut dyn DccConnection> {
        Some(self)
    }

    fn as_script_engine(&self) -> Option<&dyn DccScriptEngine> {
        Some(self)
    }

    fn as_scene_info(&self) -> Option<&dyn DccSceneInfo> {
        Some(self)
    }

    fn as_snapshot(&self) -> Option<&dyn DccSnapshot> {
        if self.snapshot_enabled {
            Some(self)
        } else {
            None
        }
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
}

// ── Helpers ──

/// Get the current platform string.
fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// A minimal valid 1x1 white PNG image (67 bytes).
fn minimal_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // 8-bit RGB
        0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, // IDAT chunk
        0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, // compressed data
        0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, //
        0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND chunk
        0x44, 0xAE, 0x42, 0x60, 0x82, //
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction tests ──

    mod construction {
        use super::*;

        #[test]
        fn test_default_adapter() {
            let adapter = MockDccAdapter::new();
            assert_eq!(adapter.info().dcc_type, "mock");
            assert_eq!(adapter.info().version, "1.0.0");
            assert!(adapter.info().python_version.is_some());
            assert!(!adapter.is_connected());
        }

        #[test]
        fn test_custom_config() {
            let config = MockConfig::builder()
                .dcc_type("test_dcc")
                .version("2.0.0")
                .python_version("3.9.0")
                .platform("linux")
                .pid(42)
                .metadata("renderer", "cycles")
                .build();

            let adapter = MockDccAdapter::with_config(config);
            assert_eq!(adapter.info().dcc_type, "test_dcc");
            assert_eq!(adapter.info().version, "2.0.0");
            assert_eq!(adapter.info().python_version.as_deref(), Some("3.9.0"));
            assert_eq!(adapter.info().platform, "linux");
            assert_eq!(adapter.info().pid, 42);
            assert_eq!(adapter.info().metadata["renderer"], "cycles");
        }

        #[test]
        fn test_no_python_config() {
            let config = MockConfig::builder().no_python().build();
            let adapter = MockDccAdapter::with_config(config);
            assert!(adapter.info().python_version.is_none());
        }
    }

    // ── DCC preset tests ──

    mod presets {
        use super::*;

        #[test]
        fn test_maya_preset() {
            let config = MockConfig::maya("2024.2");
            let adapter = MockDccAdapter::with_config(config);
            assert_eq!(adapter.info().dcc_type, "maya");
            assert_eq!(adapter.info().version, "2024.2");
            let caps = adapter.capabilities();
            assert!(caps.script_languages.contains(&ScriptLanguage::Python));
            assert!(caps.script_languages.contains(&ScriptLanguage::Mel));
        }

        #[test]
        fn test_blender_preset() {
            let config = MockConfig::blender("4.1.0");
            let adapter = MockDccAdapter::with_config(config);
            assert_eq!(adapter.info().dcc_type, "blender");
            assert_eq!(adapter.capabilities().script_languages.len(), 1);
        }

        #[test]
        fn test_houdini_preset() {
            let config = MockConfig::houdini("20.0.547");
            let adapter = MockDccAdapter::with_config(config);
            assert_eq!(adapter.info().dcc_type, "houdini");
            let caps = adapter.capabilities();
            assert_eq!(caps.script_languages.len(), 3);
            assert!(caps.script_languages.contains(&ScriptLanguage::Vex));
        }

        #[test]
        fn test_max_3ds_preset() {
            let config = MockConfig::max_3ds("2025");
            let adapter = MockDccAdapter::with_config(config);
            assert_eq!(adapter.info().dcc_type, "3dsmax");
            assert!(
                adapter
                    .capabilities()
                    .script_languages
                    .contains(&ScriptLanguage::MaxScript)
            );
        }

        #[test]
        fn test_unreal_preset() {
            let config = MockConfig::unreal("5.4");
            let adapter = MockDccAdapter::with_config(config);
            assert_eq!(adapter.info().dcc_type, "unreal");
            assert!(
                adapter
                    .capabilities()
                    .script_languages
                    .contains(&ScriptLanguage::Blueprint)
            );
        }

        #[test]
        fn test_unity_preset() {
            let config = MockConfig::unity("2022.3");
            let adapter = MockDccAdapter::with_config(config);
            assert_eq!(adapter.info().dcc_type, "unity");
            assert!(adapter.info().python_version.is_none());
            assert!(
                adapter
                    .capabilities()
                    .script_languages
                    .contains(&ScriptLanguage::CSharp)
            );
        }
    }

    // ── Connection tests ──

    mod connection {
        use super::*;

        #[test]
        fn test_connect_disconnect() {
            let mut adapter = MockDccAdapter::new();
            assert!(!adapter.is_connected());

            adapter.connect().unwrap();
            assert!(adapter.is_connected());
            assert_eq!(adapter.connect_count(), 1);

            adapter.disconnect().unwrap();
            assert!(!adapter.is_connected());
            assert_eq!(adapter.disconnect_count(), 1);
        }

        #[test]
        fn test_connect_failure() {
            let config = MockConfig::builder()
                .connect_should_fail("Test failure")
                .build();
            let mut adapter = MockDccAdapter::with_config(config);

            let result = adapter.connect();
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert_eq!(err.code, DccErrorCode::ConnectionFailed);
            assert!(err.message.contains("Test failure"));
            assert!(err.recoverable);
            assert!(!adapter.is_connected());
        }

        #[test]
        fn test_health_check_connected() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            let rtt = adapter.health_check().unwrap();
            assert_eq!(rtt, 1); // default latency
            assert_eq!(adapter.health_check_count(), 1);
        }

        #[test]
        fn test_health_check_disconnected() {
            let adapter = MockDccAdapter::new();
            let result = adapter.health_check();
            assert!(result.is_err());
        }

        #[test]
        fn test_custom_health_check_latency() {
            let config = MockConfig::builder().health_check_latency_ms(42).build();
            let mut adapter = MockDccAdapter::with_config(config);
            adapter.connect().unwrap();

            assert_eq!(adapter.health_check().unwrap(), 42);
        }
    }

    // ── Script execution tests ──

    mod scripts {
        use super::*;

        #[test]
        fn test_execute_script_echo() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            let result = adapter
                .execute_script("print('hello')", ScriptLanguage::Python, None)
                .unwrap();

            assert!(result.success);
            assert_eq!(result.output.as_deref(), Some("print('hello')"));
            assert_eq!(adapter.script_count(), 1);
        }

        #[test]
        fn test_execute_script_unsupported_language() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            // Default mock only supports Python
            let result = adapter
                .execute_script("some mel code", ScriptLanguage::Mel, None)
                .unwrap();

            assert!(!result.success);
            assert!(result.error.as_deref().unwrap().contains("Unsupported"));
        }

        #[test]
        fn test_execute_script_not_connected() {
            let adapter = MockDccAdapter::new();
            let result = adapter.execute_script("code", ScriptLanguage::Python, None);
            assert!(result.is_err());
            assert_eq!(result.unwrap_err().code, DccErrorCode::ConnectionFailed);
        }

        #[test]
        fn test_custom_script_handler() {
            let config = MockConfig::builder()
                .script_handler(|code, _lang, _timeout| {
                    if code.contains("error") {
                        Err("Simulated error".to_string())
                    } else {
                        Ok(format!("result: {code}"))
                    }
                })
                .build();
            let mut adapter = MockDccAdapter::with_config(config);
            adapter.connect().unwrap();

            let ok_result = adapter
                .execute_script("hello", ScriptLanguage::Python, None)
                .unwrap();
            assert!(ok_result.success);
            assert_eq!(ok_result.output.as_deref(), Some("result: hello"));

            let err_result = adapter
                .execute_script("trigger error", ScriptLanguage::Python, None)
                .unwrap();
            assert!(!err_result.success);
            assert!(err_result.error.as_deref().unwrap().contains("Simulated"));
        }

        #[test]
        fn test_supported_languages() {
            let config = MockConfig::maya("2024");
            let adapter = MockDccAdapter::with_config(config);

            let langs = adapter.supported_languages();
            assert_eq!(langs.len(), 2);
            assert!(langs.contains(&ScriptLanguage::Python));
            assert!(langs.contains(&ScriptLanguage::Mel));
        }
    }

    // ── Scene info tests ──

    mod scene {
        use super::*;

        #[test]
        fn test_get_scene_info() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            let info = adapter.get_scene_info().unwrap();
            assert_eq!(info.name, "untitled");
            assert!(!info.modified);
            assert_eq!(adapter.scene_query_count(), 1);
        }

        #[test]
        fn test_set_scene() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            adapter.set_scene(SceneInfo {
                file_path: "/projects/shot_010.ma".to_string(),
                name: "shot_010".to_string(),
                modified: true,
                format: ".ma".to_string(),
                statistics: SceneStatistics {
                    object_count: 100,
                    vertex_count: 50000,
                    ..Default::default()
                },
                ..Default::default()
            });

            let info = adapter.get_scene_info().unwrap();
            assert_eq!(info.name, "shot_010");
            assert!(info.modified);
            assert_eq!(info.statistics.object_count, 100);
        }

        #[test]
        fn test_set_modified() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            adapter.set_modified(true);
            assert!(adapter.get_scene_info().unwrap().modified);

            adapter.set_modified(false);
            assert!(!adapter.get_scene_info().unwrap().modified);
        }

        #[test]
        fn test_list_objects() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            adapter.set_statistics(SceneStatistics {
                object_count: 3,
                light_count: 2,
                camera_count: 1,
                ..Default::default()
            });

            let objects = adapter.list_objects().unwrap();
            assert_eq!(objects.len(), 6); // 3 mesh + 2 light + 1 camera
            assert_eq!(objects[0].1, "mesh");
            assert_eq!(objects[3].1, "light");
            assert_eq!(objects[5].1, "camera");
        }

        #[test]
        fn test_get_selection() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            let selection = adapter.get_selection().unwrap();
            assert!(selection.is_empty());
        }

        #[test]
        fn test_scene_query_not_connected() {
            let adapter = MockDccAdapter::new();
            assert!(adapter.get_scene_info().is_err());
            assert!(adapter.list_objects().is_err());
            assert!(adapter.get_selection().is_err());
        }
    }

    // ── Snapshot tests ──

    mod snapshot {
        use super::*;

        #[test]
        fn test_capture_viewport() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            let result = adapter
                .capture_viewport(Some("persp"), Some(800), Some(600), "png")
                .unwrap();

            assert_eq!(result.width, 800);
            assert_eq!(result.height, 600);
            assert_eq!(result.format, "png");
            assert_eq!(result.viewport.as_deref(), Some("persp"));
            assert!(!result.data.is_empty());
            assert_eq!(adapter.snapshot_count(), 1);
        }

        #[test]
        fn test_capture_default_resolution() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();

            let result = adapter.capture_viewport(None, None, None, "png").unwrap();
            assert_eq!(result.width, 1920);
            assert_eq!(result.height, 1080);
        }

        #[test]
        fn test_snapshot_disabled() {
            let config = MockConfig::builder().snapshot_enabled(false).build();
            let mut adapter = MockDccAdapter::with_config(config);
            adapter.connect().unwrap();

            let result = adapter.capture_viewport(None, None, None, "png");
            assert!(result.is_err());
            assert_eq!(result.unwrap_err().code, DccErrorCode::Unsupported);
        }

        #[test]
        fn test_snapshot_not_connected() {
            let adapter = MockDccAdapter::new();
            let result = adapter.capture_viewport(None, None, None, "png");
            assert!(result.is_err());
        }
    }

    // ── DccAdapter trait tests ──

    mod adapter_trait {
        use super::*;

        #[test]
        fn test_capabilities() {
            let adapter = MockDccAdapter::new();
            let caps = adapter.capabilities();
            assert!(caps.scene_info);
            assert!(caps.snapshot);
            assert!(caps.selection);
            assert!(caps.file_operations);
        }

        #[test]
        fn test_as_connection() {
            let mut adapter = MockDccAdapter::new();
            assert!(adapter.as_connection().is_some());
        }

        #[test]
        fn test_as_script_engine() {
            let adapter = MockDccAdapter::new();
            assert!(adapter.as_script_engine().is_some());
        }

        #[test]
        fn test_as_scene_info() {
            let adapter = MockDccAdapter::new();
            assert!(adapter.as_scene_info().is_some());
        }

        #[test]
        fn test_as_snapshot_enabled() {
            let adapter = MockDccAdapter::new();
            assert!(adapter.as_snapshot().is_some());
        }

        #[test]
        fn test_as_snapshot_disabled() {
            let config = MockConfig::builder().snapshot_enabled(false).build();
            let adapter = MockDccAdapter::with_config(config);
            assert!(adapter.as_snapshot().is_none());
        }
    }

    // ── Counter tests ──

    mod counters {
        use super::*;

        #[test]
        fn test_invocation_counters() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();
            adapter.connect().unwrap(); // connect twice

            adapter
                .execute_script("a", ScriptLanguage::Python, None)
                .unwrap();
            adapter
                .execute_script("b", ScriptLanguage::Python, None)
                .unwrap();
            adapter
                .execute_script("c", ScriptLanguage::Python, None)
                .unwrap();

            adapter.get_scene_info().unwrap();
            adapter.capture_viewport(None, None, None, "png").unwrap();
            adapter.health_check().unwrap();

            assert_eq!(adapter.connect_count(), 2);
            assert_eq!(adapter.script_count(), 3);
            assert_eq!(adapter.scene_query_count(), 1);
            assert_eq!(adapter.snapshot_count(), 1);
            assert_eq!(adapter.health_check_count(), 1);
        }

        #[test]
        fn test_reset_counters() {
            let mut adapter = MockDccAdapter::new();
            adapter.connect().unwrap();
            adapter
                .execute_script("x", ScriptLanguage::Python, None)
                .unwrap();

            adapter.reset_counters();

            assert_eq!(adapter.connect_count(), 0);
            assert_eq!(adapter.script_count(), 0);
            assert_eq!(adapter.scene_query_count(), 0);
            assert_eq!(adapter.snapshot_count(), 0);
            assert_eq!(adapter.health_check_count(), 0);
            assert_eq!(adapter.disconnect_count(), 0);
        }
    }
}
