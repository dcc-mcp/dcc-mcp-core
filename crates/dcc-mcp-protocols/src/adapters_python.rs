//! Python bindings for DCC adapter data types via PyO3.
//!
//! Exposes `PyDccInfo`, `PyScriptResult`, `PyScriptLanguage`, `PySceneInfo`,
//! `PySceneStatistics`, `PyDccCapabilities`, `PyDccError`, and `PyDccErrorCode`
//! as Python classes.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;

#[cfg(feature = "python-bindings")]
use std::collections::HashMap;

#[cfg(feature = "python-bindings")]
use crate::adapters::{
    CaptureResult, DccCapabilities, DccError, DccErrorCode, DccInfo, SceneInfo, SceneStatistics,
    ScriptLanguage, ScriptResult,
};

// ── PyScriptLanguage ──

/// Python-facing enum for DCC script languages.
///
/// ```python
/// from dcc_mcp_core import ScriptLanguage
///
/// lang = ScriptLanguage.PYTHON
/// print(lang)  # "python"
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ScriptLanguage", eq, from_py_object)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PyScriptLanguage {
    #[pyo3(name = "PYTHON")]
    Python,
    #[pyo3(name = "MEL")]
    Mel,
    #[pyo3(name = "MAXSCRIPT")]
    MaxScript,
    #[pyo3(name = "HSCRIPT")]
    HScript,
    #[pyo3(name = "VEX")]
    Vex,
    #[pyo3(name = "LUA")]
    Lua,
    #[pyo3(name = "CSHARP")]
    CSharp,
    #[pyo3(name = "BLUEPRINT")]
    Blueprint,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyScriptLanguage {
    fn __repr__(&self) -> String {
        format!("ScriptLanguage.{}", self.__str__())
    }

    fn __str__(&self) -> &'static str {
        match self {
            Self::Python => "PYTHON",
            Self::Mel => "MEL",
            Self::MaxScript => "MAXSCRIPT",
            Self::HScript => "HSCRIPT",
            Self::Vex => "VEX",
            Self::Lua => "LUA",
            Self::CSharp => "CSHARP",
            Self::Blueprint => "BLUEPRINT",
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<ScriptLanguage> for PyScriptLanguage {
    fn from(lang: ScriptLanguage) -> Self {
        match lang {
            ScriptLanguage::Python => PyScriptLanguage::Python,
            ScriptLanguage::Mel => PyScriptLanguage::Mel,
            ScriptLanguage::MaxScript => PyScriptLanguage::MaxScript,
            ScriptLanguage::HScript => PyScriptLanguage::HScript,
            ScriptLanguage::Vex => PyScriptLanguage::Vex,
            ScriptLanguage::Lua => PyScriptLanguage::Lua,
            ScriptLanguage::CSharp => PyScriptLanguage::CSharp,
            ScriptLanguage::Blueprint => PyScriptLanguage::Blueprint,
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<&PyScriptLanguage> for ScriptLanguage {
    fn from(lang: &PyScriptLanguage) -> Self {
        match lang {
            PyScriptLanguage::Python => ScriptLanguage::Python,
            PyScriptLanguage::Mel => ScriptLanguage::Mel,
            PyScriptLanguage::MaxScript => ScriptLanguage::MaxScript,
            PyScriptLanguage::HScript => ScriptLanguage::HScript,
            PyScriptLanguage::Vex => ScriptLanguage::Vex,
            PyScriptLanguage::Lua => ScriptLanguage::Lua,
            PyScriptLanguage::CSharp => ScriptLanguage::CSharp,
            PyScriptLanguage::Blueprint => ScriptLanguage::Blueprint,
        }
    }
}

// ── PyDccErrorCode ──

/// Python-facing enum for DCC error codes.
#[cfg(feature = "python-bindings")]
#[pyclass(name = "DccErrorCode", eq, from_py_object)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PyDccErrorCode {
    #[pyo3(name = "CONNECTION_FAILED")]
    ConnectionFailed,
    #[pyo3(name = "TIMEOUT")]
    Timeout,
    #[pyo3(name = "SCRIPT_ERROR")]
    ScriptError,
    #[pyo3(name = "NOT_RESPONDING")]
    NotResponding,
    #[pyo3(name = "UNSUPPORTED")]
    Unsupported,
    #[pyo3(name = "PERMISSION_DENIED")]
    PermissionDenied,
    #[pyo3(name = "INVALID_INPUT")]
    InvalidInput,
    #[pyo3(name = "SCENE_ERROR")]
    SceneError,
    #[pyo3(name = "INTERNAL")]
    Internal,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyDccErrorCode {
    fn __repr__(&self) -> String {
        format!("DccErrorCode.{}", self.__str__())
    }

    fn __str__(&self) -> &'static str {
        match self {
            Self::ConnectionFailed => "CONNECTION_FAILED",
            Self::Timeout => "TIMEOUT",
            Self::ScriptError => "SCRIPT_ERROR",
            Self::NotResponding => "NOT_RESPONDING",
            Self::Unsupported => "UNSUPPORTED",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::InvalidInput => "INVALID_INPUT",
            Self::SceneError => "SCENE_ERROR",
            Self::Internal => "INTERNAL",
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<DccErrorCode> for PyDccErrorCode {
    fn from(code: DccErrorCode) -> Self {
        match code {
            DccErrorCode::ConnectionFailed => PyDccErrorCode::ConnectionFailed,
            DccErrorCode::Timeout => PyDccErrorCode::Timeout,
            DccErrorCode::ScriptError => PyDccErrorCode::ScriptError,
            DccErrorCode::NotResponding => PyDccErrorCode::NotResponding,
            DccErrorCode::Unsupported => PyDccErrorCode::Unsupported,
            DccErrorCode::PermissionDenied => PyDccErrorCode::PermissionDenied,
            DccErrorCode::InvalidInput => PyDccErrorCode::InvalidInput,
            DccErrorCode::SceneError => PyDccErrorCode::SceneError,
            DccErrorCode::Internal => PyDccErrorCode::Internal,
        }
    }
}

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
    pub extensions: HashMap<String, bool>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyDccCapabilities {
    #[new]
    #[pyo3(signature = (
        script_languages=vec![], scene_info=false, snapshot=false,
        undo_redo=false, progress_reporting=false, file_operations=false,
        selection=false, extensions=None
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
            extensions: extensions.unwrap_or_default(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "DccCapabilities(languages={}, scene_info={}, snapshot={})",
            self.script_languages.len(),
            self.scene_info,
            self.snapshot
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
            self.code.__str__(),
            self.message,
            self.recoverable,
        )
    }

    fn __str__(&self) -> String {
        format!("[{}] {}", self.code.__str__(), self.message)
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_compiles() {
        // Compilation test — the Python bindings are gated behind the feature flag,
        // so we only verify the module compiles in default (non-binding) mode.
        // assert!(true) is a no-op; use a meaningful check instead.
        let _ = 1 + 1;
    }
}
