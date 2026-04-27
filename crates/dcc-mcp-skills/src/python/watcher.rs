//! Python bindings for [`SkillWatcher`].
//!
//! Only compiled when the `python-bindings` Cargo feature is enabled.

use std::time::Duration;

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

use dcc_mcp_models::SkillMetadata;

use crate::watcher::SkillWatcher;

/// Python-facing wrapper for [`SkillWatcher`].
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "SkillWatcher")]
pub struct PySkillWatcher {
    inner: SkillWatcher,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PySkillWatcher {
    /// Create a new SkillWatcher.
    ///
    /// Args:
    ///     debounce_ms: Milliseconds to wait before reloading after a change
    ///                  (default: 300).
    #[new]
    #[pyo3(signature = (debounce_ms=300))]
    pub fn new(debounce_ms: u64) -> pyo3::PyResult<Self> {
        let watcher = SkillWatcher::new(Duration::from_millis(debounce_ms))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self { inner: watcher })
    }

    /// Start watching *path* for skill changes.
    ///
    /// An immediate reload is performed so skills are available without waiting
    /// for a filesystem event.
    ///
    /// Raises:
    ///     RuntimeError: If the path cannot be watched.
    pub fn watch(&mut self, path: &str) -> pyo3::PyResult<()> {
        self.inner
            .watch(path)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Stop watching *path*.
    ///
    /// Returns ``True`` if the path was being watched, ``False`` otherwise.
    pub fn unwatch(&mut self, path: &str) -> bool {
        self.inner.unwatch(path)
    }

    /// Return the current skill snapshot as a list.
    pub fn skills(&self) -> Vec<SkillMetadata> {
        self.inner.skills()
    }

    /// Return the number of loaded skills.
    pub fn skill_count(&self) -> usize {
        self.inner.skill_count()
    }

    /// Return the list of watched directory paths.
    pub fn watched_paths(&self) -> Vec<String> {
        self.inner
            .watched_paths()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }

    /// Manually trigger a reload.
    pub fn reload(&self) {
        self.inner.reload();
    }

    fn __repr__(&self) -> String {
        format!(
            "SkillWatcher(skills={}, paths={})",
            self.inner.skill_count(),
            self.inner.watched_paths().len()
        )
    }
}
