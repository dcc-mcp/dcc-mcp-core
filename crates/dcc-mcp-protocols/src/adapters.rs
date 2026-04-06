//! DCC adapter traits — unified interface for all DCC application integrations.
//!
//! These traits define the standard contracts that all DCC adapters must implement.
//! Upper-layer projects (dcc-mcp-maya, dcc-mcp-blender, etc.) only need to implement
//! these traits; all common behavior is handled by the framework.
//!
//! # Architecture
//!
//! ```text
//! DccAdapter         — Top-level trait: connect, disconnect, execute, capabilities
//!   ├── DccConnection    — Connection lifecycle: connect, disconnect, health check
//!   ├── DccScriptEngine  — Script execution: run Python/MEL/MaxScript in DCC
//!   ├── DccSceneInfo     — Scene inspection: file path, modified state, statistics
//!   └── DccSnapshot      — Screenshot/viewport capture
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
    /// Additional capability flags.
    #[serde(default)]
    pub extensions: HashMap<String, bool>,
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

/// Top-level DCC adapter combining all sub-traits.
///
/// This is the primary interface that DCC integration projects implement.
/// Not all sub-traits are required — use the `capabilities()` method to
/// advertise which features are available.
///
/// # Example
///
/// ```rust
/// use dcc_mcp_protocols::adapters::*;
///
/// struct MockAdapter {
///     connected: bool,
///     info: DccInfo,
/// }
///
/// impl DccAdapter for MockAdapter {
///     fn info(&self) -> &DccInfo {
///         &self.info
///     }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Data structure tests ──

    #[test]
    fn test_dcc_info_serialization() {
        let info = DccInfo {
            dcc_type: "maya".to_string(),
            version: "2024.2".to_string(),
            python_version: Some("3.10.11".to_string()),
            platform: "windows".to_string(),
            pid: 12345,
            metadata: HashMap::from([("renderer".to_string(), "arnold".to_string())]),
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: DccInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.dcc_type, "maya");
        assert_eq!(deserialized.pid, 12345);
        assert_eq!(deserialized.metadata["renderer"], "arnold");
    }

    #[test]
    fn test_script_result_success() {
        let result = ScriptResult::success("42", 100);
        assert!(result.success);
        assert_eq!(result.output.as_deref(), Some("42"));
        assert!(result.error.is_none());
        assert_eq!(result.execution_time_ms, 100);
    }

    #[test]
    fn test_script_result_failure() {
        let result = ScriptResult::failure("NameError: undefined variable", 50);
        assert!(!result.success);
        assert!(result.output.is_none());
        assert_eq!(
            result.error.as_deref(),
            Some("NameError: undefined variable")
        );
    }

    #[test]
    fn test_script_language_display() {
        assert_eq!(ScriptLanguage::Python.to_string(), "python");
        assert_eq!(ScriptLanguage::Mel.to_string(), "mel");
        assert_eq!(ScriptLanguage::MaxScript.to_string(), "maxscript");
        assert_eq!(ScriptLanguage::HScript.to_string(), "hscript");
        assert_eq!(ScriptLanguage::Vex.to_string(), "vex");
        assert_eq!(ScriptLanguage::Lua.to_string(), "lua");
        assert_eq!(ScriptLanguage::CSharp.to_string(), "csharp");
        assert_eq!(ScriptLanguage::Blueprint.to_string(), "blueprint");
    }

    #[test]
    fn test_script_language_serialization_roundtrip() {
        let lang = ScriptLanguage::Python;
        let json = serde_json::to_string(&lang).unwrap();
        assert_eq!(json, "\"python\"");
        let deserialized: ScriptLanguage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ScriptLanguage::Python);
    }

    #[test]
    fn test_scene_info_default() {
        let scene = SceneInfo::default();
        assert!(scene.file_path.is_empty());
        assert!(!scene.modified);
        assert!(scene.frame_range.is_none());
    }

    #[test]
    fn test_scene_info_serialization() {
        let scene = SceneInfo {
            file_path: "/projects/shot_010.ma".to_string(),
            name: "shot_010".to_string(),
            modified: true,
            format: ".ma".to_string(),
            frame_range: Some((1.0, 120.0)),
            current_frame: Some(24.0),
            fps: Some(24.0),
            up_axis: Some("y".to_string()),
            units: Some("cm".to_string()),
            statistics: SceneStatistics {
                object_count: 150,
                vertex_count: 500_000,
                polygon_count: 250_000,
                material_count: 20,
                texture_count: 45,
                light_count: 5,
                camera_count: 3,
            },
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&scene).unwrap();
        let deserialized: SceneInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "shot_010");
        assert!(deserialized.modified);
        assert_eq!(deserialized.statistics.vertex_count, 500_000);
        assert_eq!(deserialized.frame_range, Some((1.0, 120.0)));
    }

    #[test]
    fn test_scene_statistics_default() {
        let stats = SceneStatistics::default();
        assert_eq!(stats.object_count, 0);
        assert_eq!(stats.vertex_count, 0);
        assert_eq!(stats.polygon_count, 0);
    }

    #[test]
    fn test_dcc_capabilities_default() {
        let caps = DccCapabilities::default();
        assert!(caps.script_languages.is_empty());
        assert!(!caps.scene_info);
        assert!(!caps.snapshot);
        assert!(!caps.undo_redo);
    }

    #[test]
    fn test_dcc_capabilities_serialization() {
        let caps = DccCapabilities {
            script_languages: vec![ScriptLanguage::Python, ScriptLanguage::Mel],
            scene_info: true,
            snapshot: true,
            undo_redo: true,
            progress_reporting: false,
            file_operations: true,
            selection: true,
            extensions: HashMap::from([("usd_export".to_string(), true)]),
        };
        let json = serde_json::to_string(&caps).unwrap();
        let deserialized: DccCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.script_languages.len(), 2);
        assert!(deserialized.scene_info);
        assert!(deserialized.extensions["usd_export"]);
    }

    #[test]
    fn test_dcc_error_display() {
        let err = DccError {
            code: DccErrorCode::ScriptError,
            message: "NameError: x is not defined".to_string(),
            details: Some("Traceback...".to_string()),
            recoverable: true,
        };
        assert_eq!(
            err.to_string(),
            "[SCRIPT_ERROR] NameError: x is not defined"
        );
    }

    #[test]
    fn test_dcc_error_code_display() {
        assert_eq!(
            DccErrorCode::ConnectionFailed.to_string(),
            "CONNECTION_FAILED"
        );
        assert_eq!(DccErrorCode::Timeout.to_string(), "TIMEOUT");
        assert_eq!(DccErrorCode::ScriptError.to_string(), "SCRIPT_ERROR");
        assert_eq!(DccErrorCode::NotResponding.to_string(), "NOT_RESPONDING");
        assert_eq!(DccErrorCode::Unsupported.to_string(), "UNSUPPORTED");
        assert_eq!(
            DccErrorCode::PermissionDenied.to_string(),
            "PERMISSION_DENIED"
        );
        assert_eq!(DccErrorCode::InvalidInput.to_string(), "INVALID_INPUT");
        assert_eq!(DccErrorCode::SceneError.to_string(), "SCENE_ERROR");
        assert_eq!(DccErrorCode::Internal.to_string(), "INTERNAL");
    }

    #[test]
    fn test_dcc_error_serialization() {
        let err = DccError {
            code: DccErrorCode::ConnectionFailed,
            message: "Connection refused".to_string(),
            details: None,
            recoverable: true,
        };
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: DccError = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.code, DccErrorCode::ConnectionFailed);
        assert!(deserialized.recoverable);
    }

    // ── Mock adapter test ──

    struct MockDccAdapter {
        info: DccInfo,
    }

    impl MockDccAdapter {
        fn new() -> Self {
            Self {
                info: DccInfo {
                    dcc_type: "mock".to_string(),
                    version: "1.0.0".to_string(),
                    python_version: Some("3.11".to_string()),
                    platform: "windows".to_string(),
                    pid: 1234,
                    metadata: HashMap::new(),
                },
            }
        }
    }

    impl DccAdapter for MockDccAdapter {
        fn info(&self) -> &DccInfo {
            &self.info
        }

        fn capabilities(&self) -> DccCapabilities {
            DccCapabilities {
                script_languages: vec![ScriptLanguage::Python],
                scene_info: false,
                snapshot: false,
                ..Default::default()
            }
        }

        fn as_connection(&mut self) -> Option<&mut dyn DccConnection> {
            None
        }

        fn as_script_engine(&self) -> Option<&dyn DccScriptEngine> {
            None
        }

        fn as_scene_info(&self) -> Option<&dyn DccSceneInfo> {
            None
        }

        fn as_snapshot(&self) -> Option<&dyn DccSnapshot> {
            None
        }
    }

    #[test]
    fn test_mock_adapter() {
        let adapter = MockDccAdapter::new();
        assert_eq!(adapter.info().dcc_type, "mock");
        assert_eq!(adapter.capabilities().script_languages.len(), 1);
        assert!(!adapter.capabilities().scene_info);
    }

    #[test]
    fn test_mock_adapter_optional_sub_traits() {
        let mut adapter = MockDccAdapter::new();
        assert!(adapter.as_connection().is_none());
        assert!(adapter.as_script_engine().is_none());
        assert!(adapter.as_scene_info().is_none());
        assert!(adapter.as_snapshot().is_none());
    }

    // ── Capture result test ──

    #[test]
    fn test_capture_result() {
        let capture = CaptureResult {
            data: vec![0x89, 0x50, 0x4E, 0x47], // PNG magic bytes
            width: 1920,
            height: 1080,
            format: "png".to_string(),
            viewport: Some("persp".to_string()),
        };
        assert_eq!(capture.width, 1920);
        assert_eq!(capture.height, 1080);
        assert_eq!(capture.viewport.as_deref(), Some("persp"));
    }

    #[test]
    fn test_dcc_info_no_python_version() {
        let info = DccInfo {
            dcc_type: "unity".to_string(),
            version: "2022.3".to_string(),
            python_version: None,
            platform: "windows".to_string(),
            pid: 99999,
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: DccInfo = serde_json::from_str(&json).unwrap();
        assert!(back.python_version.is_none());
        assert_eq!(back.dcc_type, "unity");
    }

    #[test]
    fn test_script_result_context_field() {
        let mut result = ScriptResult::success("done", 10);
        result
            .context
            .insert("node".to_string(), "pSphere1".to_string());
        assert_eq!(result.context["node"], "pSphere1");
        let json = serde_json::to_string(&result).unwrap();
        let back: ScriptResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.context["node"], "pSphere1");
    }

    #[test]
    fn test_capture_result_no_viewport() {
        let capture = CaptureResult {
            data: vec![1, 2, 3],
            width: 640,
            height: 480,
            format: "jpeg".to_string(),
            viewport: None,
        };
        assert!(capture.viewport.is_none());
    }

    #[test]
    fn test_dcc_error_not_recoverable() {
        let err = DccError {
            code: DccErrorCode::Internal,
            message: "Fatal error".to_string(),
            details: None,
            recoverable: false,
        };
        assert!(!err.recoverable);
        assert!(err.details.is_none());
        assert!(err.to_string().contains("INTERNAL"));
    }
}
