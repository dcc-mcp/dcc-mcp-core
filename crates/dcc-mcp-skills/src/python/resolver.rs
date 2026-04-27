//! PyO3 bindings for the skill dependency resolver.

use pyo3::prelude::*;

use dcc_mcp_models::SkillMetadata;

use crate::resolver::{
    expand_transitive_dependencies, resolve_dependencies, validate_dependencies,
};

#[pyfunction]
#[pyo3(name = "resolve_dependencies")]
pub fn py_resolve_dependencies(skills: Vec<SkillMetadata>) -> PyResult<Vec<SkillMetadata>> {
    resolve_dependencies(&skills)
        .map(|r| r.ordered)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
}

#[pyfunction]
#[pyo3(name = "validate_dependencies")]
pub fn py_validate_dependencies(skills: Vec<SkillMetadata>) -> Vec<String> {
    validate_dependencies(&skills)
        .into_iter()
        .map(|e| e.to_string())
        .collect()
}

#[pyfunction]
#[pyo3(name = "expand_transitive_dependencies")]
pub fn py_expand_transitive_dependencies(
    skills: Vec<SkillMetadata>,
    skill_name: &str,
) -> PyResult<Vec<String>> {
    expand_transitive_dependencies(&skills, skill_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
}
