//! Python-facing data structs for DCC adapter types.
//!
//! Exports `PyDccInfo`, `PyScriptResult`, `PySceneStatistics`, `PySceneInfo`,
//! `PyDccCapabilities`, `PyDccError`, `PyCaptureResult`, `PyObjectTransform`,
//! `PyBoundingBox`, `PySceneObject`, `PyFrameRange`, and `PyRenderOutput`.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;

#[cfg(feature = "python-bindings")]
use std::collections::HashMap;

#[cfg(feature = "python-bindings")]
use crate::adapters::{
    BoundingBox, BridgeKind, CaptureResult, DccCapabilities, DccError, DccInfo, FrameRange,
    ObjectTransform, RenderOutput, SceneInfo, SceneObject, SceneStatistics, ScriptResult,
};

#[cfg(feature = "python-bindings")]
use super::enums::{PyDccErrorCode, PyScriptLanguage};

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
#[cfg(feature = "python-bindings")]
#[pyclass(name = "DccError", get_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyDccError {
    pub code: PyDccErrorCode,
    pub message: String,
    pub details: Option<String>,
    pub recoverable: bool,
}

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

// ── PyObjectTransform ──

/// Python-facing 3D object transform (TRS).
///
/// ```python
/// from dcc_mcp_core import ObjectTransform
///
/// t = ObjectTransform(translate=[0.0, 10.0, 0.0], rotate=[0.0, 45.0, 0.0], scale=[1.0, 1.0, 1.0])
/// print(t.translate)  # [0.0, 10.0, 0.0]
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ObjectTransform", get_all, set_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyObjectTransform {
    /// World-space translation [x, y, z] in centimeters.
    pub translate: [f64; 3],
    /// Euler XYZ rotation in degrees [rx, ry, rz].
    pub rotate: [f64; 3],
    /// Scale [sx, sy, sz].
    pub scale: [f64; 3],
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyObjectTransform {
    #[new]
    #[pyo3(signature = (translate=None, rotate=None, scale=None))]
    fn new(translate: Option<[f64; 3]>, rotate: Option<[f64; 3]>, scale: Option<[f64; 3]>) -> Self {
        Self {
            translate: translate.unwrap_or([0.0, 0.0, 0.0]),
            rotate: rotate.unwrap_or([0.0, 0.0, 0.0]),
            scale: scale.unwrap_or([1.0, 1.0, 1.0]),
        }
    }

    /// Create an identity transform (no translation/rotation, unit scale).
    #[staticmethod]
    fn identity() -> Self {
        Self {
            translate: [0.0, 0.0, 0.0],
            rotate: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("translate", self.translate.to_vec())?;
        dict.set_item("rotate", self.rotate.to_vec())?;
        dict.set_item("scale", self.scale.to_vec())?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "ObjectTransform(translate={:?}, rotate={:?}, scale={:?})",
            self.translate, self.rotate, self.scale
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&ObjectTransform> for PyObjectTransform {
    fn from(t: &ObjectTransform) -> Self {
        Self {
            translate: t.translate,
            rotate: t.rotate,
            scale: t.scale,
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<&PyObjectTransform> for ObjectTransform {
    fn from(t: &PyObjectTransform) -> Self {
        Self {
            translate: t.translate,
            rotate: t.rotate,
            scale: t.scale,
        }
    }
}

// ── PyBoundingBox ──

/// Python-facing axis-aligned bounding box.
///
/// ```python
/// from dcc_mcp_core import BoundingBox
///
/// bb = BoundingBox(min=[-1.0, 0.0, -1.0], max=[1.0, 2.0, 1.0])
/// print(bb.center())  # [0.0, 1.0, 0.0]
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "BoundingBox", get_all, set_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyBoundingBox {
    /// Minimum corner [x, y, z] in world space (centimeters).
    pub min: [f64; 3],
    /// Maximum corner [x, y, z] in world space (centimeters).
    pub max: [f64; 3],
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyBoundingBox {
    #[new]
    #[pyo3(signature = (min=None, max=None))]
    fn new(min: Option<[f64; 3]>, max: Option<[f64; 3]>) -> Self {
        Self {
            min: min.unwrap_or([0.0, 0.0, 0.0]),
            max: max.unwrap_or([0.0, 0.0, 0.0]),
        }
    }

    /// Compute the center of the bounding box.
    fn center(&self) -> [f64; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// Compute the size (extents) [width, height, depth].
    fn size(&self) -> [f64; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("min", self.min.to_vec())?;
        dict.set_item("max", self.max.to_vec())?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!("BoundingBox(min={:?}, max={:?})", self.min, self.max)
    }
}

#[cfg(feature = "python-bindings")]
impl From<&BoundingBox> for PyBoundingBox {
    fn from(bb: &BoundingBox) -> Self {
        Self {
            min: bb.min,
            max: bb.max,
        }
    }
}

// ── PySceneObject ──

/// Python-facing lightweight scene object descriptor.
///
/// ```python
/// from dcc_mcp_core import SceneObject
///
/// obj = SceneObject(name="pCube1", long_name="|group1|pCube1", object_type="mesh")
/// print(obj.name)  # "pCube1"
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "SceneObject", get_all, set_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PySceneObject {
    pub name: String,
    pub long_name: String,
    pub object_type: String,
    pub parent: Option<String>,
    pub visible: bool,
    pub metadata: HashMap<String, String>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PySceneObject {
    #[new]
    #[pyo3(signature = (name, long_name=None, object_type="transform".to_string(), parent=None, visible=true, metadata=None))]
    fn new(
        name: String,
        long_name: Option<String>,
        object_type: String,
        parent: Option<String>,
        visible: bool,
        metadata: Option<HashMap<String, String>>,
    ) -> Self {
        let long_name = long_name.unwrap_or_else(|| name.clone());
        Self {
            name,
            long_name,
            object_type,
            parent,
            visible,
            metadata: metadata.unwrap_or_default(),
        }
    }

    pub fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("name", &self.name)?;
        dict.set_item("long_name", &self.long_name)?;
        dict.set_item("object_type", &self.object_type)?;
        dict.set_item("parent", &self.parent)?;
        dict.set_item("visible", self.visible)?;
        dict.set_item("metadata", &self.metadata)?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "SceneObject(name={:?}, type={:?}, visible={})",
            self.name, self.object_type, self.visible
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&SceneObject> for PySceneObject {
    fn from(obj: &SceneObject) -> Self {
        Self {
            name: obj.name.clone(),
            long_name: obj.long_name.clone(),
            object_type: obj.object_type.clone(),
            parent: obj.parent.clone(),
            visible: obj.visible,
            metadata: obj.metadata.clone(),
        }
    }
}

// ── PyFrameRange ──

/// Python-facing frame range and timing information.
///
/// ```python
/// from dcc_mcp_core import FrameRange
///
/// fr = FrameRange(start=1.0, end=240.0, fps=24.0, current=1.0)
/// print(fr.end - fr.start)  # 239.0
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "FrameRange", get_all, set_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyFrameRange {
    pub start: f64,
    pub end: f64,
    pub fps: f64,
    pub current: f64,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyFrameRange {
    #[new]
    #[pyo3(signature = (start=1.0, end=100.0, fps=24.0, current=1.0))]
    fn new(start: f64, end: f64, fps: f64, current: f64) -> Self {
        Self {
            start,
            end,
            fps,
            current,
        }
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("start", self.start)?;
        dict.set_item("end", self.end)?;
        dict.set_item("fps", self.fps)?;
        dict.set_item("current", self.current)?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "FrameRange(start={}, end={}, fps={}, current={})",
            self.start, self.end, self.fps, self.current
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&FrameRange> for PyFrameRange {
    fn from(fr: &FrameRange) -> Self {
        Self {
            start: fr.start,
            end: fr.end,
            fps: fr.fps,
            current: fr.current,
        }
    }
}

// ── PyRenderOutput ──

/// Python-facing render output metadata.
///
/// ```python
/// from dcc_mcp_core import RenderOutput
///
/// out = RenderOutput(file_path="/renders/frame001.png", width=1920, height=1080,
///                    format="png", render_time_ms=5000)
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "RenderOutput", get_all, from_py_object)]
#[derive(Debug, Clone)]
pub struct PyRenderOutput {
    pub file_path: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub render_time_ms: u64,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyRenderOutput {
    #[new]
    #[pyo3(signature = (file_path, width, height, format, render_time_ms=0))]
    fn new(
        file_path: String,
        width: u32,
        height: u32,
        format: String,
        render_time_ms: u64,
    ) -> Self {
        Self {
            file_path,
            width,
            height,
            format,
            render_time_ms,
        }
    }

    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("file_path", &self.file_path)?;
        dict.set_item("width", self.width)?;
        dict.set_item("height", self.height)?;
        dict.set_item("format", &self.format)?;
        dict.set_item("render_time_ms", self.render_time_ms)?;
        Ok(dict.unbind().into_any())
    }

    fn __repr__(&self) -> String {
        format!(
            "RenderOutput({}x{}, format={:?}, time={}ms)",
            self.width, self.height, self.format, self.render_time_ms
        )
    }
}

#[cfg(feature = "python-bindings")]
impl From<&RenderOutput> for PyRenderOutput {
    fn from(out: &RenderOutput) -> Self {
        Self {
            file_path: out.file_path.clone(),
            width: out.width,
            height: out.height,
            format: out.format.clone(),
            render_time_ms: out.render_time_ms,
        }
    }
}
