//! Python-facing data structs for DCC core adapter types.
//!
//! Exports `PyDccInfo`, `PyScriptResult`, `PySceneStatistics`, `PySceneInfo`,
//! `PyDccCapabilities`, `PyDccError`, and `PyCaptureResult`.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

#[cfg(feature = "python-bindings")]
use std::collections::HashMap;

#[cfg(feature = "python-bindings")]
use crate::adapters::{
    BridgeKind, CaptureResult, DccCapabilities, DccError, DccInfo, SceneInfo, SceneStatistics,
    ScriptResult,
};

#[cfg(feature = "python-bindings")]
use super::super::enums::{PyDccErrorCode, PyScriptLanguage};

// ── PyDccInfo ──

/// Python-facing DCC application information.
///
/// ```python
/// from dcc_mcp_core import DccInfo
///
/// info = DccInfo(
///     dcc_type="maya",
///     version="2024.2",
///     platform="windows",
///     pid=12345,
///     python_version="3.10.11",
/// )
/// print(info.dcc_type)  # "maya"
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "DccInfo", get_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyDccInfo {
    pub dcc_type: String,
    pub version: String,
    pub python_version: Option<String>,
    pub platform: String,
    pub pid: u32,
    pub metadata: HashMap<String, String>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyDccInfo {
    #[new]
    #[pyo3(signature = (dcc_type, version, platform, pid, python_version=None, metadata=None))]
    fn new(
        dcc_type: String,
        version: String,
        platform: String,
        pid: u32,
        python_version: Option<String>,
        metadata: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            dcc_type,
            version,
            python_version,
            platform,
            pid,
            metadata: metadata.unwrap_or_default(),
        }
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("dcc_type", &self.dcc_type)?;
        dict.set_item("version", &self.version)?;
        dict.set_item("python_version", &self.python_version)?;
        dict.set_item("platform", &self.platform)?;
        dict.set_item("pid", self.pid)?;
        dict.set_item("metadata", &self.metadata)?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "DccInfo(dcc_type={:?}, version={:?}, pid={})",
            self.dcc_type, self.version, self.pid
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&DccInfo> for PyDccInfo {
    fn from(info: &DccInfo) -> Self {
        Self {
            dcc_type: info.dcc_type.clone(),
            version: info.version.clone(),
            python_version: info.python_version.clone(),
            platform: info.platform.clone(),
            pid: info.pid,
            metadata: info.metadata.clone(),
        }
    }
}

// ── PyScriptResult ──

/// Python-facing script execution result.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ScriptResult", get_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyScriptResult {
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub execution_time_ms: u64,
    pub context: HashMap<String, String>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyScriptResult {
    #[new]
    #[pyo3(signature = (success, execution_time_ms, output=None, error=None, context=None))]
    fn new(
        success: bool,
        execution_time_ms: u64,
        output: Option<String>,
        error: Option<String>,
        context: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            success,
            output,
            error,
            execution_time_ms,
            context: context.unwrap_or_default(),
        }
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("success", self.success)?;
        dict.set_item("output", &self.output)?;
        dict.set_item("error", &self.error)?;
        dict.set_item("execution_time_ms", self.execution_time_ms)?;
        dict.set_item("context", &self.context)?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "ScriptResult(success={}, time={}ms)",
            self.success, self.execution_time_ms
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&ScriptResult> for PyScriptResult {
    fn from(result: &ScriptResult) -> Self {
        Self {
            success: result.success,
            output: result.output.clone(),
            error: result.error.clone(),
            execution_time_ms: result.execution_time_ms,
            context: result.context.clone(),
        }
    }
}

// ── PySceneStatistics ──

/// Python-facing scene statistics.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "SceneStatistics", get_all, set_all, from_py_object)]
#[derive(Debug, Clone, Default)]
pub struct PySceneStatistics {
    pub object_count: u64,
    pub vertex_count: u64,
    pub polygon_count: u64,
    pub material_count: u64,
    pub texture_count: u64,
    pub light_count: u64,
    pub camera_count: u64,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PySceneStatistics {
    #[new]
    #[pyo3(signature = (
        object_count=0, vertex_count=0, polygon_count=0,
        material_count=0, texture_count=0, light_count=0, camera_count=0
    ))]
    fn new(
        object_count: u64,
        vertex_count: u64,
        polygon_count: u64,
        material_count: u64,
        texture_count: u64,
        light_count: u64,
        camera_count: u64,
    ) -> Self {
        Self {
            object_count,
            vertex_count,
            polygon_count,
            material_count,
            texture_count,
            light_count,
            camera_count,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "SceneStatistics(objects={}, verts={}, polys={})",
            self.object_count, self.vertex_count, self.polygon_count
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&SceneStatistics> for PySceneStatistics {
    fn from(stats: &SceneStatistics) -> Self {
        Self {
            object_count: stats.object_count,
            vertex_count: stats.vertex_count,
            polygon_count: stats.polygon_count,
            material_count: stats.material_count,
            texture_count: stats.texture_count,
            light_count: stats.light_count,
            camera_count: stats.camera_count,
        }
    }
}

// ── PySceneInfo ──

/// Python-facing scene information.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "SceneInfo", get_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PySceneInfo {
    pub file_path: String,
    pub name: String,
    pub modified: bool,
    pub format: String,
    pub frame_range: Option<(f64, f64)>,
    pub current_frame: Option<f64>,
    pub fps: Option<f64>,
    pub up_axis: Option<String>,
    pub units: Option<String>,
    pub statistics: PySceneStatistics,
    pub metadata: HashMap<String, String>,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PySceneInfo {
    #[new]
    #[pyo3(signature = (
        file_path="".to_string(), name="untitled".to_string(), modified=false,
        format="".to_string(), frame_range=None, current_frame=None,
        fps=None, up_axis=None, units=None, statistics=None, metadata=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        file_path: String,
        name: String,
        modified: bool,
        format: String,
        frame_range: Option<(f64, f64)>,
        current_frame: Option<f64>,
        fps: Option<f64>,
        up_axis: Option<String>,
        units: Option<String>,
        statistics: Option<PySceneStatistics>,
        metadata: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            file_path,
            name,
            modified,
            format,
            frame_range,
            current_frame,
            fps,
            up_axis,
            units,
            statistics: statistics.unwrap_or_default(),
            metadata: metadata.unwrap_or_default(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "SceneInfo(name={:?}, modified={}, objects={})",
            self.name, self.modified, self.statistics.object_count
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&SceneInfo> for PySceneInfo {
    fn from(info: &SceneInfo) -> Self {
        Self {
            file_path: info.file_path.clone(),
            name: info.name.clone(),
            modified: info.modified,
            format: info.format.clone(),
            frame_range: info.frame_range,
            current_frame: info.current_frame,
            fps: info.fps,
            up_axis: info.up_axis.clone(),
            units: info.units.clone(),
            statistics: PySceneStatistics::from(&info.statistics),
            metadata: info.metadata.clone(),
        }
    }
}

// ── PyDccCapabilities ──

/// Python-facing DCC capabilities declaration.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "DccCapabilities", get_all, set_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyDccCapabilities {
    pub script_languages: Vec<PyScriptLanguage>,
    pub scene_info: bool,
    pub snapshot: bool,
    pub undo_redo: bool,
    pub progress_reporting: bool,
    pub file_operations: bool,
    pub selection: bool,
    /// Whether the adapter implements `DccSceneManager` (scene/file management).
    pub scene_manager: bool,
    /// Whether the adapter implements `DccTransform` (object TRS transforms).
    pub transform: bool,
    /// Whether the adapter implements `DccRenderCapture` (viewport capture + render).
    pub render_capture: bool,
    /// Whether the adapter implements `DccHierarchy` (parent/child hierarchy).
    pub hierarchy: bool,
    /// Whether the DCC has an embedded Python interpreter.
    /// `False` for bridge-based DCCs (ZBrush, Photoshop).
    pub has_embedded_python: bool,
    /// Bridge kind string: `"http"`, `"websocket"`, `"named_pipe"`, or `None`.
    pub bridge_kind: Option<String>,
    /// Bridge endpoint (URL or socket path).
    pub bridge_endpoint: Option<String>,
    pub extensions: HashMap<String, bool>,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyDccCapabilities {
    #[new]
    #[pyo3(signature = (
        script_languages=vec![], scene_info=false, snapshot=false,
        undo_redo=false, progress_reporting=false, file_operations=false,
        selection=false, scene_manager=false, transform=false,
        render_capture=false, hierarchy=false,
        has_embedded_python=true, bridge_kind=None, bridge_endpoint=None,
        extensions=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        script_languages: Vec<PyScriptLanguage>,
        scene_info: bool,
        snapshot: bool,
        undo_redo: bool,
        progress_reporting: bool,
        file_operations: bool,
        selection: bool,
        scene_manager: bool,
        transform: bool,
        render_capture: bool,
        hierarchy: bool,
        has_embedded_python: bool,
        bridge_kind: Option<String>,
        bridge_endpoint: Option<String>,
        extensions: Option<HashMap<String, bool>>,
    ) -> Self {
        Self {
            script_languages,
            scene_info,
            snapshot,
            undo_redo,
            progress_reporting,
            file_operations,
            selection,
            scene_manager,
            transform,
            render_capture,
            hierarchy,
            has_embedded_python,
            bridge_kind,
            bridge_endpoint,
            extensions: extensions.unwrap_or_default(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "DccCapabilities(languages={}, scene_info={}, snapshot={}, scene_manager={}, transform={}, render_capture={}, hierarchy={}, has_embedded_python={}, bridge_kind={:?})",
            self.script_languages.len(),
            self.scene_info,
            self.snapshot,
            self.scene_manager,
            self.transform,
            self.render_capture,
            self.hierarchy,
            self.has_embedded_python,
            self.bridge_kind,
        )
    }

    /// Check if this DCC uses a bridge connection (i.e., no embedded Python).
    ///
    /// Returns ``True`` if ``bridge_kind`` is set (http, websocket, named_pipe, or custom).
    fn uses_bridge(&self) -> bool {
        self.bridge_kind.is_some()
    }

    /// Create a DccCapabilities for an HTTP bridge-based DCC.
    ///
    /// Args:
    ///     endpoint: HTTP endpoint URL (e.g., ``"http://localhost:8765"``).
    ///
    /// Example::
    ///
    ///     caps = DccCapabilities.http_bridge("http://localhost:8765")
    ///     assert caps.bridge_kind == "http"
    ///     assert caps.uses_bridge()
    #[staticmethod]
    fn http_bridge(endpoint: String) -> Self {
        Self {
            script_languages: vec![],
            scene_info: false,
            snapshot: false,
            undo_redo: false,
            progress_reporting: false,
            file_operations: false,
            selection: false,
            scene_manager: false,
            transform: false,
            render_capture: false,
            hierarchy: false,
            has_embedded_python: false,
            bridge_kind: Some("http".to_string()),
            bridge_endpoint: Some(endpoint),
            extensions: HashMap::new(),
        }
    }

    /// Create a DccCapabilities for a WebSocket bridge-based DCC.
    ///
    /// Args:
    ///     endpoint: WebSocket endpoint URL (e.g., ``"ws://localhost:9001"``).
    ///
    /// Example::
    ///
    ///     caps = DccCapabilities.websocket_bridge("ws://localhost:9001")
    ///     assert caps.bridge_kind == "websocket"
    ///     assert caps.uses_bridge()
    #[staticmethod]
    fn websocket_bridge(endpoint: String) -> Self {
        Self {
            script_languages: vec![],
            scene_info: false,
            snapshot: false,
            undo_redo: false,
            progress_reporting: false,
            file_operations: false,
            selection: false,
            scene_manager: false,
            transform: false,
            render_capture: false,
            hierarchy: false,
            has_embedded_python: false,
            bridge_kind: Some("websocket".to_string()),
            bridge_endpoint: Some(endpoint),
            extensions: HashMap::new(),
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<&DccCapabilities> for PyDccCapabilities {
    fn from(caps: &DccCapabilities) -> Self {
        Self {
            script_languages: caps
                .script_languages
                .iter()
                .map(|l| PyScriptLanguage::from(*l))
                .collect(),
            scene_info: caps.scene_info,
            snapshot: caps.snapshot,
            undo_redo: caps.undo_redo,
            progress_reporting: caps.progress_reporting,
            file_operations: caps.file_operations,
            selection: caps.selection,
            scene_manager: caps.scene_manager,
            transform: caps.transform,
            render_capture: caps.render_capture,
            hierarchy: caps.hierarchy,
            has_embedded_python: caps.has_embedded_python,
            bridge_kind: caps.bridge_kind.as_ref().map(|k| match k {
                BridgeKind::Http => "http".to_string(),
                BridgeKind::WebSocket => "websocket".to_string(),
                BridgeKind::NamedPipe => "named_pipe".to_string(),
                BridgeKind::Custom(s) => s.clone(),
            }),
            bridge_endpoint: caps.bridge_endpoint.clone(),
            extensions: caps.extensions.clone(),
        }
    }
}

// ── PyDccError ──

/// Python-facing DCC error.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "DccError", get_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyDccError {
    pub code: PyDccErrorCode,
    pub message: String,
    pub details: Option<String>,
    pub recoverable: bool,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyDccError {
    #[new]
    #[pyo3(signature = (code, message, details=None, recoverable=false))]
    fn new(
        code: PyDccErrorCode,
        message: String,
        details: Option<String>,
        recoverable: bool,
    ) -> Self {
        Self {
            code,
            message,
            details,
            recoverable,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "DccError(code={}, message={:?}, recoverable={})",
            self.code.as_str(),
            self.message,
            self.recoverable,
        )
    }

    fn __str__(&self) -> String {
        format!("[{}] {}", self.code.as_str(), self.message)
    }
}

#[cfg(feature = "python-bindings")]
impl From<&DccError> for PyDccError {
    fn from(err: &DccError) -> Self {
        Self {
            code: PyDccErrorCode::from(err.code),
            message: err.message.clone(),
            details: err.details.clone(),
            recoverable: err.recoverable,
        }
    }
}

// ── PyCaptureResult ──

/// Python-facing capture/screenshot result.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "CaptureResult", get_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyCaptureResult {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub viewport: Option<String>,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyCaptureResult {
    #[new]
    #[pyo3(signature = (data, width, height, format, viewport=None))]
    fn new(
        data: Vec<u8>,
        width: u32,
        height: u32,
        format: String,
        viewport: Option<String>,
    ) -> Self {
        Self {
            data,
            width,
            height,
            format,
            viewport,
        }
    }

    /// Get the image data size in bytes.
    fn data_size(&self) -> usize {
        self.data.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "CaptureResult({}x{}, format={:?}, size={})",
            self.width,
            self.height,
            self.format,
            self.data.len()
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&CaptureResult> for PyCaptureResult {
    fn from(result: &CaptureResult) -> Self {
        Self {
            data: result.data.clone(),
            width: result.width,
            height: result.height,
            format: result.format.clone(),
            viewport: result.viewport.clone(),
        }
    }
}
