//! PyO3 bindings for `SkillScanner`.

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyfunction, gen_stub_pymethods};

use crate::scanner::SkillScanner;

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl SkillScanner {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    #[pyo3(name = "scan")]
    #[pyo3(signature = (extra_paths=None, dcc_name=None, force_refresh=false))]
    fn py_scan(
        &mut self,
        extra_paths: Option<Vec<String>>,
        dcc_name: Option<&str>,
        force_refresh: bool,
    ) -> Vec<String> {
        self.scan(extra_paths.as_deref(), dcc_name, force_refresh)
    }

    #[pyo3(name = "clear_cache")]
    fn py_clear_cache(&mut self) {
        self.clear_cache()
    }

    #[getter]
    fn discovered_skills(&self) -> Vec<String> {
        self.skill_dirs.to_vec()
    }

    fn __repr__(&self) -> String {
        format!(
            "SkillScanner(cached={}, discovered={})",
            self.cache.len(),
            self.skill_dirs.len()
        )
    }
}

/// Convenience function: scan with a fresh scanner.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "scan_skill_paths")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_skill_paths(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> Vec<String> {
    let mut scanner = SkillScanner::new();
    scanner.scan(extra_paths.as_deref(), dcc_name, false)
}
