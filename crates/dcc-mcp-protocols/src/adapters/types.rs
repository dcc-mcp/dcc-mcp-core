//! DCC adapter data types — shared data structures used by all DCC adapter traits.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Information about a DCC application instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DccInfo {
    /// DCC type identifier (e.g. "maya", "blender", "houdini", "3dsmax", "unreal", "unity").
    pub dcc_type: String,
    /// DCC application version string (e.g. "2024.2", "4.1.0").
    pub version: String,
    /// Python version available in this DCC (None for C#-only DCCs like Unity).
    pub python_version: Option<String>,
    /// Operating system (e.g. "windows", "linux", "macos").
    pub platform: String,
    /// Process ID of the DCC application.
    pub pid: u32,
    /// Arbitrary metadata (e.g. renderer, license type).
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Result of executing a script in a DCC application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptResult {
    /// Whether the script executed successfully.
    pub success: bool,
    /// Return value from the script (serialized as string).
    pub output: Option<String>,
    /// Error message if the script failed.
    pub error: Option<String>,
    /// Execution time in milliseconds.
    pub execution_time_ms: u64,
    /// Arbitrary context data from the execution.
    #[serde(default)]
    pub context: HashMap<String, String>,
}

impl ScriptResult {
    /// Create a successful script result.
    #[must_use]
    pub fn success(output: impl Into<String>, execution_time_ms: u64) -> Self {
        Self {
            success: true,
            output: Some(output.into()),
            error: None,
            execution_time_ms,
            context: HashMap::new(),
        }
    }

    /// Create a failed script result.
    #[must_use]
    pub fn failure(error: impl Into<String>, execution_time_ms: u64) -> Self {
        Self {
            success: false,
            output: None,
            error: Some(error.into()),
            execution_time_ms,
            context: HashMap::new(),
        }
    }
}

/// Supported script languages for DCC script execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptLanguage {
    /// Python script (supported by Maya, Blender, Houdini, 3ds Max, Nuke).
    Python,
    /// MEL script (Maya only).
    Mel,
    /// MaxScript (3ds Max only).
    MaxScript,
    /// HScript (Houdini only).
    HScript,
    /// VEX expressions (Houdini only).
    Vex,
    /// Lua script (some DCCs support Lua plugins).
    Lua,
    /// C# script (Unity only).
    CSharp,
    /// Blueprint/Visual Script (Unreal Engine).
    Blueprint,
}

impl std::fmt::Display for ScriptLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Python => write!(f, "python"),
            Self::Mel => write!(f, "mel"),
            Self::MaxScript => write!(f, "maxscript"),
            Self::HScript => write!(f, "hscript"),
            Self::Vex => write!(f, "vex"),
            Self::Lua => write!(f, "lua"),
            Self::CSharp => write!(f, "csharp"),
            Self::Blueprint => write!(f, "blueprint"),
        }
    }
}

/// Information about the currently open scene in a DCC application.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneInfo {
    /// Full file path of the scene (empty if untitled/new).
    pub file_path: String,
    /// Scene name (derived from file name or "untitled").
    pub name: String,
    /// Whether the scene has unsaved changes.
    pub modified: bool,
    /// Scene file format (e.g. ".ma", ".mb", ".blend", ".hip").
    pub format: String,
    /// Frame range: (start, end).
    pub frame_range: Option<(f64, f64)>,
    /// Current frame.
    pub current_frame: Option<f64>,
    /// Frames per second.
    pub fps: Option<f64>,
    /// Up axis ("y" or "z").
    pub up_axis: Option<String>,
    /// Unit system (e.g. "cm", "m", "inch").
    pub units: Option<String>,
    /// Scene statistics.
    #[serde(default)]
    pub statistics: SceneStatistics,
    /// Arbitrary scene metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Basic scene statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SceneStatistics {
    /// Number of objects/nodes in the scene.
    pub object_count: u64,
    /// Total vertex count across all geometry.
    pub vertex_count: u64,
    /// Total polygon/face count across all geometry.
    pub polygon_count: u64,
    /// Number of materials/shaders.
    pub material_count: u64,
    /// Number of texture files referenced.
    pub texture_count: u64,
    /// Number of lights in the scene.
    pub light_count: u64,
    /// Number of cameras in the scene.
    pub camera_count: u64,
}

/// Captured screenshot/viewport image data.
#[derive(Debug, Clone)]
pub struct CaptureResult {
    /// Raw image data (PNG, JPEG, or WebP encoded).
    pub data: Vec<u8>,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Image format (e.g. "png", "jpeg", "webp").
    pub format: String,
    /// Which viewport/panel was captured (e.g. "persp", "top", "modelPanel4").
    pub viewport: Option<String>,
}

/// Capabilities that a DCC adapter supports.
///
/// Used for feature negotiation — the MCP server can query which operations
/// are available for a given DCC and adapt its tool offerings accordingly.
///
/// # Bridge Mode
///
/// DCCs without an embedded Python interpreter (ZBrush, Photoshop) use a
/// *bridge* process rather than direct subprocess execution:
/// - Set `has_embedded_python = false`
/// - Set the appropriate `bridge_kind` variant
/// - Set `bridge_endpoint` to the address/URL the bridge listens on
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DccCapabilities {
    /// Supported script languages.
    pub script_languages: Vec<ScriptLanguage>,
    /// Whether the adapter supports scene info queries.
    pub scene_info: bool,
    /// Whether the adapter supports viewport capture/screenshots.
    pub snapshot: bool,
    /// Whether the adapter supports undo/redo operations.
    pub undo_redo: bool,
    /// Whether the adapter supports progress reporting.
    pub progress_reporting: bool,
    /// Whether the adapter supports file open/save/export.
    pub file_operations: bool,
    /// Whether the adapter supports selection queries/manipulation.
    pub selection: bool,
    /// Whether the adapter implements [`DccSceneManager`] (scene/file management).
    pub scene_manager: bool,
    /// Whether the adapter implements [`DccTransform`] (object TRS transforms).
    pub transform: bool,
    /// Whether the adapter implements [`DccRenderCapture`] (viewport capture + render).
    pub render_capture: bool,
    /// Whether the adapter implements [`DccHierarchy`] (parent/child hierarchy).
    pub hierarchy: bool,
    /// Whether the DCC has an embedded Python interpreter.
    ///
    /// `true` for Maya, Blender, Houdini, Unreal, Godot, etc.
    /// `false` for ZBrush, Photoshop (require bridge process).
    #[serde(default = "default_true")]
    pub has_embedded_python: bool,
    /// Communication bridge kind used when `has_embedded_python` is `false`.
    ///
    /// `None` for Python-embedded DCCs; set to the appropriate variant for
    /// bridge-based adapters.
    #[serde(default)]
    pub bridge_kind: Option<BridgeKind>,
    /// Bridge endpoint address (URL or socket path).
    ///
    /// For `BridgeKind::Http`: `"http://localhost:8765"`
    /// For `BridgeKind::WebSocket`: `"ws://localhost:3000"`
    /// For `BridgeKind::NamedPipe`: `"\\\\.\\pipe\\zbrush-mcp"`
    #[serde(default)]
    pub bridge_endpoint: Option<String>,
    /// Additional capability flags.
    #[serde(default)]
    pub extensions: HashMap<String, bool>,
}

fn default_true() -> bool {
    true
}

/// The communication bridge kind for DCCs without embedded Python.
///
/// Determines how `dcc-mcp-core` routes `tools/call` requests to the DCC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeKind {
    /// HTTP REST bridge.
    ///
    /// Used by ZBrush 2024+ (built-in HTTP server on a configurable port).
    /// The bridge process translates MCP tool calls into ZBrush HTTP API requests.
    Http,
    /// WebSocket bridge.
    ///
    /// Used by Photoshop (UXP plugin opens a WebSocket server).
    /// The bridge process connects to the UXP WebSocket and forwards calls.
    WebSocket,
    /// Named-pipe / Unix-socket bridge.
    ///
    /// Used by applications that expose an IPC pipe (e.g. 3ds Max COM + pipe).
    NamedPipe,
    /// Custom/unknown bridge — check `extensions` for details.
    Custom(String),
}

impl DccCapabilities {
    /// Create capabilities for a standard Python-embedded DCC (Maya, Blender, Unreal…).
    #[must_use]
    pub fn python_embedded() -> Self {
        Self {
            has_embedded_python: true,
            bridge_kind: None,
            bridge_endpoint: None,
            ..Default::default()
        }
    }

    /// Create capabilities for an HTTP-bridge DCC (ZBrush).
    #[must_use]
    pub fn http_bridge(endpoint: impl Into<String>) -> Self {
        Self {
            has_embedded_python: false,
            bridge_kind: Some(BridgeKind::Http),
            bridge_endpoint: Some(endpoint.into()),
            ..Default::default()
        }
    }

    /// Create capabilities for a WebSocket-bridge DCC (Photoshop UXP).
    #[must_use]
    pub fn websocket_bridge(endpoint: impl Into<String>) -> Self {
        Self {
            has_embedded_python: false,
            bridge_kind: Some(BridgeKind::WebSocket),
            bridge_endpoint: Some(endpoint.into()),
            ..Default::default()
        }
    }

    /// Return `true` if this DCC uses a bridge process rather than a subprocess.
    #[must_use]
    pub fn uses_bridge(&self) -> bool {
        self.bridge_kind.is_some()
    }
}

/// Error type for DCC adapter operations.
///
/// Wraps the various errors that can occur during DCC communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DccError {
    /// Error code for programmatic handling.
    pub code: DccErrorCode,
    /// Human-readable error message.
    pub message: String,
    /// Optional detailed error information (e.g. traceback).
    pub details: Option<String>,
    /// Whether the error is recoverable (transient).
    pub recoverable: bool,
}

impl std::fmt::Display for DccError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for DccError {}

/// Error codes for DCC adapter operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DccErrorCode {
    /// Connection to DCC failed or was lost.
    ConnectionFailed,
    /// Connection timed out.
    Timeout,
    /// Script execution failed.
    ScriptError,
    /// DCC is not responding (frozen/busy).
    NotResponding,
    /// Requested operation is not supported by this DCC.
    Unsupported,
    /// Permission denied (e.g. sandbox restriction).
    PermissionDenied,
    /// Invalid input parameters.
    InvalidInput,
    /// Scene operation failed (e.g. file not found, save failed).
    SceneError,
    /// Internal error in the adapter.
    Internal,
}

impl std::fmt::Display for DccErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed => write!(f, "CONNECTION_FAILED"),
            Self::Timeout => write!(f, "TIMEOUT"),
            Self::ScriptError => write!(f, "SCRIPT_ERROR"),
            Self::NotResponding => write!(f, "NOT_RESPONDING"),
            Self::Unsupported => write!(f, "UNSUPPORTED"),
            Self::PermissionDenied => write!(f, "PERMISSION_DENIED"),
            Self::InvalidInput => write!(f, "INVALID_INPUT"),
            Self::SceneError => write!(f, "SCENE_ERROR"),
            Self::Internal => write!(f, "INTERNAL"),
        }
    }
}

/// Result type alias for DCC adapter operations.
pub type DccResult<T> = Result<T, DccError>;

// ── Cross-DCC Protocol Data Models ──

/// 3D transform with translation, rotation (Euler XYZ, degrees), and scale.
///
/// Coordinate convention: right-hand Y-up world space, centimeter units.
/// Adapters are responsible for converting from their native coordinate system.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectTransform {
    /// World-space translation [x, y, z] in centimeters.
    pub translate: [f64; 3],
    /// Euler XYZ rotation in degrees [rx, ry, rz].
    pub rotate: [f64; 3],
    /// Non-uniform scale [sx, sy, sz].
    pub scale: [f64; 3],
}

impl ObjectTransform {
    /// Create a transform at the origin (identity).
    #[must_use]
    pub fn identity() -> Self {
        Self {
            translate: [0.0, 0.0, 0.0],
            rotate: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Minimum corner [x, y, z] in world space (centimeters).
    pub min: [f64; 3],
    /// Maximum corner [x, y, z] in world space (centimeters).
    pub max: [f64; 3],
}

impl BoundingBox {
    /// Compute the center of the bounding box.
    #[must_use]
    pub fn center(&self) -> [f64; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// Compute the size (extents) [width, height, depth].
    #[must_use]
    pub fn size(&self) -> [f64; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }
}

/// Lightweight description of a scene object.
///
/// Applies to all DCC tools: Maya DAG nodes, Blender objects, UE actors,
/// Unity GameObjects, Photoshop layers, Figma nodes, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneObject {
    /// Short name (leaf node name, without path separators).
    pub name: String,
    /// Full path or unique identifier within the scene.
    ///
    /// - Maya: `|group1|pCube1`
    /// - Blender: `Mesh.001`
    /// - UE: actor GUID string
    /// - Photoshop: layer name or index path
    pub long_name: String,
    /// Object type string (DCC-specific, e.g. "mesh", "transform", "light", "camera", "layer").
    pub object_type: String,
    /// Full path of the parent object, or `None` if at the scene root.
    pub parent: Option<String>,
    /// Whether the object is currently visible.
    pub visible: bool,
    /// Arbitrary extra metadata (e.g. material name, layer ID).
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Node in the scene hierarchy tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneNode {
    /// The scene object at this node.
    pub object: SceneObject,
    /// Immediate children of this node.
    pub children: Vec<SceneNode>,
}

/// Frame range and timing information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrameRange {
    /// First frame (inclusive).
    pub start: f64,
    /// Last frame (inclusive).
    pub end: f64,
    /// Frames per second.
    pub fps: f64,
    /// Currently active frame.
    pub current: f64,
}

/// Render output configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderOutput {
    /// Output file path (absolute).
    pub file_path: String,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// File format (e.g. "png", "exr", "jpg").
    pub format: String,
    /// Time taken to render in milliseconds.
    pub render_time_ms: u64,
}
