//! DCC adapter traits — unified interface for all DCC application integrations.
//!
//! These traits define the standard contracts that all DCC adapters must implement.
//! Upper-layer projects (dcc-mcp-maya, dcc-mcp-blender, etc.) only need to implement
//! these traits; all common behavior is handled by the framework.
//!
//! # Architecture
//!
//! ```text
//! DccAdapter              — Top-level trait: connect, disconnect, execute, capabilities
//!   ├── DccConnection         — Connection lifecycle: connect, disconnect, health check
//!   ├── DccScriptEngine       — Script execution: run Python/MEL/MaxScript in DCC
//!   ├── DccSceneInfo          — Scene inspection: file path, modified state, statistics
//!   └── DccSnapshot           — Screenshot/viewport capture
//!
//! Cross-DCC Protocol Traits (universally applicable across Maya/Blender/3dsMax/UE/Unity/Photoshop)
//!   ├── DccSceneManager       — Scene & file management: open/save/export/list objects/selection
//!   ├── DccTransform          — Object transform (TRS): get/set translate/rotate/scale + hierarchy
//!   ├── DccRenderCapture      — Viewport capture & render output
//!   └── DccHierarchy          — Object hierarchy & grouping queries
//! ```
//!
//! # Design Principles
//!
//! - **Synchronous by default**: DCC main threads (Maya, Blender, 3ds Max) typically
//!   do not support async event loops. All trait methods are synchronous.
//! - **No external dependencies**: Traits use only `std` types and types from this crate.
//! - **Error-agnostic**: Uses `Box<dyn Error>` so each DCC adapter can bring its own
//!   error types without forcing a single error enum.
//! - **Optional sub-traits**: Adapters can implement only the sub-traits they support.
//!   For example, a headless DCC might skip `DccSnapshot`.
//! - **Coordinate convention**: All transforms use right-hand Y-up world space, Euler XYZ
//!   angles in degrees, and centimeter units. Adapters handle internal conversion.

use std::collections::HashMap;

use super::types::{
    BoundingBox, CaptureResult, DccCapabilities, DccInfo, DccResult, ObjectTransform, RenderOutput,
    SceneInfo, SceneNode, SceneObject, ScriptLanguage, ScriptResult,
};

// ── Core Traits ──

/// Connection lifecycle management for a DCC instance.
///
/// Implementations handle the low-level connection setup, teardown, and health monitoring.
pub trait DccConnection: Send + Sync {
    /// Establish a connection to the DCC application.
    fn connect(&mut self) -> DccResult<()>;

    /// Disconnect from the DCC application.
    fn disconnect(&mut self) -> DccResult<()>;

    /// Check if the connection is currently alive and responsive.
    fn is_connected(&self) -> bool;

    /// Perform a health check (ping). Returns round-trip time in milliseconds.
    fn health_check(&self) -> DccResult<u64>;
}

/// Script execution engine for running code inside a DCC application.
///
/// The primary way MCP tools interact with DCCs — by sending script code
/// to be executed in the DCC's scripting environment.
pub trait DccScriptEngine: Send + Sync {
    /// Execute a script in the DCC application.
    ///
    /// # Arguments
    /// * `code` — The script source code to execute.
    /// * `language` — The scripting language to use.
    /// * `timeout_ms` — Optional execution timeout in milliseconds.
    fn execute_script(
        &self,
        code: &str,
        language: ScriptLanguage,
        timeout_ms: Option<u64>,
    ) -> DccResult<ScriptResult>;

    /// Get the list of supported script languages for this DCC.
    fn supported_languages(&self) -> Vec<ScriptLanguage>;
}

/// Scene information queries.
///
/// Provides read-only access to the current scene state in the DCC application.
pub trait DccSceneInfo: Send + Sync {
    /// Get information about the currently open scene.
    fn get_scene_info(&self) -> DccResult<SceneInfo>;

    /// Get the list of all objects/nodes in the scene.
    ///
    /// Returns a list of (name, type) pairs.
    fn list_objects(&self) -> DccResult<Vec<(String, String)>>;

    /// Get the currently selected objects.
    fn get_selection(&self) -> DccResult<Vec<String>>;
}

/// Viewport/screenshot capture.
///
/// Captures the DCC viewport as an image. Implementations may use:
/// - DCC API (e.g. `cmds.playblast` in Maya)
/// - GPU frame buffer capture (OS-level, for any DCC)
/// - Remote rendering API (e.g. Unreal Remote Control)
pub trait DccSnapshot: Send + Sync {
    /// Capture a screenshot of the specified viewport.
    ///
    /// # Arguments
    /// * `viewport` — Which viewport to capture (None for the active/default viewport).
    /// * `width` — Desired image width (None for native resolution).
    /// * `height` — Desired image height (None for native resolution).
    /// * `format` — Image format: "png", "jpeg", or "webp".
    fn capture_viewport(
        &self,
        viewport: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        format: &str,
    ) -> DccResult<CaptureResult>;
}

// ── Cross-DCC Protocol Traits ──

/// **SceneManagerTrait** — Universal scene and file management.
///
/// Covers the operations common to all DCC tools and creative applications:
/// Maya, Blender, 3dsMax, Unreal Engine, Unity, Photoshop, Figma.
///
/// # Implementation notes
/// - `list_objects`: for 2D tools (Photoshop/Figma), this lists layers/nodes.
/// - `open_file` / `save_file`: for Figma (read-only API), these may return `Unsupported`.
/// - Selection is object-level (not component-level) to keep the interface minimal.
pub trait DccSceneManager: Send + Sync {
    /// Get high-level information about the current scene/document.
    ///
    /// Returns metadata including file path, frame range, coordinate system, etc.
    fn get_scene_info(&self) -> DccResult<SceneInfo>;

    /// List all top-level and descendant objects/layers in the scene.
    ///
    /// # Arguments
    /// * `object_type` — Optional filter (e.g. "mesh", "light", "camera").
    ///   Pass `None` to list all objects.
    fn list_objects(&self, object_type: Option<&str>) -> DccResult<Vec<SceneObject>>;

    /// Create a new empty scene/document.
    ///
    /// # Arguments
    /// * `save_prompt` — If `true`, prompt the user to save unsaved changes first.
    fn new_scene(&self, save_prompt: bool) -> DccResult<SceneInfo>;

    /// Open a scene/document from disk.
    ///
    /// # Arguments
    /// * `file_path` — Absolute path to the scene file.
    /// * `force` — If `true`, discard unsaved changes without prompting.
    fn open_file(&self, file_path: &str, force: bool) -> DccResult<SceneInfo>;

    /// Save the current scene/document.
    ///
    /// # Arguments
    /// * `file_path` — Destination path. Pass `None` to save in place.
    fn save_file(&self, file_path: Option<&str>) -> DccResult<String>;

    /// Export the scene or a selection to a file.
    ///
    /// # Arguments
    /// * `file_path` — Output file path.
    /// * `format` — Export format (e.g. "fbx", "obj", "usd", "png", "svg").
    /// * `selection_only` — If `true`, export only selected objects/layers.
    fn export_file(&self, file_path: &str, format: &str, selection_only: bool)
    -> DccResult<String>;

    /// Get the names of currently selected objects/layers.
    fn get_selection(&self) -> DccResult<Vec<String>>;

    /// Replace the current selection.
    ///
    /// # Arguments
    /// * `object_names` — List of object names (short or long) to select.
    fn set_selection(&self, object_names: &[&str]) -> DccResult<Vec<String>>;

    /// Select all objects of a given type.
    ///
    /// # Arguments
    /// * `object_type` — Type string (e.g. "mesh", "camera", "light").
    fn select_by_type(&self, object_type: &str) -> DccResult<Vec<String>>;

    /// Set the visibility of an object/layer.
    ///
    /// # Arguments
    /// * `object_name` — Short or long name of the object.
    /// * `visible` — Target visibility state.
    fn set_visibility(&self, object_name: &str, visible: bool) -> DccResult<bool>;
}

/// **DccTransformTrait** — Universal object transform (TRS) interface.
///
/// Handles translate / rotate / scale queries and mutations for 3D objects
/// in Maya, Blender, 3dsMax, Unreal Engine, and Unity, as well as 2D position
/// and rotation for Photoshop layers and Figma nodes.
///
/// # Coordinate convention
/// - All translations are in **centimeters** in world space.
/// - All rotations are **Euler XYZ in degrees**.
/// - Adapters must convert from their native representation (e.g. Blender's
///   Z-up radians, Unreal's centimeter Z-up, Photoshop's pixel origin).
pub trait DccTransform: Send + Sync {
    /// Get the world-space transform of an object.
    ///
    /// # Arguments
    /// * `object_name` — Short or long name of the object.
    fn get_transform(&self, object_name: &str) -> DccResult<ObjectTransform>;

    /// Set the world-space transform of an object.
    ///
    /// `None` fields leave the corresponding component unchanged.
    ///
    /// # Arguments
    /// * `object_name` — Short or long name of the object.
    /// * `translate` — New translation [x, y, z] in centimeters, or `None`.
    /// * `rotate` — New Euler XYZ rotation in degrees, or `None`.
    /// * `scale` — New scale [sx, sy, sz], or `None`.
    fn set_transform(
        &self,
        object_name: &str,
        translate: Option<[f64; 3]>,
        rotate: Option<[f64; 3]>,
        scale: Option<[f64; 3]>,
    ) -> DccResult<ObjectTransform>;

    /// Get the axis-aligned world-space bounding box of an object.
    ///
    /// # Arguments
    /// * `object_name` — Short or long name of the object.
    fn get_bounding_box(&self, object_name: &str) -> DccResult<BoundingBox>;

    /// Rename an object/layer.
    ///
    /// # Arguments
    /// * `old_name` — Current short or long name.
    /// * `new_name` — Desired new name (short name, without path).
    fn rename_object(&self, old_name: &str, new_name: &str) -> DccResult<String>;
}

/// **DccRenderCaptureTrait** — Universal viewport capture and render output.
///
/// Covers screenshot / playblast (Maya), viewport render (Blender),
/// high-res render (UE/Unity), and document export (Photoshop/Figma).
pub trait DccRenderCapture: Send + Sync {
    /// Capture a screenshot of the active (or specified) viewport.
    ///
    /// # Arguments
    /// * `viewport` — Which viewport to capture (e.g. "persp", "top"). `None` = active viewport.
    /// * `width` — Image width in pixels. `None` = native/current resolution.
    /// * `height` — Image height in pixels. `None` = native/current resolution.
    /// * `format` — Output format: "png", "jpeg", or "webp".
    fn capture_viewport(
        &self,
        viewport: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        format: &str,
    ) -> DccResult<CaptureResult>;

    /// Render the scene and write output to disk.
    ///
    /// This is a potentially long-running operation. Adapters may implement
    /// a timeout via the DCC's own render API.
    ///
    /// # Arguments
    /// * `output_path` — Destination file path.
    /// * `width` — Render width in pixels. `None` = use current render settings.
    /// * `height` — Render height in pixels. `None` = use current render settings.
    /// * `renderer` — Renderer name (e.g. "arnold", "cycles", "eevee"). `None` = default.
    fn render_scene(
        &self,
        output_path: &str,
        width: Option<u32>,
        height: Option<u32>,
        renderer: Option<&str>,
    ) -> DccResult<RenderOutput>;

    /// Get the current render settings (width, height, renderer, sample count, etc.).
    fn get_render_settings(&self) -> DccResult<HashMap<String, String>>;

    /// Update one or more render settings.
    ///
    /// # Arguments
    /// * `settings` — Key-value pairs to update (e.g. `{"width": "1920", "renderer": "arnold"}`).
    fn set_render_settings(&self, settings: HashMap<String, String>) -> DccResult<()>;
}

/// **DccHierarchyTrait** — Universal scene hierarchy and grouping.
///
/// Provides read and write access to the parent-child object graph that all
/// DCC tools share (Maya DAG, Blender collection tree, UE level hierarchy,
/// Unity scene graph, Photoshop layer groups, Figma frames/groups).
pub trait DccHierarchy: Send + Sync {
    /// Get the full scene hierarchy as a tree.
    ///
    /// Returns only root-level nodes; each `SceneNode` contains its children.
    fn get_hierarchy(&self) -> DccResult<Vec<SceneNode>>;

    /// Get the immediate children of an object.
    ///
    /// # Arguments
    /// * `object_name` — Short or long name of the parent object. `None` = scene root.
    fn get_children(&self, object_name: Option<&str>) -> DccResult<Vec<SceneObject>>;

    /// Get the parent of an object.
    ///
    /// Returns `Ok(None)` when the object is at the scene root.
    ///
    /// # Arguments
    /// * `object_name` — Short or long name of the child object.
    fn get_parent(&self, object_name: &str) -> DccResult<Option<String>>;

    /// Group a set of objects under a new named group/null/empty object.
    ///
    /// # Arguments
    /// * `object_names` — Objects to group (short or long names).
    /// * `group_name` — Name for the newly created group.
    /// * `parent` — Parent for the new group. `None` = scene root.
    fn group_objects(
        &self,
        object_names: &[&str],
        group_name: &str,
        parent: Option<&str>,
    ) -> DccResult<SceneObject>;

    /// Ungroup a group/container, moving its children to the group's parent.
    ///
    /// # Arguments
    /// * `group_name` — Short or long name of the group to dissolve.
    fn ungroup(&self, group_name: &str) -> DccResult<Vec<String>>;

    /// Reparent an object (change its parent in the hierarchy).
    ///
    /// # Arguments
    /// * `object_name` — Object to reparent (short or long name).
    /// * `new_parent` — New parent name. `None` = move to scene root.
    /// * `preserve_world_transform` — If `true`, adjust local transform to keep world position.
    fn reparent(
        &self,
        object_name: &str,
        new_parent: Option<&str>,
        preserve_world_transform: bool,
    ) -> DccResult<SceneObject>;
}

/// Top-level DCC adapter combining all sub-traits.
///
/// This is the primary interface that DCC integration projects implement.
/// Not all sub-traits are required — use the `capabilities()` method to
/// advertise which features are available.
///
/// In addition to the original four sub-traits, adapters can optionally expose
/// the four cross-DCC protocol traits:
/// - `DccSceneManager` — scene/file management, selection, visibility
/// - `DccTransform` — object TRS transforms and bounding boxes
/// - `DccRenderCapture` — viewport capture and scene rendering
/// - `DccHierarchy` — parent/child hierarchy and grouping
///
/// # Example
///
/// ```rust
/// use dcc_mcp_protocols::adapters::*;
///
/// struct MockAdapter {
///     info: DccInfo,
/// }
///
/// impl DccAdapter for MockAdapter {
///     fn info(&self) -> &DccInfo { &self.info }
///
///     fn capabilities(&self) -> DccCapabilities {
///         DccCapabilities {
///             script_languages: vec![ScriptLanguage::Python],
///             scene_info: true,
///             snapshot: false,
///             ..Default::default()
///         }
///     }
///
///     fn as_connection(&mut self) -> Option<&mut dyn DccConnection> { None }
///     fn as_script_engine(&self) -> Option<&dyn DccScriptEngine> { None }
///     fn as_scene_info(&self) -> Option<&dyn DccSceneInfo> { None }
///     fn as_snapshot(&self) -> Option<&dyn DccSnapshot> { None }
///
///     // Cross-DCC protocol traits — all optional, return None by default
///     fn as_scene_manager(&self) -> Option<&dyn DccSceneManager> { None }
///     fn as_transform(&self) -> Option<&dyn DccTransform> { None }
///     fn as_render_capture(&self) -> Option<&dyn DccRenderCapture> { None }
///     fn as_hierarchy(&self) -> Option<&dyn DccHierarchy> { None }
/// }
/// ```
pub trait DccAdapter: Send + Sync {
    /// Get static information about this DCC instance.
    fn info(&self) -> &DccInfo;

    /// Get the capabilities of this adapter.
    fn capabilities(&self) -> DccCapabilities;

    /// Access the connection management interface.
    fn as_connection(&mut self) -> Option<&mut dyn DccConnection>;

    /// Access the script execution interface.
    fn as_script_engine(&self) -> Option<&dyn DccScriptEngine>;

    /// Access the scene info query interface.
    fn as_scene_info(&self) -> Option<&dyn DccSceneInfo>;

    /// Access the snapshot/capture interface.
    fn as_snapshot(&self) -> Option<&dyn DccSnapshot>;

    // ── Cross-DCC Protocol Accessors (optional, default to None) ──

    /// Access the universal scene & file management interface.
    ///
    /// Supported by: Maya, Blender, 3dsMax, Unreal Engine, Unity, Photoshop, Figma.
    fn as_scene_manager(&self) -> Option<&dyn DccSceneManager> {
        None
    }

    /// Access the universal object transform (TRS) interface.
    ///
    /// Supported by: Maya, Blender, 3dsMax, Unreal Engine, Unity, Photoshop (layers), Figma (nodes).
    fn as_transform(&self) -> Option<&dyn DccTransform> {
        None
    }

    /// Access the universal viewport capture and render output interface.
    ///
    /// Supported by: Maya, Blender, 3dsMax, Unreal Engine, Unity, Photoshop.
    fn as_render_capture(&self) -> Option<&dyn DccRenderCapture> {
        None
    }

    /// Access the universal scene hierarchy and grouping interface.
    ///
    /// Supported by: Maya (DAG), Blender (collections), Unreal (level hierarchy),
    /// Unity (scene graph), Photoshop (layer groups), Figma (frames/groups).
    fn as_hierarchy(&self) -> Option<&dyn DccHierarchy> {
        None
    }
}
