//! Mock DCC adapter struct and trait implementations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use crate::adapters::{
    CaptureResult, DccAdapter, DccCapabilities, DccConnection, DccError, DccErrorCode, DccInfo,
    DccResult, DccSceneInfo, DccScriptEngine, DccSnapshot, SceneInfo, SceneStatistics,
    ScriptLanguage, ScriptResult,
};

use super::config::{MockConfig, ScriptHandler};

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
