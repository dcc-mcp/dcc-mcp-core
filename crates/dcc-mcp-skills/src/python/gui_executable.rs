//! PyO3 bindings for `crate::gui_executable` (issue #524).

use std::path::{Path, PathBuf};

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};

use crate::gui_executable::{GuiExecutableHint, correct_python_executable, is_gui_executable};

/// Python-visible mirror of [`GuiExecutableHint`].
///
/// Constructed only by `is_gui_executable(...)`; not user-instantiable.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(
    name = "GuiExecutableHint",
    frozen,
    module = "dcc_mcp_core._core",
    skip_from_py_object
)]
#[derive(Clone)]
pub struct PyGuiExecutableHint {
    inner: GuiExecutableHint,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyGuiExecutableHint {
    /// The path that was probed.
    #[getter]
    fn gui_path(&self) -> PathBuf {
        self.inner.gui_path.clone()
    }

    /// DCC family name (`"maya"`, `"houdini"`, `"unreal"`, …).
    #[getter]
    fn dcc_kind(&self) -> &'static str {
        self.inner.dcc_kind
    }

    /// Recommended replacement path resolved from a sibling binary, or
    /// `None` when no headless equivalent exists / the sibling cannot
    /// be located on disk.
    #[getter]
    fn recommended_replacement(&self) -> Option<PathBuf> {
        self.inner.recommended_replacement.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "GuiExecutableHint(gui_path={:?}, dcc_kind={:?}, recommended_replacement={:?})",
            self.inner.gui_path, self.inner.dcc_kind, self.inner.recommended_replacement,
        )
    }
}

/// Detect a known DCC GUI binary. Returns `None` for Python interpreters
/// (`python.exe`, `mayapy`, `hython` …) and for unknown vendor binaries.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "is_gui_executable")]
pub fn py_is_gui_executable(path: &str) -> Option<PyGuiExecutableHint> {
    is_gui_executable(Path::new(path)).map(|inner| PyGuiExecutableHint { inner })
}

/// If `path` is a known DCC GUI binary with a headless-Python sibling on
/// disk, return that sibling path; otherwise return `path` unchanged.
/// Convenience for one-shot fixing of `DCC_MCP_PYTHON_EXECUTABLE`.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "correct_python_executable")]
pub fn py_correct_python_executable(path: &str) -> PathBuf {
    correct_python_executable(Path::new(path))
}
