//! Mock DCC adapter struct and shared helpers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::adapters::{
    BoundingBox, DccCapabilities, DccError, DccErrorCode, DccInfo, DccResult, ObjectTransform,
    SceneInfo, SceneNode, SceneObject, SceneStatistics,
};

use super::config::{MockConfig, ScriptHandler};

#[path = "adapter_connection.rs"]
mod connection_impl;
#[path = "adapter_hierarchy.rs"]
mod hierarchy_impl;
#[path = "adapter_scene_manager.rs"]
mod scene_manager_impl;
#[path = "adapter_script_scene.rs"]
mod script_scene_impl;
#[path = "adapter_snapshot_render.rs"]
mod snapshot_render_impl;
#[path = "adapter_transform.rs"]
mod transform_impl;

/// A fully functional mock DCC adapter for testing and development.
///
/// Implements all DCC adapter traits with configurable behavior:
/// - Connection: tracks connected/disconnected state
/// - Script execution: echo-back or custom handler
/// - Scene info: configurable scene with mutable statistics
/// - Snapshot: returns configurable image data
/// - SceneManager: list/select/visibility with in-memory object store
/// - Transform: get/set per-object TRS with in-memory transform map
/// - RenderCapture: returns mock capture data and records render calls
/// - Hierarchy: returns configurable scene node tree
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

    // Cross-DCC protocol state
    pub(crate) objects: parking_lot::RwLock<Vec<SceneObject>>,
    pub(crate) selection: parking_lot::RwLock<Vec<String>>,
    pub(crate) hierarchy: parking_lot::RwLock<Vec<SceneNode>>,
    pub(crate) transforms: parking_lot::RwLock<HashMap<String, ObjectTransform>>,
    pub(crate) bounding_boxes: parking_lot::RwLock<HashMap<String, BoundingBox>>,
    render_time_ms: u64,
    render_settings: parking_lot::RwLock<HashMap<String, String>>,

    // Invocation counters
    connect_count: AtomicU64,
    disconnect_count: AtomicU64,
    script_count: AtomicU64,
    scene_query_count: AtomicU64,
    snapshot_count: AtomicU64,
    health_check_count: AtomicU64,
    scene_manager_count: AtomicU64,
    transform_count: AtomicU64,
    render_capture_count: AtomicU64,
    hierarchy_count: AtomicU64,
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
            scene_manager: true,
            transform: true,
            render_capture: config.snapshot_enabled,
            hierarchy: true,
            extensions: HashMap::new(),
            ..Default::default()
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
            objects: parking_lot::RwLock::new(config.objects),
            selection: parking_lot::RwLock::new(config.selection),
            hierarchy: parking_lot::RwLock::new(config.hierarchy),
            transforms: parking_lot::RwLock::new(config.transforms),
            bounding_boxes: parking_lot::RwLock::new(config.bounding_boxes),
            render_time_ms: config.render_time_ms,
            render_settings: parking_lot::RwLock::new(config.render_settings),
            connect_count: AtomicU64::new(0),
            disconnect_count: AtomicU64::new(0),
            script_count: AtomicU64::new(0),
            scene_query_count: AtomicU64::new(0),
            snapshot_count: AtomicU64::new(0),
            health_check_count: AtomicU64::new(0),
            scene_manager_count: AtomicU64::new(0),
            transform_count: AtomicU64::new(0),
            render_capture_count: AtomicU64::new(0),
            hierarchy_count: AtomicU64::new(0),
        }
    }

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

    /// Number of times scene info was queried (DccSceneInfo).
    pub fn scene_query_count(&self) -> u64 {
        self.scene_query_count.load(Ordering::Relaxed)
    }

    /// Number of times `capture_viewport()` was called (DccSnapshot).
    pub fn snapshot_count(&self) -> u64 {
        self.snapshot_count.load(Ordering::Relaxed)
    }

    /// Number of times `health_check()` was called.
    pub fn health_check_count(&self) -> u64 {
        self.health_check_count.load(Ordering::Relaxed)
    }

    /// Number of times any DccSceneManager method was called.
    pub fn scene_manager_count(&self) -> u64 {
        self.scene_manager_count.load(Ordering::Relaxed)
    }

    /// Number of times any DccTransform method was called.
    pub fn transform_count(&self) -> u64 {
        self.transform_count.load(Ordering::Relaxed)
    }

    /// Number of times any DccRenderCapture method was called.
    pub fn render_capture_count(&self) -> u64 {
        self.render_capture_count.load(Ordering::Relaxed)
    }

    /// Number of times any DccHierarchy method was called.
    pub fn hierarchy_count(&self) -> u64 {
        self.hierarchy_count.load(Ordering::Relaxed)
    }

    /// Reset all invocation counters.
    pub fn reset_counters(&self) {
        self.connect_count.store(0, Ordering::Relaxed);
        self.disconnect_count.store(0, Ordering::Relaxed);
        self.script_count.store(0, Ordering::Relaxed);
        self.scene_query_count.store(0, Ordering::Relaxed);
        self.snapshot_count.store(0, Ordering::Relaxed);
        self.health_check_count.store(0, Ordering::Relaxed);
        self.scene_manager_count.store(0, Ordering::Relaxed);
        self.transform_count.store(0, Ordering::Relaxed);
        self.render_capture_count.store(0, Ordering::Relaxed);
        self.hierarchy_count.store(0, Ordering::Relaxed);
    }

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

    /// Replace the in-memory object list.
    pub fn set_objects(&self, objects: Vec<SceneObject>) {
        *self.objects.write() = objects;
    }

    /// Register a transform for a named object.
    pub fn register_transform(&self, name: impl Into<String>, transform: ObjectTransform) {
        self.transforms.write().insert(name.into(), transform);
    }

    /// Register a bounding box for a named object.
    pub fn register_bounding_box(&self, name: impl Into<String>, bb: BoundingBox) {
        self.bounding_boxes.write().insert(name.into(), bb);
    }

    /// Helper: require connection, return error if not connected.
    fn require_connected(&self, op: &str) -> DccResult<()> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(DccError {
                code: DccErrorCode::ConnectionFailed,
                message: format!("Not connected — call connect() before {op}"),
                details: None,
                recoverable: true,
            });
        }
        Ok(())
    }
}

impl Default for MockDccAdapter {
    fn default() -> Self {
        Self::new()
    }
}
