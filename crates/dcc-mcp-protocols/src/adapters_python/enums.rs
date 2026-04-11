//! Python-facing enumerations for DCC adapter types.
//!
//! Exports [`PyScriptLanguage`] and [`PyDccErrorCode`].

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "python-bindings")]
use crate::adapters::{DccErrorCode, ScriptLanguage};

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
        format!("DccErrorCode.{}", self.as_str())
    }

    fn __str__(&self) -> &'static str {
        self.as_str()
    }
}

#[cfg(feature = "python-bindings")]
impl PyDccErrorCode {
    /// Pure-Rust string representation (callable from Rust without going through PyO3).
    pub fn as_str(&self) -> &'static str {
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
