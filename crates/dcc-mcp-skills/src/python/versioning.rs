//! PyO3 bindings for the skill-version manifest.

use std::path::Path;

use pyo3::prelude::*;

use crate::versioning::{SkillVersionManifest, get_skill_version_manifest};

#[pyfunction]
#[pyo3(name = "get_skill_version_manifest")]
pub fn py_get_skill_version_manifest(skill_dir: &str) -> PyResult<SkillVersionManifest> {
    Ok(get_skill_version_manifest(Path::new(skill_dir))?)
}
