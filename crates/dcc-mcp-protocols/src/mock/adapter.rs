//! Mock DCC adapter struct and trait implementations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use crate::adapters::{
    BoundingBox, CaptureResult, DccAdapter, DccCapabilities, DccConnection, DccError, DccErrorCode,
    DccHierarchy, DccInfo, DccRenderCapture, DccResult, DccSceneInfo, DccSceneManager,
    DccScriptEngine, DccSnapshot, DccTransform, ObjectTransform, RenderOutput, SceneInfo,
    SceneNode, SceneObject, SceneStatistics, ScriptLanguage, ScriptResult,
};

use super::config::{MockConfig, ScriptHandler};

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
        self.require_connected("get_scene_info")?;
        Ok(self.scene.read().clone())
    }

    fn list_objects(&self) -> DccResult<Vec<(String, String)>> {
        self.scene_query_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("list_objects")?;

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
        self.require_connected("get_selection")?;
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
        self.require_connected("capture_viewport")?;

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

// ── DccSceneManager ──

impl DccSceneManager for MockDccAdapter {
    fn get_scene_info(&self) -> DccResult<SceneInfo> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_scene_info")?;
        Ok(self.scene.read().clone())
    }

    fn list_objects(&self, object_type: Option<&str>) -> DccResult<Vec<SceneObject>> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("list_objects")?;

        let objects = self.objects.read();
        let filtered = match object_type {
            None => objects.clone(),
            Some(ty) => objects
                .iter()
                .filter(|o| o.object_type == ty)
                .cloned()
                .collect(),
        };
        Ok(filtered)
    }

    fn new_scene(&self, _save_prompt: bool) -> DccResult<SceneInfo> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("new_scene")?;

        let new = SceneInfo {
            name: "untitled".to_string(),
            modified: false,
            ..Default::default()
        };
        *self.scene.write() = new.clone();
        self.objects.write().clear();
        self.selection.write().clear();
        Ok(new)
    }

    fn open_file(&self, file_path: &str, _force: bool) -> DccResult<SceneInfo> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("open_file")?;

        let name = file_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(file_path)
            .to_string();
        let info = SceneInfo {
            file_path: file_path.to_string(),
            name,
            modified: false,
            ..Default::default()
        };
        *self.scene.write() = info.clone();
        Ok(info)
    }

    fn save_file(&self, file_path: Option<&str>) -> DccResult<String> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("save_file")?;

        let path = file_path
            .map(String::from)
            .unwrap_or_else(|| self.scene.read().file_path.clone());
        self.scene.write().modified = false;
        Ok(path)
    }

    fn export_file(
        &self,
        file_path: &str,
        _format: &str,
        _selection_only: bool,
    ) -> DccResult<String> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("export_file")?;
        Ok(file_path.to_string())
    }

    fn get_selection(&self) -> DccResult<Vec<String>> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_selection")?;
        Ok(self.selection.read().clone())
    }

    fn set_selection(&self, object_names: &[&str]) -> DccResult<Vec<String>> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("set_selection")?;

        let names: Vec<String> = object_names.iter().map(|s| s.to_string()).collect();
        *self.selection.write() = names.clone();
        Ok(names)
    }

    fn select_by_type(&self, object_type: &str) -> DccResult<Vec<String>> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("select_by_type")?;

        let names: Vec<String> = self
            .objects
            .read()
            .iter()
            .filter(|o| o.object_type == object_type)
            .map(|o| o.name.clone())
            .collect();
        *self.selection.write() = names.clone();
        Ok(names)
    }

    fn set_visibility(&self, object_name: &str, visible: bool) -> DccResult<bool> {
        self.scene_manager_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("set_visibility")?;

        let mut objects = self.objects.write();
        for obj in objects.iter_mut() {
            if obj.name == object_name || obj.long_name == object_name {
                obj.visible = visible;
                return Ok(visible);
            }
        }
        Err(DccError {
            code: DccErrorCode::InvalidInput,
            message: format!("Object not found: {object_name}"),
            details: None,
            recoverable: false,
        })
    }
}

// ── DccTransform ──

impl DccTransform for MockDccAdapter {
    fn get_transform(&self, object_name: &str) -> DccResult<ObjectTransform> {
        self.transform_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_transform")?;

        let transforms = self.transforms.read();
        transforms
            .get(object_name)
            .cloned()
            .ok_or_else(|| DccError {
                code: DccErrorCode::InvalidInput,
                message: format!("Transform not found for: {object_name}"),
                details: None,
                recoverable: false,
            })
    }

    fn set_transform(
        &self,
        object_name: &str,
        translate: Option<[f64; 3]>,
        rotate: Option<[f64; 3]>,
        scale: Option<[f64; 3]>,
    ) -> DccResult<ObjectTransform> {
        self.transform_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("set_transform")?;

        let mut transforms = self.transforms.write();
        let entry = transforms
            .entry(object_name.to_string())
            .or_insert_with(ObjectTransform::identity);

        if let Some(t) = translate {
            entry.translate = t;
        }
        if let Some(r) = rotate {
            entry.rotate = r;
        }
        if let Some(s) = scale {
            entry.scale = s;
        }

        Ok(entry.clone())
    }

    fn get_bounding_box(&self, object_name: &str) -> DccResult<BoundingBox> {
        self.transform_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_bounding_box")?;

        let bbs = self.bounding_boxes.read();
        bbs.get(object_name).cloned().ok_or_else(|| DccError {
            code: DccErrorCode::InvalidInput,
            message: format!("Bounding box not found for: {object_name}"),
            details: None,
            recoverable: false,
        })
    }

    fn rename_object(&self, old_name: &str, new_name: &str) -> DccResult<String> {
        self.transform_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("rename_object")?;

        // Update objects list
        let mut objects = self.objects.write();
        let found = objects
            .iter_mut()
            .find(|o| o.name == old_name || o.long_name == old_name);

        match found {
            Some(obj) => {
                obj.name = new_name.to_string();
                Ok(new_name.to_string())
            }
            None => Err(DccError {
                code: DccErrorCode::InvalidInput,
                message: format!("Object not found: {old_name}"),
                details: None,
                recoverable: false,
            }),
        }
    }
}

// ── DccRenderCapture ──

impl DccRenderCapture for MockDccAdapter {
    fn capture_viewport(
        &self,
        viewport: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        format: &str,
    ) -> DccResult<CaptureResult> {
        self.render_capture_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("capture_viewport")?;

        if !self.snapshot_enabled {
            return Err(DccError {
                code: DccErrorCode::Unsupported,
                message: "Viewport capture not supported by this mock adapter".to_string(),
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

    fn render_scene(
        &self,
        output_path: &str,
        width: Option<u32>,
        height: Option<u32>,
        renderer: Option<&str>,
    ) -> DccResult<RenderOutput> {
        self.render_capture_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("render_scene")?;

        let settings = self.render_settings.read();
        let w = width.unwrap_or_else(|| {
            settings
                .get("width")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1920)
        });
        let h = height.unwrap_or_else(|| {
            settings
                .get("height")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1080)
        });
        let fmt = output_path.rsplit('.').next().unwrap_or("png").to_string();

        // If a renderer was requested but differs from settings, record it
        let _ = renderer;

        Ok(RenderOutput {
            file_path: output_path.to_string(),
            width: w,
            height: h,
            format: fmt,
            render_time_ms: self.render_time_ms,
        })
    }

    fn get_render_settings(&self) -> DccResult<HashMap<String, String>> {
        self.render_capture_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_render_settings")?;
        Ok(self.render_settings.read().clone())
    }

    fn set_render_settings(&self, settings: HashMap<String, String>) -> DccResult<()> {
        self.render_capture_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("set_render_settings")?;

        let mut current = self.render_settings.write();
        for (k, v) in settings {
            current.insert(k, v);
        }
        Ok(())
    }
}

// ── DccHierarchy ──

impl DccHierarchy for MockDccAdapter {
    fn get_hierarchy(&self) -> DccResult<Vec<SceneNode>> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_hierarchy")?;
        Ok(self.hierarchy.read().clone())
    }

    fn get_children(&self, object_name: Option<&str>) -> DccResult<Vec<SceneObject>> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_children")?;

        match object_name {
            // None = scene root children
            None => {
                let children = self
                    .hierarchy
                    .read()
                    .iter()
                    .map(|n| n.object.clone())
                    .collect();
                Ok(children)
            }
            Some(name) => {
                // BFS search for the named node
                let hierarchy = self.hierarchy.read();
                let children = find_children_in_tree(&hierarchy, name);
                Ok(children)
            }
        }
    }

    fn get_parent(&self, object_name: &str) -> DccResult<Option<String>> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_parent")?;

        let objects = self.objects.read();
        let obj = objects
            .iter()
            .find(|o| o.name == object_name || o.long_name == object_name);

        Ok(obj.and_then(|o| o.parent.clone()))
    }

    fn group_objects(
        &self,
        object_names: &[&str],
        group_name: &str,
        parent: Option<&str>,
    ) -> DccResult<SceneObject> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("group_objects")?;

        let group = SceneObject {
            name: group_name.to_string(),
            long_name: format!("|{group_name}"),
            object_type: "group".to_string(),
            parent: parent.map(String::from),
            visible: true,
            metadata: HashMap::from([("children".to_string(), object_names.join(","))]),
        };
        self.objects.write().push(group.clone());
        Ok(group)
    }

    fn ungroup(&self, group_name: &str) -> DccResult<Vec<String>> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("ungroup")?;

        let mut objects = self.objects.write();
        let pos = objects
            .iter()
            .position(|o| o.name == group_name || o.long_name == group_name);

        match pos {
            Some(idx) => {
                let group = objects.remove(idx);
                let children: Vec<String> = group
                    .metadata
                    .get("children")
                    .map(|s| s.split(',').map(String::from).collect())
                    .unwrap_or_default();
                Ok(children)
            }
            None => Err(DccError {
                code: DccErrorCode::InvalidInput,
                message: format!("Group not found: {group_name}"),
                details: None,
                recoverable: false,
            }),
        }
    }

    fn reparent(
        &self,
        object_name: &str,
        new_parent: Option<&str>,
        _preserve_world_transform: bool,
    ) -> DccResult<SceneObject> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("reparent")?;

        let mut objects = self.objects.write();
        let obj = objects
            .iter_mut()
            .find(|o| o.name == object_name || o.long_name == object_name);

        match obj {
            Some(o) => {
                o.parent = new_parent.map(String::from);
                Ok(o.clone())
            }
            None => Err(DccError {
                code: DccErrorCode::InvalidInput,
                message: format!("Object not found: {object_name}"),
                details: None,
                recoverable: false,
            }),
        }
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

    fn as_scene_manager(&self) -> Option<&dyn DccSceneManager> {
        Some(self)
    }

    fn as_transform(&self) -> Option<&dyn DccTransform> {
        Some(self)
    }

    fn as_render_capture(&self) -> Option<&dyn DccRenderCapture> {
        if self.snapshot_enabled {
            Some(self)
        } else {
            None
        }
    }

    fn as_hierarchy(&self) -> Option<&dyn DccHierarchy> {
        Some(self)
    }
}

// ── Helpers ──

/// BFS search for the immediate children of the named node in a scene tree.
fn find_children_in_tree(nodes: &[SceneNode], target_name: &str) -> Vec<SceneObject> {
    for node in nodes {
        if node.object.name == target_name || node.object.long_name == target_name {
            return node.children.iter().map(|c| c.object.clone()).collect();
        }
        let found = find_children_in_tree(&node.children, target_name);
        if !found.is_empty() {
            return found;
        }
    }
    vec![]
}
